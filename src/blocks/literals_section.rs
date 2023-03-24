use super::super::decoding::bit_reader::{BitReader, GetBitsError};

pub struct LiteralsSection {
    pub regenerated_size: u32,
    pub compressed_size: Option<u32>,
    pub num_streams: Option<u8>,
    pub ls_type: LiteralsSectionType,
}

pub enum LiteralsSectionType {
    Raw,
    RLE,
    Compressed,
    Treeless,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LiteralsSectionParseError {
    #[error("Illegal literalssectiontype. Is: {got}, must be in: 0, 1, 2, 3")]
    IllegalLiteralSectionType { got: u8 },
    #[error(transparent)]
    GetBitsError(#[from] GetBitsError),
    #[error("Not enough byte to parse the literals section header. Have: {have}, Need: {need}")]
    NotEnoughBytes { have: usize, need: u8 },
}

impl core::fmt::Display for LiteralsSectionType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        match self {
            LiteralsSectionType::Compressed => write!(f, "Compressed"),
            LiteralsSectionType::Raw => write!(f, "Raw"),
            LiteralsSectionType::RLE => write!(f, "RLE"),
            LiteralsSectionType::Treeless => write!(f, "Treeless"),
        }
    }
}

impl Default for LiteralsSection {
    fn default() -> Self {
        Self::new()
    }
}

impl LiteralsSection {
    pub fn new() -> LiteralsSection {
        LiteralsSection {
            regenerated_size: 0,
            compressed_size: None,
            num_streams: None,
            ls_type: LiteralsSectionType::Raw,
        }
    }

    pub fn header_bytes_needed(&self, first_byte: u8) -> Result<u8, LiteralsSectionParseError> {
        let ls_type = Self::section_type(first_byte)?;
        let size_format = (first_byte >> 2) & 0x3;
        match ls_type {
            LiteralsSectionType::RLE | LiteralsSectionType::Raw => {
                match size_format {
                    0 | 2 => {
                        //size_format actually only uses one bit
                        //regenerated_size uses 5 bits
                        Ok(1)
                    }
                    1 => {
                        //size_format uses 2 bit
                        //regenerated_size uses 12 bits
                        Ok(2)
                    }
                    3 => {
                        //size_format uses 2 bit
                        //regenerated_size uses 20 bits
                        Ok(3)
                    }
                    _ => panic!(
                        "This is a bug in the program. There should only be values between 0..3"
                    ),
                }
            }
            LiteralsSectionType::Compressed | LiteralsSectionType::Treeless => {
                match size_format {
                    0 | 1 => {
                        //Only differ in num_streams
                        //both regenerated and compressed sizes use 10 bit
                        Ok(3)
                    }
                    2 => {
                        //both regenerated and compressed sizes use 14 bit
                        Ok(4)
                    }
                    3 => {
                        //both regenerated and compressed sizes use 18 bit
                        Ok(5)
                    }

                    _ => panic!(
                        "This is a bug in the program. There should only be values between 0..3"
                    ),
                }
            }
        }
    }

    pub fn parse_from_header(&mut self, raw: &[u8]) -> Result<u8, LiteralsSectionParseError> {
        let mut br = BitReader::new(raw);
        let t = br.get_bits(2)? as u8;
        self.ls_type = Self::section_type(t)?;
        let size_format = br.get_bits(2)? as u8;

        let byte_needed = self.header_bytes_needed(raw[0])?;
        if raw.len() < byte_needed as usize {
            return Err(LiteralsSectionParseError::NotEnoughBytes {
                have: raw.len(),
                need: byte_needed,
            });
        }

        match self.ls_type {
            LiteralsSectionType::RLE | LiteralsSectionType::Raw => {
                self.compressed_size = None;
                match size_format {
                    0 | 2 => {
                        //size_format actually only uses one bit
                        //regenerated_size uses 5 bits
                        self.regenerated_size = u32::from(raw[0]) >> 3;
                        Ok(1)
                    }
                    1 => {
                        //size_format uses 2 bit
                        //regenerated_size uses 12 bits
                        self.regenerated_size = (u32::from(raw[0]) >> 4) + (u32::from(raw[1]) << 4);
                        Ok(2)
                    }
                    3 => {
                        //size_format uses 2 bit
                        //regenerated_size uses 20 bits
                        self.regenerated_size = (u32::from(raw[0]) >> 4)
                            + (u32::from(raw[1]) << 4)
                            + (u32::from(raw[2]) << 12);
                        Ok(3)
                    }
                    _ => panic!(
                        "This is a bug in the program. There should only be values between 0..3"
                    ),
                }
            }
            LiteralsSectionType::Compressed | LiteralsSectionType::Treeless => {
                match size_format {
                    0 => {
                        self.num_streams = Some(1);
                    }
                    1 | 2 | 3 => {
                        self.num_streams = Some(4);
                    }
                    _ => panic!(
                        "This is a bug in the program. There should only be values between 0..3"
                    ),
                };

                match size_format {
                    0 | 1 => {
                        //Differ in num_streams see above
                        //both regenerated and compressed sizes use 10 bit

                        //4 from the first, six from the second byte
                        self.regenerated_size =
                            (u32::from(raw[0]) >> 4) + ((u32::from(raw[1]) & 0x3f) << 4);

                        // 2 from the second, full last byte
                        self.compressed_size =
                            Some(u32::from(raw[1] >> 6) + (u32::from(raw[2]) << 2));
                        Ok(3)
                    }
                    2 => {
                        //both regenerated and compressed sizes use 14 bit

                        //4 from first, full second, 2 from the third byte
                        self.regenerated_size = (u32::from(raw[0]) >> 4)
                            + (u32::from(raw[1]) << 4)
                            + ((u32::from(raw[2]) & 0x3) << 12);

                        //6 from the third, full last byte
                        self.compressed_size =
                            Some((u32::from(raw[2]) >> 2) + (u32::from(raw[3]) << 6));
                        Ok(4)
                    }
                    3 => {
                        //both regenerated and compressed sizes use 18 bit

                        //4 from first, full second, six from third byte
                        self.regenerated_size = (u32::from(raw[0]) >> 4)
                            + (u32::from(raw[1]) << 4)
                            + ((u32::from(raw[2]) & 0x3F) << 12);

                        //2 from third, full fourth, full fifth byte
                        self.compressed_size = Some(
                            (u32::from(raw[2]) >> 6)
                                + (u32::from(raw[3]) << 2)
                                + (u32::from(raw[4]) << 10),
                        );
                        Ok(5)
                    }

                    _ => panic!(
                        "This is a bug in the program. There should only be values between 0..3"
                    ),
                }
            }
        }
    }

    fn section_type(raw: u8) -> Result<LiteralsSectionType, LiteralsSectionParseError> {
        let t = raw & 0x3;
        match t {
            0 => Ok(LiteralsSectionType::Raw),
            1 => Ok(LiteralsSectionType::RLE),
            2 => Ok(LiteralsSectionType::Compressed),
            3 => Ok(LiteralsSectionType::Treeless),
            other => Err(LiteralsSectionParseError::IllegalLiteralSectionType { got: other }),
        }
    }
}
