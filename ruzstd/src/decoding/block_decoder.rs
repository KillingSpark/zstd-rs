use super::super::blocks::block::BlockHeader;
use super::super::blocks::block::BlockType;
use super::super::blocks::literals_section::LiteralsSection;
use super::super::blocks::literals_section::LiteralsSectionType;
use super::super::blocks::sequence_section::SequencesHeader;
use super::literals_section_decoder::decode_literals;
use super::sequence_section_decoder::decode_sequences;
use crate::common::MAX_BLOCK_SIZE;
use crate::decoding::errors::DecodeSequenceError;
use crate::decoding::errors::{
    BlockHeaderReadError, BlockSizeError, BlockTypeError, DecodeBlockContentError,
    DecompressBlockError,
};
use crate::decoding::scratch::DecoderScratch;
use crate::decoding::sequence_execution::execute_sequences;
use crate::io::Read;

pub struct BlockDecoder {
    header_buffer: [u8; 3],
    internal_state: DecoderState,
}

enum DecoderState {
    ReadyToDecodeNextHeader,
    ReadyToDecodeNextBody,
    #[allow(dead_code)]
    Failed, //TODO put "self.internal_state = DecoderState::Failed;" everywhere an unresolvable error occurs
}

/// Create a new [BlockDecoder].
pub fn new() -> BlockDecoder {
    BlockDecoder {
        internal_state: DecoderState::ReadyToDecodeNextHeader,
        header_buffer: [0u8; 3],
    }
}

impl BlockDecoder {
    pub fn decode_block_content(
        &mut self,
        header: &BlockHeader,
        workspace: &mut DecoderScratch, //reuse this as often as possible. Not only if the trees are reused but also reuse the allocations when building new trees
        mut source: impl Read,
    ) -> Result<u64, DecodeBlockContentError> {
        match self.internal_state {
            DecoderState::ReadyToDecodeNextBody => { /* Happy :) */ }
            DecoderState::Failed => return Err(DecodeBlockContentError::DecoderStateIsFailed),
            DecoderState::ReadyToDecodeNextHeader => {
                return Err(DecodeBlockContentError::ExpectedHeaderOfPreviousBlock)
            }
        }

        // Block_Maximum_Size (RFC 8878 3.1.1.2.4): the smaller of the window and 128 KiB
        let max_block_len = (workspace.buffer.window_size).min(MAX_BLOCK_SIZE as usize);
        // raw and RLE blocks state their size, compressed ones are checked as they decode
        Self::check_decompressed_size(header.decompressed_size as usize, max_block_len)?;

        let block_type = header.block_type;
        match block_type {
            BlockType::RLE => {
                let mut buf = [0u8; 1];
                source.read_exact(&mut buf[..]).map_err(|err| {
                    DecodeBlockContentError::ReadError {
                        step: block_type,
                        source: err,
                    }
                })?;
                workspace
                    .buffer
                    .extend_and_fill(buf[0], header.decompressed_size as usize);

                self.internal_state = DecoderState::ReadyToDecodeNextHeader;

                Ok(1)
            }
            BlockType::Raw => {
                workspace
                    .buffer
                    .extend_from_reader(&mut source, header.decompressed_size as usize)
                    .map_err(|err| DecodeBlockContentError::ReadError {
                        step: block_type,
                        source: err,
                    })?;

                self.internal_state = DecoderState::ReadyToDecodeNextHeader;
                Ok(u64::from(header.decompressed_size))
            }

            BlockType::Reserved => {
                panic!("How did you even get this. The decoder should error out if it detects a reserved-type block");
            }

            BlockType::Compressed => {
                self.decompress_block(header, workspace, source, max_block_len)?;

                self.internal_state = DecoderState::ReadyToDecodeNextHeader;
                Ok(u64::from(header.content_size))
            }
        }
    }

    fn check_decompressed_size(at_least: usize, max: usize) -> Result<(), DecompressBlockError> {
        if at_least > max {
            return Err(DecompressBlockError::DecompressedSizeTooLarge { at_least, max });
        }
        Ok(())
    }

    fn decompress_block(
        &mut self,
        header: &BlockHeader,
        workspace: &mut DecoderScratch, //reuse this as often as possible. Not only if the trees are reused but also reuse the allocations when building new trees
        mut source: impl Read,
        max_block_decoded_len: usize,
    ) -> Result<(), DecompressBlockError> {
        workspace
            .block_content_buffer
            .resize(header.content_size as usize, 0);

        source.read_exact(workspace.block_content_buffer.as_mut_slice())?;
        let raw = workspace.block_content_buffer.as_slice();

        let mut section = LiteralsSection::new();
        let bytes_in_literals_header = section.parse_from_header(raw)?;
        // Literal decoding may reserve or resize using this untrusted length.
        // Check before decoding, including the zero-sequence case.
        Self::check_decompressed_size(section.regenerated_size as usize, max_block_decoded_len)?;
        let raw = &raw[bytes_in_literals_header as usize..];
        vprintln!(
            "Found {} literalssection with regenerated size: {}, and compressed size: {:?}",
            section.ls_type,
            section.regenerated_size,
            section.compressed_size
        );

        let upper_limit_for_literals = match section.compressed_size {
            Some(x) => x as usize,
            None => match section.ls_type {
                LiteralsSectionType::RLE => 1,
                LiteralsSectionType::Raw => section.regenerated_size as usize,
                _ => panic!("Bug in this library"),
            },
        };

        if raw.len() < upper_limit_for_literals {
            return Err(DecompressBlockError::MalformedSectionHeader {
                expected_len: upper_limit_for_literals,
                remaining_bytes: raw.len(),
            });
        }

        let raw_literals = &raw[..upper_limit_for_literals];
        vprintln!("Slice for literals: {}", raw_literals.len());

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
        vprintln!("Slice for sequences with headers: {}", raw.len());

        let mut seq_section = SequencesHeader::new();
        let bytes_in_sequence_header = seq_section.parse_from_header(raw)?;
        // All literals are eventually emitted and each sequence match emits
        // at least 3 bytes. Reject an impossible count before sequence table
        // decoding or either entropy path clears/reserves the sequence vector.
        // The execution loop checks actual lengths before each output append.
        let minimum_decoded_len = (seq_section.num_sequences as usize)
            .checked_mul(3)
            .and_then(|matches| matches.checked_add(workspace.literals_buffer.len()))
            .unwrap_or(usize::MAX);
        Self::check_decompressed_size(minimum_decoded_len, max_block_decoded_len)?;
        let raw = &raw[bytes_in_sequence_header as usize..];
        vprintln!(
            "Found sequencessection with sequences: {} and size: {}",
            seq_section.num_sequences,
            raw.len()
        );

        assert!(
            u32::from(bytes_in_literals_header)
                + bytes_used_in_literals_section
                + u32::from(bytes_in_sequence_header)
                + raw.len() as u32
                == header.content_size
        );
        vprintln!("Slice for sequences: {}", raw.len());

        if seq_section.num_sequences != 0 {
            decode_sequences(
                &seq_section,
                raw,
                &mut workspace.fse,
                &mut workspace.sequences,
            )?;
            vprintln!("Executing sequences");
            execute_sequences(workspace, max_block_decoded_len)?;
        } else {
            if !raw.is_empty() {
                return Err(DecompressBlockError::DecodeSequenceError(
                    DecodeSequenceError::ExtraBits {
                        bits_remaining: raw.len() as isize * 8,
                    },
                ));
            }
            workspace.buffer.push(&workspace.literals_buffer);
            workspace.sequences.clear();
        }

        Ok(())
    }

    /// Reads 3 bytes from the provided reader and returns
    /// the deserialized header and the number of bytes read.
    pub fn read_block_header(
        &mut self,
        mut r: impl Read,
    ) -> Result<(BlockHeader, u8), BlockHeaderReadError> {
        //match self.internal_state {
        //    DecoderState::ReadyToDecodeNextHeader => {/* Happy :) */},
        //    DecoderState::Failed => return Err(format!("Cant decode next block if failed along the way. Results will be nonsense")),
        //    DecoderState::ReadyToDecodeNextBody => return Err(format!("Cant decode next block header, while expecting to decode the body of the previous block. Results will be nonsense")),
        //}

        r.read_exact(&mut self.header_buffer[0..3])?;

        let btype = self.block_type()?;
        if let BlockType::Reserved = btype {
            return Err(BlockHeaderReadError::FoundReservedBlock);
        }

        let block_size = self.block_content_size()?;
        let decompressed_size = match btype {
            BlockType::Raw => block_size,
            BlockType::RLE => block_size,
            BlockType::Reserved => 0, //should be caught above, this is an error state
            BlockType::Compressed => 0, //unknown but will be smaller than 128kb (or window_size if that is smaller than 128kb)
        };
        let content_size = match btype {
            BlockType::Raw => block_size,
            BlockType::Compressed => block_size,
            BlockType::RLE => 1,
            BlockType::Reserved => 0, //should be caught above, this is an error state
        };

        let last_block = self.is_last();

        self.reset_buffer();
        self.internal_state = DecoderState::ReadyToDecodeNextBody;

        //just return 3. Blockheaders always take 3 bytes
        Ok((
            BlockHeader {
                last_block,
                block_type: btype,
                decompressed_size,
                content_size,
            },
            3,
        ))
    }

    fn reset_buffer(&mut self) {
        self.header_buffer[0] = 0;
        self.header_buffer[1] = 0;
        self.header_buffer[2] = 0;
    }

    fn is_last(&self) -> bool {
        self.header_buffer[0] & 0x1 == 1
    }

    fn block_type(&self) -> Result<BlockType, BlockTypeError> {
        let t = (self.header_buffer[0] >> 1) & 0x3;
        match t {
            0 => Ok(BlockType::Raw),
            1 => Ok(BlockType::RLE),
            2 => Ok(BlockType::Compressed),
            3 => Ok(BlockType::Reserved),
            other => Err(BlockTypeError::InvalidBlocktypeNumber { num: other }),
        }
    }

    fn block_content_size(&self) -> Result<u32, BlockSizeError> {
        let val = self.block_content_size_unchecked();
        if val > MAX_BLOCK_SIZE {
            Err(BlockSizeError::BlockSizeTooLarge { size: val })
        } else {
            Ok(val)
        }
    }

    fn block_content_size_unchecked(&self) -> u32 {
        u32::from(self.header_buffer[0] >> 3) //push out type and last_block flags. Retain 5 bit
            | (u32::from(self.header_buffer[1]) << 5)
            | (u32::from(self.header_buffer[2]) << 13)
    }
}

#[cfg(test)]
mod repair08_block_tests {
    use super::new;
    use crate::decoding::{
        errors::{DecodeBlockContentError, DecompressBlockError},
        scratch::DecoderScratch,
    };

    fn compressed_rle_literal_body(size: u32) -> [u8; 5] {
        let header = (size << 4) | 13; // RLE, 20-bit regenerated length
        [
            header as u8,
            (header >> 8) as u8,
            (header >> 16) as u8,
            b'x',
            0,
        ]
    }

    #[test]
    fn literal_limit_precedes_literal_allocation_and_zero_sequence_output() {
        for size in [1025, 131073, 1048575] {
            let mut decoder = new();
            let mut scratch = DecoderScratch::new(1024);
            let (header, _) = decoder.read_block_header(&[45u8, 0, 0][..]).unwrap();
            let body = compressed_rle_literal_body(size);
            let capacity = scratch.literals_buffer.capacity();
            let error = decoder
                .decode_block_content(&header, &mut scratch, &body[..])
                .unwrap_err();
            assert!(
                matches!(error, DecodeBlockContentError::DecompressBlockError(
                DecompressBlockError::DecompressedSizeTooLarge { max: 1024, at_least }
            ) if at_least == size as usize)
            );
            assert_eq!(scratch.literals_buffer.capacity(), capacity);
            assert!(scratch.literals_buffer.is_empty());
            assert_eq!(scratch.buffer.len(), 0);
        }
    }

    #[test]
    fn literal_only_exact_window_limit_is_legal() {
        let mut decoder = new();
        let mut scratch = DecoderScratch::new(1024);
        let (header, _) = decoder.read_block_header(&[45u8, 0, 0][..]).unwrap();
        let body = compressed_rle_literal_body(1024);
        decoder
            .decode_block_content(&header, &mut scratch, &body[..])
            .unwrap();
        assert_eq!(scratch.buffer.len(), 1024);
        assert_eq!(scratch.literals_buffer.len(), 1024);
    }
    #[test]
    fn raw_and_rle_blocks_refuse_before_output_and_accept_tiny_empty() {
        for kind in [0u32, 1] {
            let mut decoder = new();
            let mut scratch = DecoderScratch::new(1024);
            let encoded_header = (1025u32 << 3) | (kind << 1) | 1;
            let bytes = [
                encoded_header as u8,
                (encoded_header >> 8) as u8,
                (encoded_header >> 16) as u8,
            ];
            let (header, _) = decoder.read_block_header(&bytes[..]).unwrap();
            // Rejection must precede even reading the body.
            assert!(matches!(
                decoder.decode_block_content(&header, &mut scratch, &[][..]),
                Err(DecodeBlockContentError::DecompressBlockError(
                    DecompressBlockError::DecompressedSizeTooLarge {
                        max: 1024,
                        at_least: 1025
                    }
                ))
            ));
            assert_eq!(scratch.buffer.len(), 0);
        }
        for size in [0u32, 1] {
            let mut decoder = new();
            let mut scratch = DecoderScratch::new(1024);
            let bytes = [(size << 3) as u8 | 1, 0, 0]; // raw block
            let (header, _) = decoder.read_block_header(&bytes[..]).unwrap();
            decoder
                .decode_block_content(&header, &mut scratch, &b"x"[..size as usize])
                .unwrap();
            assert_eq!(scratch.buffer.len(), size as usize);
        }
    }
}

#[cfg(test)]
mod count12_block_tests {
    use super::new;
    use crate::blocks::sequence_section::Sequence;
    use crate::decoding::{
        errors::{DecodeBlockContentError, DecompressBlockError, ExecuteSequencesError},
        scratch::DecoderScratch,
    };
    use alloc::{vec, vec::Vec};

    fn sequence_header(count: u32, mode: u8) -> Vec<u8> {
        match count {
            0 => vec![0],
            1..=127 => vec![count as u8, mode],
            128..=32511 => vec![(128 + (count >> 8)) as u8, count as u8, mode],
            32512..=98047 => {
                let n = count - 32512;
                vec![255, n as u8, (n >> 8) as u8, mode]
            }
            _ => panic!("out of format range"),
        }
    }

    fn body(literals: u32, count: u32, mode: u8, payload: &[u8]) -> Vec<u8> {
        // RLE literal section, 20-bit count, followed by the sequence section.
        let header = (literals << 4) | 13;
        let mut bytes = vec![
            header as u8,
            (header >> 8) as u8,
            (header >> 16) as u8,
            b'x',
        ];
        bytes.extend_from_slice(&sequence_header(count, mode));
        bytes.extend_from_slice(payload);
        bytes
    }

    fn decode(bytes: &[u8], scratch: &mut DecoderScratch) -> Result<u64, DecodeBlockContentError> {
        let encoded = ((bytes.len() as u32) << 3) | 5; // compressed, last block
        let header_bytes = [encoded as u8, (encoded >> 8) as u8, (encoded >> 16) as u8];
        let mut decoder = new();
        let (header, _) = decoder.read_block_header(&header_bytes[..]).unwrap();
        decoder.decode_block_content(&header, scratch, bytes)
    }

    #[test]
    fn impossible_count_precedes_tables_clear_reserve_and_bitstream_errors() {
        // Count-header encoding transitions, exact block budget + one, maximum
        // header count, zero remaining match budget, and a sub-match window.
        for (window, literals, count) in [
            (3usize, 0u32, 2u32),
            (1024, 1022, 1),
            (1024, 1024, 1),
            (1024, 641, 128),
            (1024, 0, 342),
            (131072, 0, 43691),
            (262144, 0, 98047),
            (2, 0, 1),
        ] {
            for mode in [0u8, 0x54] {
                // all predefined and all RLE
                let mut scratch = DecoderScratch::new(window);
                scratch.buffer.push(b"abcd");
                scratch.sequences.push(Sequence {
                    ll: 7,
                    ml: 11,
                    of: 4,
                });
                scratch.fse.ll_rle = Some(9);
                scratch.fse.ml_rle = Some(10);
                scratch.fse.of_rle = Some(2);
                let capacity = scratch.sequences.capacity();
                let history = scratch.offset_hist;
                // Missing tables/bitstream must not mask the earlier count error.
                let bytes = body(literals, count, mode, &[]);
                let budget = window.min(131072);
                let minimum = literals as usize + count as usize * 3;
                assert!(matches!(decode(&bytes, &mut scratch),
                    Err(DecodeBlockContentError::DecompressBlockError(
                        DecompressBlockError::DecompressedSizeTooLarge { max, at_least }))
                    if max == budget && at_least == minimum));
                assert_eq!(scratch.sequences.capacity(), capacity);
                assert_eq!(scratch.sequences.len(), 1);
                assert_eq!(
                    (
                        scratch.sequences[0].ll,
                        scratch.sequences[0].ml,
                        scratch.sequences[0].of
                    ),
                    (7, 11, 4)
                );
                assert_eq!(
                    (scratch.fse.ll_rle, scratch.fse.ml_rle, scratch.fse.of_rle),
                    (Some(9), Some(10), Some(2))
                );
                assert_eq!(scratch.buffer.len(), 4);
                assert_eq!(scratch.offset_hist, history);
            }
        }
    }

    #[test]
    fn exact_minimum_keeps_overlap_and_previous_block_references() {
        for (window, literals, ll_code) in [(3usize, 0u32, 0u8), (4, 1, 1)] {
            let mut scratch = DecoderScratch::new(window);
            scratch.buffer.push(b"p");
            scratch.offset_hist = [1, 1, 1]; // legal one-byte match history
                                             // RLE LL=0/1, OF=0 (repeat history), ML=0 (match length 3),
                                             // no extra bits and a final one. LL=0 references the prior block;
                                             // LL=1 copies x then repeats offset1 with overlap.
            let bytes = body(literals, 1, 0x54, &[ll_code, 0, 0, 1]);
            decode(&bytes, &mut scratch).unwrap();
            assert_eq!(scratch.sequences.len(), 1);
            assert_eq!(
                (scratch.sequences[0].ll, scratch.sequences[0].ml),
                (literals, 3)
            );
            assert_eq!(scratch.buffer.len(), 1 + window);
        }
        for literals in [0u32, 1] {
            let mut scratch = DecoderScratch::new(1);
            let bytes = body(literals, 0, 0, &[]);
            decode(&bytes, &mut scratch).unwrap();
            assert_eq!(scratch.buffer.len(), literals as usize);
        }
    }

    #[test]
    fn admitted_count_still_checks_actual_match_before_append() {
        let mut scratch = DecoderScratch::new(4);
        scratch.buffer.push(b"abcd");
        let history = scratch.offset_hist;
        // L+3N=4 passes the necessary bound; actual match length4 makes
        // output5 and must fail before this sequence changes the output.
        let bytes = body(1, 1, 0x54, &[1, 0, 1, 1]);
        assert!(matches!(
            decode(&bytes, &mut scratch),
            Err(DecodeBlockContentError::DecompressBlockError(
                DecompressBlockError::ExecuteSequencesError(
                    ExecuteSequencesError::TooManyBytesGenerated { max: 4, decoded: 5 }
                )
            ))
        ));
        assert_eq!(scratch.buffer.len(), 4);
        assert_eq!(scratch.offset_hist, history);
    }
}
