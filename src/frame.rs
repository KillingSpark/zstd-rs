use std::convert::TryInto;

pub const MAGIC_NUM: u32 = 0xFD2F_B528;
pub const MIN_WINDOW_SIZE: u64 = 1024;
pub const MAX_WINDOW_SIZE: u64 = (1 << 41) + 7 * (1 << 38);

pub struct Frame {
    magic_num: u32,
    pub header: FrameHeader,
}

pub struct FrameHeader {
    pub descriptor: FrameDescriptor,
    window_descriptor: u8,
    dict_id: Vec<u8>,
    frame_content_size: Vec<u8>,
}

pub struct FrameDescriptor(u8);

impl FrameDescriptor {
    pub fn frame_content_size_flag(&self) -> u8 {
        self.0 >> 6
    }

    pub fn reserved_flag(&self) -> bool {
        ((self.0 >> 3) & 0x1) == 1
    }

    pub fn single_segment_flag(&self) -> bool {
        ((self.0 >> 5) & 0x1) == 1
    }

    pub fn content_checksum_flag(&self) -> bool {
        ((self.0 >> 2) & 0x1) == 1
    }

    pub fn dict_id_flag(&self) -> u8 {
        self.0 & 0x3
    }

    // Deriving info from the flags
    pub fn frame_content_size_bytes(&self) -> Result<u8, String> {
        match self.frame_content_size_flag() {
            0 => {
                if self.single_segment_flag() {
                    Ok(1)
                } else {
                    Ok(0)
                }
            }
            1 => Ok(2),
            2 => Ok(4),
            3 => Ok(8),
            _ => Err(format!(
                "Invalid Frame_Content_Size_Flag Is: {} Should be one of: 0,1,2,3",
                self.frame_content_size_flag()
            )),
        }
    }

    pub fn dictionary_id_bytes(&self) -> Result<u8, String> {
        match self.dict_id_flag() {
            0 => Ok(0),
            1 => Ok(1),
            2 => Ok(2),
            3 => Ok(4),
            _ => Err(format!(
                "Invalid Frame_Content_Size_Flag Is: {} Should be one of: 0,1,2,3",
                self.frame_content_size_flag()
            )),
        }
    }
}
impl FrameHeader {
    pub fn window_size(&self) -> Result<u64, String> {
        if self.descriptor.single_segment_flag() {
            self.frame_content_size()
        } else {
            let exp = self.window_descriptor >> 3;
            let mantissa = self.window_descriptor & 0x7;

            let window_log = 10 + (exp as u64);
            let window_base = 1 << window_log;
            let window_add = (window_base / 8) * (mantissa as u64);

            let window_size = window_base + window_add;

            if window_size >= MIN_WINDOW_SIZE {
                if window_size < MAX_WINDOW_SIZE {
                    Ok(window_size)
                } else {
                    Err(format!(
                        "window_size bigger than allowed maximum. Is: {}, Should be lower than: {}",
                        window_size, MAX_WINDOW_SIZE
                    ))
                }
            } else {
                Err(format!(
                    "window_size smaller than allowed minimum. Is: {}, Should be greater than: {}",
                    window_size, MIN_WINDOW_SIZE
                ))
            }
        }
    }

    pub fn dictiornary_id(&self) -> Result<Option<u32>, String> {
        if self.descriptor.dict_id_flag() == 0 {
            Ok(None)
        } else {
            match self.descriptor.dictionary_id_bytes() {
                Err(m) => Err(m),
                Ok(bytes) => {
                    if self.dict_id.len() != bytes as usize {
                        Err(format!(
                            "Not enough bytes in dict_id. Is: {}, Should be: {}",
                            self.dict_id.len(),
                            bytes
                        ))
                    } else {
                        let mut value: u32 = 0;
                        let mut shift = 0;
                        for x in &self.dict_id {
                            value |= (*x as u32) << shift;
                            shift += 8;
                        }

                        Ok(Some(value))
                    }
                }
            }
        }
    }

    pub fn frame_content_size(&self) -> Result<u64, String> {
        match self.descriptor.frame_content_size_bytes() {
            Err(m) => Err(m),
            Ok(bytes) => match bytes {
                0 => Err("Bytes was zero".to_owned()),
                1 => {
                    if self.frame_content_size.len() == 1 {
                        Ok(u64::from(self.frame_content_size[0]))
                    } else {
                        Err(format!(
                            "frame_content_size not long enough. Is: {}, Should be: {}",
                            self.frame_content_size.len(),
                            bytes
                        ))
                    }
                }
                2 => {
                    if self.frame_content_size.len() == 2 {
                        let val = (u64::from(self.frame_content_size[1]) << 8)
                            + (u64::from(self.frame_content_size[0]));
                        Ok(val + 256) //this weird offset is from the documentation. Only if bytes == 2
                    } else {
                        Err(format!(
                            "frame_content_size not long enough. Is: {}, Should be: {}",
                            self.frame_content_size.len(),
                            bytes
                        ))
                    }
                }
                4 => {
                    if self.frame_content_size.len() == 4 {
                        let val = self.frame_content_size[..4]
                            .try_into()
                            .expect("optimized away");
                        let val = u32::from_le_bytes(val);
                        Ok(u64::from(val))
                    } else {
                        Err(format!(
                            "frame_content_size not long enough. Is: {}, Should be: {}",
                            self.frame_content_size.len(),
                            bytes
                        ))
                    }
                }
                8 => {
                    if self.frame_content_size.len() == 8 {
                        let val = self.frame_content_size[..8]
                            .try_into()
                            .expect("optimized away");
                        let val = u64::from_le_bytes(val);
                        Ok(val)
                    } else {
                        Err(format!(
                            "frame_content_size not long enough. Is: {}, Should be: {}",
                            self.frame_content_size.len(),
                            bytes
                        ))
                    }
                }
                _ => Err(format!(
                    "Invalid amount of bytes'. Is: {}, Should be one of 1,2,4,8",
                    self.frame_content_size.len()
                )),
            },
        }
    }
}

impl Frame {
    pub fn check_valid(&self) -> Result<(), String> {
        if self.magic_num != MAGIC_NUM {
            Err(format!(
                "magic_num wrong. Is: {}. Should be: {}",
                self.magic_num, MAGIC_NUM
            ))
        } else if self.header.descriptor.reserved_flag() {
            Err("Reserved Flag set. Must be zero".to_string())
        } else {
            match self.header.dictiornary_id() {
                Ok(_) => match self.header.window_size() {
                    Ok(_) => {
                        if self.header.descriptor.single_segment_flag() {
                            match self.header.frame_content_size() {
                                Ok(_) => Ok(()),
                                Err(m) => Err(m),
                            }
                        } else {
                            Ok(())
                        }
                    }
                    Err(m) => Err(m),
                },
                Err(m) => Err(m),
            }
        }
    }
}

use std::io::Read;
pub fn read_frame_header(r: &mut dyn Read) -> Result<(Frame, u8), String> {
    let mut buf = [0u8; 4];
    let magic_num: u32 = match r.read_exact(&mut buf) {
        Ok(_) => u32::from_le_bytes(buf),
        Err(_) => return Err("Error while reading magic number".to_owned()),
    };

    let mut bytes_read = 4;

    let desc: FrameDescriptor = match r.read_exact(&mut buf[0..1]) {
        Ok(_) => FrameDescriptor(buf[0]),
        Err(_) => return Err("Error while reading frame descriptor".to_owned()),
    };

    bytes_read += 1;

    let mut frame_header = FrameHeader {
        descriptor: FrameDescriptor(desc.0),
        dict_id: match desc.dictionary_id_bytes() {
            Ok(bytes) => vec![0; bytes as usize],
            Err(m) => return Err(m),
        },
        frame_content_size: match desc.frame_content_size_bytes() {
            Ok(bytes) => vec![0; bytes as usize],
            Err(m) => return Err(m),
        },
        window_descriptor: 0,
    };

    if !desc.single_segment_flag() {
        match r.read_exact(&mut buf[0..1]) {
            Ok(_) => frame_header.window_descriptor = buf[0],
            Err(_) => return Err("Error while reading window descriptor".to_owned()),
        }
        bytes_read += 1;
    }

    if !frame_header.dict_id.is_empty() {
        match r.read_exact(frame_header.dict_id.as_mut_slice()) {
            Ok(_) => {}
            Err(_) => return Err("Error while reading dcitionary id".to_owned()),
        }
        bytes_read += frame_header.dict_id.len();
    }

    if !frame_header.frame_content_size.is_empty() {
        match r.read_exact(frame_header.frame_content_size.as_mut_slice()) {
            Ok(_) => {}
            Err(_) => return Err("Error while reading frame content size".to_owned()),
        }
        bytes_read += frame_header.frame_content_size.len();
    }

    let frame: Frame = Frame {
        magic_num,
        header: frame_header,
    };

    Ok((frame, bytes_read as u8))
}
