use crate::io::{Error, Read};
use core::fmt;
#[cfg(feature = "std")]
use std::error::Error as StdError;

pub const MAGIC_NUM: u32 = 0xFD2F_B528;
pub const MIN_WINDOW_SIZE: u64 = 1024;
pub const MAX_WINDOW_SIZE: u64 = (1 << 41) + 7 * (1 << 38);

pub struct Frame {
    pub header: FrameHeader,
}

pub struct FrameHeader {
    pub descriptor: FrameDescriptor,
    window_descriptor: u8,
    dict_id: Option<u32>,
    frame_content_size: u64,
}

pub struct FrameDescriptor(u8);

#[derive(Debug)]
#[non_exhaustive]
pub enum FrameDescriptorError {
    InvalidFrameContentSizeFlag { got: u8 },
}

impl fmt::Display for FrameDescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match Self {
            Self::InvalidFrameContentSizeFlag { got } => write!(f, "Invalid Frame_Content_Size_Flag; Is: {}, Should be one of: 0, 1, 2, 3", got)
        }
    }
}

#[cfg(feature = "std")]
impl StdError for FrameDescriptorError {}

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

#[derive(Debug)]
#[non_exhaustive]
pub enum FrameHeaderError {
    WindowTooBig { got: u64 },
    WindowTooSmall { got: u64 },
    FrameDescriptorError(FrameDescriptorError),
    DictIdTooSmall { got: usize, expected: usize },
    MismatchedFrameSize { got: usize, expected: u8 },
    FrameSizeIsZero,
    InvalidFrameSize { got: u8 },
}

impl fmt::Display for FrameHeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match Self {
            Self::WindowTooBig { got } => write!(f, "window_size bigger than allowed maximum. Is: {}, Should be lower than: {}", got, MAX_WINDOW_SIZE),
            Self::WindowTooSmall { got } => write!(f,  "window_size smaller than allowed minimum. Is: {}, Should be greater than: {}", got, MIN_WINDOW_SIZE),
            Self::FrameDescriptorError(e) => write!(f, "{:?}", e ),
            Self::DictIdTooSmall { got, expected} => write!(f, "Not enough bytes in dict_id. Is: {}, Should be: {}",got, expected),
            Self::MismatchedFrameSize { got, expected } => write!(f, "frame_content_size does not have the right length. Is: {}, Should be: {}", got, expected),
            Self::FrameSizeIsZero => write!(f, "frame_content_size was zero"),
            Self::InvalidFrameSize { got } => write!(f, "Invalid frame_content_size. Is: {}, Should be one of 1, 2, 4, 8 bytes", got),
        }
    }
}

#[cfg(feature = "std")]
impl StdError for FrameHeaderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            FrameHeaderError::FrameDescriptorError(source) => {
                Some(source)
            }
            _ => None,
        }
    }
}

impl From<FrameDescriptorError> for FrameHeaderError {
    fn from(error: FrameDescriptorError) -> Self {
        Self::FrameDescriptorError(error)
    }
}

impl FrameHeader {
    pub fn window_size(&self) -> Result<u64, FrameHeaderError> {
        if self.descriptor.single_segment_flag() {
            Ok(self.frame_content_size())
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

    pub fn dictionary_id(&self) -> Option<u32> {
        self.dict_id
    }

    pub fn frame_content_size(&self) -> u64 {
        self.frame_content_size
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ReadFrameHeaderError {
    MagicNumberReadError(Error),
    BadMagicNumber(u32),
    FrameDescriptorReadError(Error),
    InvalidFrameDescriptor(FrameDescriptorError),
    WindowDescriptorReadError(Error),
    DictionaryIdReadError(Error),
    FrameContentSizeReadError(Error),
    SkipFrame { magic_number: u32, length: u32 },
}

impl fmt::Display for ReadFrameHeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match Self {
            Self::MagicNumberReadError(e) => write!(f, "Error while reading magic number: {}", e),
            Self::BadMagicNumber(e) => write!(f, "Read wrong magic number: 0x{:X}", e),
            Self::FrameDescriptorReadError(e) => write!(f, "Error while reading frame descriptor: {}", e),
            Self::InvalidFrameDescriptor(e) => write!(f, "{:?}", e),
            Self::WindowDescriptorReadError(e) => write!(f, "Error while reading window descriptor: {}", e),
            Self::DictionaryIdReadError(e) => write!(f, "Error while reading dictionary id: {}", e),
            Self::FrameContentSizeReadError(e) => write!(f, "Error while reading frame content size: {}", e),
            Self::SkipFrame { magic_number, length} => write!(f, "SkippableFrame encountered with MagicNumber 0x{:X} and length {} bytes", magic_number, length),
        }
    }
}

#[cfg(feature = "std")]
impl StdError for ReadFrameHeaderError {

    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ReadFrameHeaderError::MagicNumberReadError(source) => {
                Some(source)
            }
            ReadFrameHeaderError::FrameDescriptorReadError(source) => {
                Some(source)
            }
            ReadFrameHeaderError::InvalidFrameDescriptor(source) => {
                Some(source)
            }
            ReadFrameHeaderError::WindowDescriptorReadError(source) => {
                Some(source)
            }
            ReadFrameHeaderError::DictionaryIdReadError(source) => {
                Some(source)
            }
            ReadFrameHeaderError::FrameContentSizeReadError(source) => {
                Some(source)
            }
            _ => None,
        }
    }
}

impl From<FrameDescriptorError> for ReadFrameHeaderError {
    fn from(error: FrameDescriptorError) -> Self {
        Self::InvalidFrameDescriptor(error)
    }
}

pub fn read_frame_header(mut r: impl Read) -> Result<(Frame, u8), ReadFrameHeaderError> {
    use ReadFrameHeaderError as err;
    let mut buf = [0u8; 4];

    r.read_exact(&mut buf).map_err(err::MagicNumberReadError)?;
    let mut bytes_read = 4;
    let magic_num = u32::from_le_bytes(buf);

    // Skippable frames have a magic number in this interval
    if (0x184D2A50..=0x184D2A5F).contains(&magic_num) {
        r.read_exact(&mut buf)
            .map_err(err::FrameDescriptorReadError)?;
        let skip_size = u32::from_le_bytes(buf);
        return Err(ReadFrameHeaderError::SkipFrame {
            magic_number: magic_num,
            length: skip_size,
        });
    }

    if magic_num != MAGIC_NUM {
        return Err(ReadFrameHeaderError::BadMagicNumber(magic_num));
    }

    r.read_exact(&mut buf[0..1])
        .map_err(err::FrameDescriptorReadError)?;
    let desc = FrameDescriptor(buf[0]);

    bytes_read += 1;

    let mut frame_header = FrameHeader {
        descriptor: FrameDescriptor(desc.0),
        dict_id: None,
        frame_content_size: 0,
        window_descriptor: 0,
    };

    if !desc.single_segment_flag() {
        r.read_exact(&mut buf[0..1])
            .map_err(err::WindowDescriptorReadError)?;
        frame_header.window_descriptor = buf[0];
        bytes_read += 1;
    }

    let dict_id_len = desc.dictionary_id_bytes()? as usize;
    if dict_id_len != 0 {
        let buf = &mut buf[..dict_id_len];
        r.read_exact(buf).map_err(err::DictionaryIdReadError)?;
        bytes_read += dict_id_len;
        let mut dict_id = 0u32;

        #[allow(clippy::needless_range_loop)]
        for i in 0..dict_id_len {
            dict_id += (buf[i] as u32) << (8 * i);
        }
        if dict_id != 0 {
            frame_header.dict_id = Some(dict_id);
        }
    }

    let fcs_len = desc.frame_content_size_bytes()? as usize;
    if fcs_len != 0 {
        let mut fcs_buf = [0u8; 8];
        let fcs_buf = &mut fcs_buf[..fcs_len];
        r.read_exact(fcs_buf)
            .map_err(err::FrameContentSizeReadError)?;
        bytes_read += fcs_len;
        let mut fcs = 0u64;

        #[allow(clippy::needless_range_loop)]
        for i in 0..fcs_len {
            fcs += (fcs_buf[i] as u64) << (8 * i);
        }
        if fcs_len == 2 {
            fcs += 256;
        }
        frame_header.frame_content_size = fcs;
    }

    let frame: Frame = Frame {
        header: frame_header,
    };

    Ok((frame, bytes_read as u8))
}
