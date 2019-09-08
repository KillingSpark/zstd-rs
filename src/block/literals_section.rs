pub struct LiteralsSection {
    regenerated_size: u32,
    compressed_size: Option<u32>,
    num_streams: Option<u8>,
}

pub enum LiteralsSectionType {
    Raw,
    RLE,
    Compressed,
    Treeless,
}

impl LiteralsSection {
    pub fn parse_from_header(&mut self, raw: &[u8]) -> Result<u8, String> {
        let size_format = (raw[0] >> 2) & 0x3;
        match section_type(raw)? {
            LiteralsSectionType::RLE | LiteralsSectionType::Raw => {
                self.compressed_size = None;
                match size_format {
                    0 | 2 => {
                        //size_format actually only uses one bit
                        //regenerated_size uses 5 bits
                        self.regenerated_size = raw[0] as u32 >> 3;
                        Ok(1)
                    }
                    1 => {
                        //size_format uses 2 bit
                        //regenerated_size uses 12 bits
                        self.regenerated_size = (raw[0] as u32 >> 4) + ((raw[1] as u32) << 4);
                        Ok(2)
                    }
                    3 => {
                        //size_format uses 2 bit
                        //regenerated_size uses 20 bits
                        self.regenerated_size =
                            (raw[0] as u32 >> 4) + ((raw[1] as u32) << 4) + ((raw[2] as u32) << 12);
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
                            (raw[0] as u32 >> 4) + ((raw[1] as u32 & 0x3f) << 4);

                        // 2 from the second, full last byte
                        self.compressed_size = Some((raw[1] as u32 >> 6) + ((raw[2] as u32) << 2));
                        Ok(3)
                    }
                    2 => {
                        //both regenerated and compressed sizes use 14 bit

                        //4 from first, full second, 2 from the third byte
                        self.regenerated_size = (raw[0] as u32 >> 4)
                            + ((raw[1] as u32) << 4)
                            + ((raw[2] as u32 & 0x3) << 12);

                        //6 from the third, full last byte
                        self.compressed_size = Some((raw[2] as u32 >> 2) + ((raw[3] as u32) << 6));
                        Ok(4)
                    }
                    3 => {
                        //both regenerated and compressed sizes use 18 bit

                        //4 from first, full second, six from third byte
                        self.regenerated_size = (raw[0] as u32 >> 4)
                            + ((raw[1] as u32) << 4)
                            + ((raw[2] as u32 & 0x3F) << 12);

                        //2 from third, full fourth, full fifth byte
                        self.compressed_size = Some(
                            (raw[2] as u32 >> 6) + ((raw[3] as u32) << 2) + ((raw[4] as u32) << 10),
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
}

pub fn section_type(raw: &[u8]) -> Result<LiteralsSectionType, String> {
    let t = raw[0] & 0x3;
    match t {
        0 => Ok(LiteralsSectionType::Raw),
        1 => Ok(LiteralsSectionType::RLE),
        2 => Ok(LiteralsSectionType::Compressed),
        3 => Ok(LiteralsSectionType::Treeless),
        _ => Err(format!(
            "Illegal literalssectiontype. Is: {}, must be in: 0,1,2,3",
            t
        )),
    }
}
