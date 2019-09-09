use super::block::BlockHeader;
use super::block::BlockType;
use super::literals_section::LiteralsSection;
use super::literals_section_decoder::decode_literals;
use crate::decoding::scratch::DecoderScratch;
use std::io::Read;
use std::io::Write;

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
        target: &mut Write,
    ) -> Result<(), String> {
        match self.internal_state {
            DecoderState::ReadyToDecodeNextBody => {/* Happy :) */},
            DecoderState::Failed => return Err(format!("Cant decode next block if failed along the way. Results will be nonsense")),
            DecoderState::ReadyToDecodeNextHeader => return Err(format!("Cant decode next block body, while expecting to decode the header of the previous block. Results will be nonsense")),
        }

        match header.block_type {
            BlockType::RLE => {
                //TODO batch that into a sensible amount of bytes at once. Maybe a buffer of size 512b?
                let mut buf = [0u8; 1];
                match source.read_exact(&mut buf[0..1]) {
                    Ok(_) => {
                        for written in 0..header.decompressed_size {
                            match target.write_all(&buf[0..1]) {
                                Ok(_) => {}
                                Err(_) => {
                                    return Err(format!(
                                        "Error while writing byte: {} out of {}",
                                        written, header.decompressed_size
                                    ))
                                }
                            }
                        }

                        self.internal_state = DecoderState::ReadyToDecodeNextHeader;
                        Ok(())
                    }
                    Err(_) => Err(format!("Error while reading the one RLE byte")),
                }
            }
            BlockType::Raw => {
                //TODO batch that into a sensible amount of bytes at once. Maybe a buffer of size 512b?
                let mut buf = [0u8; 1];
                for written in 0..header.decompressed_size {
                    match source.read_exact(&mut buf[0..1]) {
                        Ok(_) => {
                            //TODO batch that into a sensible amount of bytes at once. Maybe a buffer of size 512b?
                            match target.write_all(&buf[0..1]) {
                                Ok(_) => {}
                                Err(_) => {
                                    return Err(format!(
                                        "Error while writing byte: {} out of {}",
                                        written, header.decompressed_size
                                    ))
                                }
                            }
                        }
                        Err(_) => {
                            return Err(format!(
                                "Error while reading byte: {} out of {}",
                                written, header.decompressed_size
                            ))
                        }
                    }
                }

                self.internal_state = DecoderState::ReadyToDecodeNextHeader;
                Ok(())
            }

            BlockType::Reserved => {
                Err("How did you even get this. The decoder should error out if it detects a reserved-type block".to_owned())
            }

            BlockType::Compressed => {
                //TODO actually decompress stuff
                let _ = workspace; //workspace will be used here
                self.decompress_block(header, workspace, source, target)?;
                //unimplemented!("Decompression is not yet implemented...");

                self.internal_state = DecoderState::ReadyToDecodeNextHeader;
                Ok(())
            }
        }
    }

    fn decompress_block(
        &mut self,
        header: &BlockHeader,
        workspace: &mut DecoderScratch, //reuse this as often as possible. Not only if the trees are reused but also reuse the allocations when building new trees
        source: &mut Read,
        target: &mut Write,
    ) -> Result<(), String> {
        workspace
            .block_content_buffer
            .resize(header.content_size as usize, 0);

        match source.read_exact(workspace.block_content_buffer.as_mut_slice()) {
            Ok(_) => {/* happy */},
            Err(_) => return Err("Error while reading the block content".to_owned()),
        }

        let raw = workspace.block_content_buffer.as_slice();
        
        let mut section = LiteralsSection::new();
        let bytes_in_header = section.parse_from_header(raw)?;
        let raw = &raw[bytes_in_header as usize..];

        workspace.literals_buffer.clear(); //all literals of the previous block must have been used in the sequence execution anyways. just be defensive here
        let bytes_used_in_literals_section = decode_literals(&section, &mut workspace.huf, raw, &mut workspace.literals_buffer)?;
        let raw = &raw[bytes_used_in_literals_section as usize..];

        assert!(workspace.literals_buffer.len() == section.regenerated_size as usize);

        //TODO decode sequences
        let _ = raw;
        
        //TODO execute sequences
        let _ = target;


        Ok(())
    }

    pub fn read_block_header(&mut self, r: &mut Read) -> Result<BlockHeader, String> {
        match self.internal_state {
            DecoderState::ReadyToDecodeNextHeader => {/* Happy :) */},
            DecoderState::Failed => return Err(format!("Cant decode next block if failed along the way. Results will be nonsense")),
            DecoderState::ReadyToDecodeNextBody => return Err(format!("Cant decode next block header, while expecting to decode the body of the previous block. Results will be nonsense")),
        }

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

        Ok(BlockHeader {
            last_block: last_block,
            block_type: btype,
            decompressed_size: decompressed_size,
            content_size: content_size,
        })
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
