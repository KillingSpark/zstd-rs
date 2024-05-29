//! Utilities and representations for the second half of a block, the sequence section.
//! This section copies literals from the literals section into the decompressed output.

pub struct SequencesHeader {
    pub num_sequences: u32,
    pub modes: Option<CompressionModes>,
}

/// A sequence represents potentially redundant data, and it can be broken up into 2 steps:
/// - A copy step, where data is copied from the literals section to the decompressed output
/// - A *match* copy step that copies data from within the previously decompressed output.
///
/// <https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#sequence-execution>
#[derive(Clone, Copy)]
pub struct Sequence {
    /// Literal length, or the number of bytes to be copied from the literals section
    /// in the copy step.
    pub ll: u32,
    /// The length of the match to make during the match copy step.
    pub ml: u32,
    /// How far back to go in the decompressed data to read from the match copy step.
    /// If this value is greater than 3, then the offset is `of -3`. If `of` is from 1-3,
    /// then it has special handling:
    ///
    /// The first 3 values define 3 different repeated offsets, with 1 referring to the most
    /// recent, 2 the second recent, and so on. When the current sequence has a literal length of 0,
    /// then the repeated offsets are shifted by 1. So an offset value of 1 refers to 2, 2 refers to 3,
    /// and 3 refers to the most recent offset minus one. If that value is equal to zero, the data
    /// is considered corrupted.
    pub of: u32,
}

impl core::fmt::Display for Sequence {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        write!(f, "LL: {}, ML: {}, OF: {}", self.ll, self.ml, self.of)
    }
}

/// This byte defines the compression mode of each symbol type
#[derive(Copy, Clone)]
pub struct CompressionModes(u8);
/// The compression mode used for symbol compression
pub enum ModeType {
    /// A predefined FSE distribution table is used, and no distribution table
    /// will be present.
    Predefined,
    /// The table consists of a single byte, which contains the symbol's value.
    RLE,
    /// Standard FSE compression, a distribution table will be present. This
    /// mode should not be used when only one symbol is present.
    FSECompressed,
    /// The table used in the previous compressed block with at least one sequence
    /// will be used again. If this is the first block, the table in the dictionary will
    /// be used.
    Repeat,
}

impl CompressionModes {
    /// Deserialize a two bit mode value into a [ModeType]
    pub fn decode_mode(m: u8) -> ModeType {
        match m {
            0 => ModeType::Predefined,
            1 => ModeType::RLE,
            2 => ModeType::FSECompressed,
            3 => ModeType::Repeat,
            _ => panic!("This can never happen"),
        }
    }
    /// Read the compression mode of the literal lengths field.
    pub fn ll_mode(self) -> ModeType {
        Self::decode_mode(self.0 >> 6)
    }

    /// Read the compression mode of the offset value field.
    pub fn of_mode(self) -> ModeType {
        Self::decode_mode((self.0 >> 4) & 0x3)
    }

    /// Read the compression mode of the match lengths field.
    pub fn ml_mode(self) -> ModeType {
        Self::decode_mode((self.0 >> 2) & 0x3)
    }
}

impl Default for SequencesHeader {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum SequencesHeaderParseError {
    NotEnoughBytes { need_at_least: u8, got: usize },
}

#[cfg(feature = "std")]
impl std::error::Error for SequencesHeaderParseError {}

impl core::fmt::Display for SequencesHeaderParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SequencesHeaderParseError::NotEnoughBytes { need_at_least, got } => {
                write!(
                    f,
                    "source must have at least {} bytes to parse header; got {} bytes",
                    need_at_least, got,
                )
            }
        }
    }
}

impl SequencesHeader {
    /// Create a new [SequencesHeader].
    pub fn new() -> SequencesHeader {
        SequencesHeader {
            num_sequences: 0,
            modes: None,
        }
    }

    /// Attempt to deserialize the provided buffer into `self`, returning the number of bytes read.
    pub fn parse_from_header(&mut self, source: &[u8]) -> Result<u8, SequencesHeaderParseError> {
        let mut bytes_read = 0;
        if source.is_empty() {
            return Err(SequencesHeaderParseError::NotEnoughBytes {
                need_at_least: 1,
                got: 0,
            });
        }

        let source = match source[0] {
            0 => {
                self.num_sequences = 0;
                return Ok(1);
            }
            1..=127 => {
                if source.len() < 2 {
                    return Err(SequencesHeaderParseError::NotEnoughBytes {
                        need_at_least: 2,
                        got: source.len(),
                    });
                }
                self.num_sequences = u32::from(source[0]);
                bytes_read += 1;
                &source[1..]
            }
            128..=254 => {
                if source.len() < 3 {
                    return Err(SequencesHeaderParseError::NotEnoughBytes {
                        need_at_least: 3,
                        got: source.len(),
                    });
                }
                self.num_sequences = ((u32::from(source[0]) - 128) << 8) + u32::from(source[1]);
                bytes_read += 2;
                &source[2..]
            }
            255 => {
                if source.len() < 4 {
                    return Err(SequencesHeaderParseError::NotEnoughBytes {
                        need_at_least: 4,
                        got: source.len(),
                    });
                }
                self.num_sequences = u32::from(source[1]) + (u32::from(source[2]) << 8) + 0x7F00;
                bytes_read += 3;
                &source[3..]
            }
        };

        self.modes = Some(CompressionModes(source[0]));
        bytes_read += 1;

        Ok(bytes_read)
    }
}
