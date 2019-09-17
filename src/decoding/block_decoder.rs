use super::super::block::block::BlockHeader;
use super::super::block::block::BlockType;
use super::super::block::literals_section::LiteralsSection;
use super::super::block::literals_section::LiteralsSectionType;
use super::super::block::sequence_section::SequencesHeader;
use super::literals_section_decoder::decode_literals;
use super::sequence_section_decoder::decode_sequences;
use crate::decoding::scratch::DecoderScratch;
use crate::decoding::sequence_execution::execute_sequences;
use std::io::Read;

pub struct BlockDecoder {
    header_buffer: [u8; 3],
    internal_state: DecoderState,
}

enum DecoderState {
    ReadyToDecodeNextHeader,
    ReadyToDecodeNextBody,
    #[allow(dead_code)]
    Failed, //TODO put "self.internal_state = DecoderState::Failed;" everywhere a unresolveable error occurs
}

pub fn new() -> BlockDecoder {
    BlockDecoder {
        internal_state: DecoderState::ReadyToDecodeNextHeader,
        header_buffer: [0u8; 3],
    }
}

const ABSOLUTE_MAXIMUM_BLOCK_SIZE: u32 = 128 * 1024;

impl BlockDecoder {
    pub fn decode_block_content(
        &mut self,
        header: &BlockHeader,
        workspace: &mut DecoderScratch, //reuse this as often as possible. Not only if the trees are reused but also reuse the allocations when building new trees
        source: &mut Read,
    ) -> Result<u64, String> {
        match self.internal_state {
            DecoderState::ReadyToDecodeNextBody => {/* Happy :) */},
            DecoderState::Failed => return Err(format!("Cant decode next block if failed along the way. Results will be nonsense")),
            DecoderState::ReadyToDecodeNextHeader => return Err(format!("Cant decode next block body, while expecting to decode the header of the previous block. Results will be nonsense")),
        }

        match header.block_type {
            BlockType::RLE => {
                const BATCH_SIZE: usize = 512;
                let mut buf = [0u8; BATCH_SIZE];
                let full_reads = header.decompressed_size / BATCH_SIZE as u32;
                let single_read_size = header.decompressed_size % BATCH_SIZE as u32;

                match source.read_exact(&mut buf[0..1]) {
                    Ok(_) => {
                        self.internal_state = DecoderState::ReadyToDecodeNextHeader;
                    }
                    Err(_) => return Err(format!("Error while reading the one RLE byte")),
                }

                for i in 1..BATCH_SIZE {
                    buf[i] = buf[0];
                }

                for _ in 0..full_reads {
                    workspace.buffer.push(&buf[..]);
                }
                let smaller = &mut buf[..single_read_size as usize];
                workspace.buffer.push(smaller);

                Ok(1)
            }
            BlockType::Raw => {
                const BATCH_SIZE: usize = 128*1024;
                let mut buf = [0u8; BATCH_SIZE];
                let full_reads = header.decompressed_size / BATCH_SIZE as u32;
                let single_read_size = header.decompressed_size % BATCH_SIZE as u32;

                for _ in 0..full_reads {
                    match source.read_exact(&mut buf[..]) {
                        Ok(_) => {
                            workspace.buffer.push(&buf[..]);
                        }
                        Err(_) => {
                            return Err(format!("Error while reading bytes of the raw block"))
                        }
                    }
                }
                
                let smaller = &mut buf[..single_read_size as usize];
                match source.read_exact(smaller) {
                    Ok(_) => {
                        workspace.buffer.push(smaller);
                    }
                    Err(_) => {
                       return Err(format!("Error while reading bytes of the raw block"))
                    }
                }


                self.internal_state = DecoderState::ReadyToDecodeNextHeader;
                Ok(header.decompressed_size as u64)
            }

            BlockType::Reserved => {
                Err("How did you even get this. The decoder should error out if it detects a reserved-type block".to_owned())
            }

            BlockType::Compressed => {
                self.decompress_block(header, workspace, source)?;
                //unimplemented!("Decompression is not yet implemented...");

                self.internal_state = DecoderState::ReadyToDecodeNextHeader;
                Ok(header.content_size as u64)
            }
        }
    }

    fn decompress_block(
        &mut self,
        header: &BlockHeader,
        workspace: &mut DecoderScratch, //reuse this as often as possible. Not only if the trees are reused but also reuse the allocations when building new trees
        source: &mut Read,
    ) -> Result<(), String> {
        workspace
            .block_content_buffer
            .resize(header.content_size as usize, 0);

        match source.read_exact(workspace.block_content_buffer.as_mut_slice()) {
            Ok(_) => { /* happy */ }
            Err(_) => return Err("Error while reading the block content".to_owned()),
        }

        let raw = workspace.block_content_buffer.as_slice();

        let mut section = LiteralsSection::new();
        let bytes_in_literals_header = section.parse_from_header(raw)?;
        let raw = &raw[bytes_in_literals_header as usize..];
        if crate::VERBOSE {
            println!(
                "Found {} literalssection with regenerated size: {}, and compressed size: {:?}",
                section.ls_type, section.regenerated_size, section.compressed_size
            );
        }

        let upper_limit_for_literals = match section.compressed_size {
            Some(x) => x as usize,
            None => match section.ls_type {
                LiteralsSectionType::RLE => 1,
                LiteralsSectionType::Raw => section.regenerated_size as usize,
                _ => panic!("Bug in this library"),
            },
        };

        if raw.len() < upper_limit_for_literals {
            return Err(format!("Malformed section header. Says literals would be this long: {} but there are only {} bytes left", upper_limit_for_literals, raw.len()));
        }

        let raw_literals = &raw[..upper_limit_for_literals];
        if crate::VERBOSE {
            println!("Slice for literals: {}", raw_literals.len());
        }

        workspace.literals_buffer.clear(); //all literals of the previous block must have been used in the sequence execution anyways. just be defensive here
        let bytes_used_in_literals_section = decode_literals(
            &section,
            &mut workspace.huf,
            raw_literals,
            &mut workspace.literals_buffer,
        )?;
        assert!(
            section.regenerated_size == workspace.literals_buffer.len() as u32,
            "Wrong number of literals: {}, Should have been: {}",
            workspace.literals_buffer.len(),
            section.regenerated_size
        );
        assert!(bytes_used_in_literals_section == upper_limit_for_literals as u32);

        let raw = &raw[upper_limit_for_literals..];
        if crate::VERBOSE {
            println!("Slice for sequences with headers: {}", raw.len());
        }

        let mut seq_section = SequencesHeader::new();
        let bytes_in_sequence_header = seq_section.parse_from_header(raw)?;
        let raw = &raw[bytes_in_sequence_header as usize..];
        if crate::VERBOSE {
            println!(
                "Found sequencessection with sequences: {} and size: {}",
                seq_section.num_sequences,
                raw.len()
            );
        }

        assert!(
            bytes_in_literals_header as u32
                + bytes_used_in_literals_section
                + bytes_in_sequence_header as u32
                + raw.len() as u32
                == header.content_size
        );
        if crate::VERBOSE {
            println!("Slice for sequences: {}", raw.len());
        }

        if seq_section.num_sequences != 0 {
            decode_sequences(
                &seq_section,
                raw,
                &mut workspace.fse,
                &mut workspace.sequences,
            )?;
            execute_sequences(workspace)?;
        } else {
            workspace.buffer.push(&workspace.literals_buffer);
            workspace.sequences.clear();
        }

        Ok(())
    }

    pub fn read_block_header(&mut self, r: &mut Read) -> Result<(BlockHeader, u8), String> {
        //match self.internal_state {
        //    DecoderState::ReadyToDecodeNextHeader => {/* Happy :) */},
        //    DecoderState::Failed => return Err(format!("Cant decode next block if failed along the way. Results will be nonsense")),
        //    DecoderState::ReadyToDecodeNextBody => return Err(format!("Cant decode next block header, while expecting to decode the body of the previous block. Results will be nonsense")),
        //}

        match r.read_exact(&mut self.header_buffer[0..3]) {
            Ok(_) => {}
            Err(_) => return Err(format!("Error while reading the block header")),
        }

        let btype = match self.block_type() {
            Ok(t) => match t {
                BlockType::Reserved => {
                    return Err(format!(
                        "Reserved block occured. This is considered corruption by the documentation"
                    ))
                }
                _ => t,
            },
            Err(m) => return Err(m),
        };

        let block_size = self.block_content_size()?;
        let decompressed_size = match btype {
            BlockType::Raw => block_size,
            BlockType::RLE => block_size,
            BlockType::Reserved => 0, //should be catched above, this is an error state
            BlockType::Compressed => 0, //unknown but will be smaller than 128kb (or window_size if that is smaller than 128kb)
        };
        let content_size = match btype {
            BlockType::Raw => block_size,
            BlockType::Compressed => block_size,
            BlockType::RLE => 1,
            BlockType::Reserved => 0, //should be catched above, this is an error state
        };

        let last_block = self.is_last();

        self.reset_buffer();
        self.internal_state = DecoderState::ReadyToDecodeNextBody;

        //just return 3. Blockheaders always take 3 bytes
        Ok((BlockHeader {
            last_block: last_block,
            block_type: btype,
            decompressed_size: decompressed_size,
            content_size: content_size,
        }, 3))
    }

    fn reset_buffer(&mut self) {
        self.header_buffer[0] = 0;
        self.header_buffer[1] = 0;
        self.header_buffer[2] = 0;
    }

    fn is_last(&self) -> bool {
        self.header_buffer[0] & 0x1 == 1
    }

    fn block_type(&self) -> Result<BlockType, String> {
        let t = (self.header_buffer[0] >> 1) & 0x3;
        match t {
            0 => Ok(BlockType::Raw),
            1 => Ok(BlockType::RLE),
            2 => Ok(BlockType::Compressed),
            3 => Ok(BlockType::Reserved),
            _ => Err(format!(
                "Invalid Blocktype number. Is: {} Should be one of: 0,1,2,3 (3 is reserved though)",
                t
            )),
        }
    }

    fn block_content_size(&self) -> Result<u32, String> {
        let val = self.block_content_size_unchecked();
        if val > ABSOLUTE_MAXIMUM_BLOCK_SIZE {
            Err(format!(
                "Blocksize was bigger than the absolute maximum 128kb. Is: {}",
                val
            ))
        } else {
            Ok(val)
        }
    }

    fn block_content_size_unchecked(&self) -> u32 {
        ((self.header_buffer[0] >> 3) as u32) //push out type and last_block flags. Retain 5 bit
            | ((self.header_buffer[1] as u32) << 5)
            | ((self.header_buffer[2] as u32) << 13)
    }
}
