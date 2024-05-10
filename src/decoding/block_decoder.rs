use super::super::blocks::block::BlockHeader;
use super::super::blocks::block::BlockType;
use super::super::blocks::literals_section::LiteralsSection;
use super::super::blocks::literals_section::LiteralsSectionType;
use super::super::blocks::sequence_section::SequencesHeader;
use super::literals_section_decoder::{decode_literals, DecompressLiteralsError};
use super::sequence_execution::ExecuteSequencesError;
use super::sequence_section_decoder::decode_sequences;
use super::sequence_section_decoder::DecodeSequenceError;
use crate::blocks::literals_section::LiteralsSectionParseError;
use crate::blocks::sequence_section::SequencesHeaderParseError;
use crate::decoding::scratch::DecoderScratch;
use crate::decoding::sequence_execution::execute_sequences;
use crate::io::{self, Read};

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

#[derive(Debug)]
#[non_exhaustive]
pub enum BlockHeaderReadError {
    ReadError(io::Error),
    FoundReservedBlock,
    BlockTypeError(BlockTypeError),
    BlockSizeError(BlockSizeError),
}

#[cfg(feature = "std")]
impl std::error::Error for BlockHeaderReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BlockHeaderReadError::ReadError(source) => Some(source),
            BlockHeaderReadError::BlockTypeError(source) => Some(source),
            BlockHeaderReadError::BlockSizeError(source) => Some(source),
            BlockHeaderReadError::FoundReservedBlock => None,
        }
    }
}

impl ::core::fmt::Display for BlockHeaderReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        match self {
            BlockHeaderReadError::ReadError(_) => write!(f, "Error while reading the block header"),
            BlockHeaderReadError::FoundReservedBlock => write!(
                f,
                "Reserved block occured. This is considered corruption by the documentation"
            ),
            BlockHeaderReadError::BlockTypeError(e) => write!(f, "Error getting block type: {}", e),
            BlockHeaderReadError::BlockSizeError(e) => {
                write!(f, "Error getting block content size: {}", e)
            }
        }
    }
}

impl From<io::Error> for BlockHeaderReadError {
    fn from(val: io::Error) -> Self {
        Self::ReadError(val)
    }
}

impl From<BlockTypeError> for BlockHeaderReadError {
    fn from(val: BlockTypeError) -> Self {
        Self::BlockTypeError(val)
    }
}

impl From<BlockSizeError> for BlockHeaderReadError {
    fn from(val: BlockSizeError) -> Self {
        Self::BlockSizeError(val)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum BlockTypeError {
    InvalidBlocktypeNumber { num: u8 },
}

#[cfg(feature = "std")]
impl std::error::Error for BlockTypeError {}

impl core::fmt::Display for BlockTypeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BlockTypeError::InvalidBlocktypeNumber { num } => {
                write!(f,
                    "Invalid Blocktype number. Is: {} Should be one of: 0, 1, 2, 3 (3 is reserved though",
                    num,
                )
            }
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum BlockSizeError {
    BlockSizeTooLarge { size: u32 },
}

#[cfg(feature = "std")]
impl std::error::Error for BlockSizeError {}

impl core::fmt::Display for BlockSizeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BlockSizeError::BlockSizeTooLarge { size } => {
                write!(
                    f,
                    "Blocksize was bigger than the absolute maximum {} (128kb). Is: {}",
                    ABSOLUTE_MAXIMUM_BLOCK_SIZE, size,
                )
            }
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum DecompressBlockError {
    BlockContentReadError(io::Error),
    MalformedSectionHeader {
        expected_len: usize,
        remaining_bytes: usize,
    },
    DecompressLiteralsError(DecompressLiteralsError),
    LiteralsSectionParseError(LiteralsSectionParseError),
    SequencesHeaderParseError(SequencesHeaderParseError),
    DecodeSequenceError(DecodeSequenceError),
    ExecuteSequencesError(ExecuteSequencesError),
}

#[cfg(feature = "std")]
impl std::error::Error for DecompressBlockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecompressBlockError::BlockContentReadError(source) => Some(source),
            DecompressBlockError::DecompressLiteralsError(source) => Some(source),
            DecompressBlockError::LiteralsSectionParseError(source) => Some(source),
            DecompressBlockError::SequencesHeaderParseError(source) => Some(source),
            DecompressBlockError::DecodeSequenceError(source) => Some(source),
            DecompressBlockError::ExecuteSequencesError(source) => Some(source),
            _ => None,
        }
    }
}

impl core::fmt::Display for DecompressBlockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecompressBlockError::BlockContentReadError(e) => {
                write!(f, "Error while reading the block content: {}", e)
            }
            DecompressBlockError::MalformedSectionHeader {
                expected_len,
                remaining_bytes,
            } => {
                write!(f,
                    "Malformed section header. Says literals would be this long: {} but there are only {} bytes left",
                    expected_len,
                    remaining_bytes,
                )
            }
            DecompressBlockError::DecompressLiteralsError(e) => write!(f, "{:?}", e),
            DecompressBlockError::LiteralsSectionParseError(e) => write!(f, "{:?}", e),
            DecompressBlockError::SequencesHeaderParseError(e) => write!(f, "{:?}", e),
            DecompressBlockError::DecodeSequenceError(e) => write!(f, "{:?}", e),
            DecompressBlockError::ExecuteSequencesError(e) => write!(f, "{:?}", e),
        }
    }
}

impl From<io::Error> for DecompressBlockError {
    fn from(val: io::Error) -> Self {
        Self::BlockContentReadError(val)
    }
}

impl From<DecompressLiteralsError> for DecompressBlockError {
    fn from(val: DecompressLiteralsError) -> Self {
        Self::DecompressLiteralsError(val)
    }
}

impl From<LiteralsSectionParseError> for DecompressBlockError {
    fn from(val: LiteralsSectionParseError) -> Self {
        Self::LiteralsSectionParseError(val)
    }
}

impl From<SequencesHeaderParseError> for DecompressBlockError {
    fn from(val: SequencesHeaderParseError) -> Self {
        Self::SequencesHeaderParseError(val)
    }
}

impl From<DecodeSequenceError> for DecompressBlockError {
    fn from(val: DecodeSequenceError) -> Self {
        Self::DecodeSequenceError(val)
    }
}

impl From<ExecuteSequencesError> for DecompressBlockError {
    fn from(val: ExecuteSequencesError) -> Self {
        Self::ExecuteSequencesError(val)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum DecodeBlockContentError {
    DecoderStateIsFailed,
    ExpectedHeaderOfPreviousBlock,
    ReadError { step: BlockType, source: io::Error },
    DecompressBlockError(DecompressBlockError),
}

#[cfg(feature = "std")]
impl std::error::Error for DecodeBlockContentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecodeBlockContentError::ReadError { step: _, source } => Some(source),
            DecodeBlockContentError::DecompressBlockError(source) => Some(source),
            _ => None,
        }
    }
}

impl core::fmt::Display for DecodeBlockContentError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeBlockContentError::DecoderStateIsFailed => {
                write!(
                    f,
                    "Can't decode next block if failed along the way. Results will be nonsense",
                )
            }
            DecodeBlockContentError::ExpectedHeaderOfPreviousBlock => {
                write!(f,
                            "Can't decode next block body, while expecting to decode the header of the previous block. Results will be nonsense",
                        )
            }
            DecodeBlockContentError::ReadError { step, source } => {
                write!(f, "Error while reading bytes for {}: {}", step, source,)
            }
            DecodeBlockContentError::DecompressBlockError(e) => write!(f, "{:?}", e),
        }
    }
}

impl From<DecompressBlockError> for DecodeBlockContentError {
    fn from(val: DecompressBlockError) -> Self {
        Self::DecompressBlockError(val)
    }
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
        mut source: impl Read,
    ) -> Result<u64, DecodeBlockContentError> {
        match self.internal_state {
            DecoderState::ReadyToDecodeNextBody => { /* Happy :) */ }
            DecoderState::Failed => return Err(DecodeBlockContentError::DecoderStateIsFailed),
            DecoderState::ReadyToDecodeNextHeader => {
                return Err(DecodeBlockContentError::ExpectedHeaderOfPreviousBlock)
            }
        }

        let block_type = header.block_type;
        match block_type {
            BlockType::RLE => {
                const BATCH_SIZE: usize = 512;
                let mut buf = [0u8; BATCH_SIZE];
                let full_reads = header.decompressed_size / BATCH_SIZE as u32;
                let single_read_size = header.decompressed_size % BATCH_SIZE as u32;

                source.read_exact(&mut buf[0..1]).map_err(|err| {
                    DecodeBlockContentError::ReadError {
                        step: block_type,
                        source: err,
                    }
                })?;
                self.internal_state = DecoderState::ReadyToDecodeNextHeader;

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
                const BATCH_SIZE: usize = 128 * 1024;
                let mut buf = [0u8; BATCH_SIZE];
                let full_reads = header.decompressed_size / BATCH_SIZE as u32;
                let single_read_size = header.decompressed_size % BATCH_SIZE as u32;

                for _ in 0..full_reads {
                    source.read_exact(&mut buf[..]).map_err(|err| {
                        DecodeBlockContentError::ReadError {
                            step: block_type,
                            source: err,
                        }
                    })?;
                    workspace.buffer.push(&buf[..]);
                }

                let smaller = &mut buf[..single_read_size as usize];
                source
                    .read_exact(smaller)
                    .map_err(|err| DecodeBlockContentError::ReadError {
                        step: block_type,
                        source: err,
                    })?;
                workspace.buffer.push(smaller);

                self.internal_state = DecoderState::ReadyToDecodeNextHeader;
                Ok(u64::from(header.decompressed_size))
            }

            BlockType::Reserved => {
                panic!("How did you even get this. The decoder should error out if it detects a reserved-type block");
            }

            BlockType::Compressed => {
                self.decompress_block(header, workspace, source)?;

                self.internal_state = DecoderState::ReadyToDecodeNextHeader;
                Ok(u64::from(header.content_size))
            }
        }
    }

    fn decompress_block(
        &mut self,
        header: &BlockHeader,
        workspace: &mut DecoderScratch, //reuse this as often as possible. Not only if the trees are reused but also reuse the allocations when building new trees
        mut source: impl Read,
    ) -> Result<(), DecompressBlockError> {
        workspace
            .block_content_buffer
            .resize(header.content_size as usize, 0);

        source.read_exact(workspace.block_content_buffer.as_mut_slice())?;
        let raw = workspace.block_content_buffer.as_slice();

        let mut section = LiteralsSection::new();
        let bytes_in_literals_header = section.parse_from_header(raw)?;
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
            execute_sequences(workspace)?;
        } else {
            workspace.buffer.push(&workspace.literals_buffer);
            workspace.sequences.clear();
        }

        Ok(())
    }

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
        if val > ABSOLUTE_MAXIMUM_BLOCK_SIZE {
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
