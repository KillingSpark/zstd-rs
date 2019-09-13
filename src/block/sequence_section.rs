pub struct SequencesHeader {
    pub num_sequences: u32,
    pub modes: Option<CompressionModes>,
}

#[derive(Clone, Copy)]
pub struct Sequence {
    pub ll: u32,
    pub ml: u32,
    pub of: u32,
}

impl std::fmt::Display for Sequence {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "LL: {}, ML: {}, OF: {}", self.ll, self.ml, self.of)
    }
}

#[derive(Copy, Clone)]
pub struct CompressionModes(u8);
pub enum ModeType {
    Predefined,
    RLE,
    FSECompressed,
    Repeat,
}

impl CompressionModes {
    pub fn decode_mode(m: u8) -> ModeType {
        match m {
            0 => ModeType::Predefined,
            1 => ModeType::RLE,
            2 => ModeType::FSECompressed,
            3 => ModeType::Repeat,
            _ => panic!("This can never happen"),
        }
    }

    pub fn ll_mode(&self) -> ModeType {
        Self::decode_mode(self.0 >> 6)
    }

    pub fn of_mode(&self) -> ModeType {
        Self::decode_mode((self.0 >> 4) & 0x3)
    }

    pub fn ml_mode(&self) -> ModeType {
        Self::decode_mode((self.0 >> 2) & 0x3)
    }
}

impl SequencesHeader {
    pub fn new() -> SequencesHeader {
        SequencesHeader {
            num_sequences: 0,
            modes: None,
        }
    }

    pub fn parse_from_header(&mut self, source: &[u8]) -> Result<u8, String> {
        let mut bytes_read = 0;
        let source = match source[0] {
            0 => {
                self.num_sequences = 0;
                return Ok(1);
            }
            1...127 => {
                self.num_sequences = source[0] as u32;
                bytes_read += 1;
                &source[1..]
            }
            128...254 => {
                self.num_sequences = ((source[0] as u32 - 128) << 8) + source[1] as u32;
                bytes_read += 2;
                &source[2..]
            }
            255 => {
                self.num_sequences = source[1] as u32 + ((source[2] as u32) << 8) + 0x7F00;
                bytes_read += 3;
                &source[3..]
            }
        };

        self.modes = Some(CompressionModes(source[0]));
        bytes_read += 1;

        Ok(bytes_read)
    }
}
