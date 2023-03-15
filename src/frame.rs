use std::convert::TryInto;
use std::io;

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

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FrameDescriptorError {
    #[error("Invalid Frame_Content_Size_Flag; Is: {got}, Should be one of: 0, 1, 2, 3")]
    InvalidFrameContentSizeFlag { got: u8 },
}

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
    pub fn frame_content_size_bytes(&self) -> Result<u8, FrameDescriptorError> {
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
            other => Err(FrameDescriptorError::InvalidFrameContentSizeFlag { got: other }),
        }
    }

    pub fn dictionary_id_bytes(&self) -> Result<u8, FrameDescriptorError> {
        match self.dict_id_flag() {
            0 => Ok(0),
            1 => Ok(1),
            2 => Ok(2),
            3 => Ok(4),
            other => Err(FrameDescriptorError::InvalidFrameContentSizeFlag { got: other }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FrameHeaderError {
    #[error("window_size bigger than allowed maximum. Is: {got}, Should be lower than: {MAX_WINDOW_SIZE}")]
    WindowTooBig { got: u64 },
    #[error("window_size smaller than allowed minimum. Is: {got}, Should be greater than: {MIN_WINDOW_SIZE}")]
    WindowTooSmall { got: u64 },
    #[error(transparent)]
    FrameDescriptorError(#[from] FrameDescriptorError),
    #[error("Not enough bytes in dict_id. Is: {got}, Should be: {expected}")]
    DictIdTooSmall { got: usize, expected: usize },
    #[error("frame_content_size does not have the right length. Is: {got}, Should be: {expected}")]
    MismatchedFrameSize { got: usize, expected: u8 },
    #[error("frame_content_size was zero")]
    FrameSizeIsZero,
    #[error("Invalid frame_content_size. Is: {got}, Should be one of 1, 2, 4, 8 bytes")]
    InvalidFrameSize { got: u8 },
}

impl FrameHeader {
    pub fn window_size(&self) -> Result<u64, FrameHeaderError> {
        if self.descriptor.single_segment_flag() {
            self.frame_content_size()
        } else {
            let exp = self.window_descriptor >> 3;
            let mantissa = self.window_descriptor & 0x7;

            let window_log = 10 + u64::from(exp);
            let window_base = 1 << window_log;
            let window_add = (window_base / 8) * u64::from(mantissa);

            let window_size = window_base + window_add;

            if window_size >= MIN_WINDOW_SIZE {
                if window_size < MAX_WINDOW_SIZE {
                    Ok(window_size)
                } else {
                    Err(FrameHeaderError::WindowTooBig { got: window_size })
                }
            } else {
                Err(FrameHeaderError::WindowTooSmall { got: window_size })
            }
        }
    }

    pub fn dictionary_id(&self) -> Result<Option<u32>, FrameHeaderError> {
        if self.descriptor.dict_id_flag() == 0 {
            Ok(None)
        } else {
            let bytes = self.descriptor.dictionary_id_bytes()?;
            if self.dict_id.len() != bytes as usize {
                Err(FrameHeaderError::DictIdTooSmall {
                    got: self.dict_id.len(),
                    expected: bytes as usize,
                })
            } else {
                let mut value: u32 = 0;
                let mut shift = 0;
                for x in &self.dict_id {
                    value |= u32::from(*x) << shift;
                    shift += 8;
                }

                Ok(Some(value))
            }
        }
    }

    pub fn frame_content_size(&self) -> Result<u64, FrameHeaderError> {
        let bytes = self.descriptor.frame_content_size_bytes()?;

        if self.frame_content_size.len() != (bytes as usize) {
            return Err(FrameHeaderError::MismatchedFrameSize {
                got: self.frame_content_size.len(),
                expected: bytes,
            });
        }

        match bytes {
            0 => Err(FrameHeaderError::FrameSizeIsZero),
            1 => Ok(u64::from(self.frame_content_size[0])),
            2 => {
                let val = (u64::from(self.frame_content_size[1]) << 8)
                    + (u64::from(self.frame_content_size[0]));
                Ok(val + 256) //this weird offset is from the documentation. Only if bytes == 2
            }
            4 => {
                let val = self.frame_content_size[..4]
                    .try_into()
                    .expect("optimized away");
                let val = u32::from_le_bytes(val);
                Ok(u64::from(val))
            }
            8 => {
                let val = self.frame_content_size[..8]
                    .try_into()
                    .expect("optimized away");
                let val = u64::from_le_bytes(val);
                Ok(val)
            }
            other => Err(FrameHeaderError::InvalidFrameSize { got: other }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FrameCheckError {
    #[error("magic_num wrong. Is: {got}. Should be: {MAGIC_NUM}")]
    WrongMagicNum { got: u32 },
    #[error("Reserved Flag set. Must be zero")]
    ReservedFlagSet,
    #[error(transparent)]
    FrameHeaderError(#[from] FrameHeaderError),
}

impl Frame {
    pub fn check_valid(&self) -> Result<(), FrameCheckError> {
        if self.magic_num != MAGIC_NUM {
            Err(FrameCheckError::WrongMagicNum {
                got: self.magic_num,
            })
        } else if self.header.descriptor.reserved_flag() {
            Err(FrameCheckError::ReservedFlagSet)
        } else {
            self.header.dictionary_id()?;
            self.header.window_size()?;
            if self.header.descriptor.single_segment_flag() {
                self.header.frame_content_size()?;
            }
            Ok(())
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadFrameHeaderError {
    #[error("Error while reading magic number: {0}")]
    MagicNumberReadError(#[source] io::Error),
    #[error("Error while reading frame descriptor: {0}")]
    FrameDescriptorReadError(#[source] io::Error),
    #[error(transparent)]
    InvalidFrameDescriptor(#[from] FrameDescriptorError),
    #[error("Error while reading window descriptor: {0}")]
    WindowDescriptorReadError(#[source] io::Error),
    #[error("Error while reading dictionary id: {0}")]
    DictionaryIdReadError(#[source] io::Error),
    #[error("Error while reading frame content size: {0}")]
    FrameContentSizeReadError(#[source] io::Error),
    #[error("SkippableFrame encountered with MagicNumber 0x{0:X} and length {1} bytes")]
    SkipFrame(u32, u32),
}

use std::io::Read;
pub fn read_frame_header(mut r: impl Read) -> Result<(Frame, u8), ReadFrameHeaderError> {
    use ReadFrameHeaderError as err;
    let mut buf = [0u8; 4];

    r.read_exact(&mut buf).map_err(err::MagicNumberReadError)?;
    let magic_num = u32::from_le_bytes(buf);

    // Skippable frames have a magic number in this interval
    if (0x184D2A50..=0x184D2A5F).contains(&magic_num) {
        r.read_exact(&mut buf)
            .map_err(err::FrameDescriptorReadError)?;
        let skip_size = u32::from_le_bytes(buf);
        return Err(ReadFrameHeaderError::SkipFrame(magic_num, skip_size));
    }

    let mut bytes_read = 4;

    r.read_exact(&mut buf[0..1])
        .map_err(err::FrameDescriptorReadError)?;
    let desc = FrameDescriptor(buf[0]);

    bytes_read += 1;

    let mut frame_header = FrameHeader {
        descriptor: FrameDescriptor(desc.0),
        dict_id: vec![0; desc.dictionary_id_bytes()? as usize],
        frame_content_size: vec![0; desc.frame_content_size_bytes()? as usize],
        window_descriptor: 0,
    };

    if !desc.single_segment_flag() {
        r.read_exact(&mut buf[0..1])
            .map_err(err::WindowDescriptorReadError)?;
        frame_header.window_descriptor = buf[0];
        bytes_read += 1;
    }

    if !frame_header.dict_id.is_empty() {
        r.read_exact(frame_header.dict_id.as_mut_slice())
            .map_err(err::DictionaryIdReadError)?;
        bytes_read += frame_header.dict_id.len();
    }

    if !frame_header.frame_content_size.is_empty() {
        r.read_exact(frame_header.frame_content_size.as_mut_slice())
            .map_err(err::FrameContentSizeReadError)?;
        bytes_read += frame_header.frame_content_size.len();
    }

    let frame: Frame = Frame {
        magic_num,
        header: frame_header,
    };

    Ok((frame, bytes_read as u8))
}
