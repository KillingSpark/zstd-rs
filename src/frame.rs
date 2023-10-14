use crate::io::{Error, Read};
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

#[derive(Debug, derive_more::Display)]
#[cfg_attr(feature = "std", derive(derive_more::Error))]
#[non_exhaustive]
pub enum FrameDescriptorError {
    #[display(fmt = "Invalid Frame_Content_Size_Flag; Is: {got}, Should be one of: 0, 1, 2, 3")]
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

#[derive(Debug, derive_more::Display, derive_more::From)]
#[cfg_attr(feature = "std", derive(derive_more::Error))]
#[non_exhaustive]
pub enum FrameHeaderError {
    #[display(
        fmt = "window_size bigger than allowed maximum. Is: {got}, Should be lower than: {MAX_WINDOW_SIZE}"
    )]
    WindowTooBig { got: u64 },
    #[display(
        fmt = "window_size smaller than allowed minimum. Is: {got}, Should be greater than: {MIN_WINDOW_SIZE}"
    )]
    WindowTooSmall { got: u64 },
    #[display(fmt = "{_0:?}")]
    #[from]
    FrameDescriptorError(FrameDescriptorError),
    #[display(fmt = "Not enough bytes in dict_id. Is: {got}, Should be: {expected}")]
    DictIdTooSmall { got: usize, expected: usize },
    #[display(
        fmt = "frame_content_size does not have the right length. Is: {got}, Should be: {expected}"
    )]
    MismatchedFrameSize { got: usize, expected: u8 },
    #[display(fmt = "frame_content_size was zero")]
    FrameSizeIsZero,
    #[display(fmt = "Invalid frame_content_size. Is: {got}, Should be one of 1, 2, 4, 8 bytes")]
    InvalidFrameSize { got: u8 },
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

#[derive(Debug, derive_more::Display, derive_more::From)]
#[cfg_attr(feature = "std", derive(derive_more::Error))]
#[non_exhaustive]
pub enum ReadFrameHeaderError {
    #[display(fmt = "Error while reading magic number: {_0}")]
    MagicNumberReadError(Error),
    #[display(fmt = "Read wrong magic number: 0x{_0:X}")]
    BadMagicNumber(#[cfg_attr(feature = "std", error(ignore))] u32),
    #[display(fmt = "Error while reading frame descriptor: {_0}")]
    FrameDescriptorReadError(Error),
    #[display(fmt = "{_0:?}")]
    #[from]
    InvalidFrameDescriptor(FrameDescriptorError),
    #[display(fmt = "Error while reading window descriptor: {_0}")]
    WindowDescriptorReadError(Error),
    #[display(fmt = "Error while reading dictionary id: {_0}")]
    DictionaryIdReadError(Error),
    #[display(fmt = "Error while reading frame content size: {_0}")]
    FrameContentSizeReadError(Error),
    #[display(fmt = "SkippableFrame encountered with MagicNumber 0x{_0:X} and length {_1} bytes")]
    SkipFrame(u32, u32),
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
        return Err(ReadFrameHeaderError::SkipFrame(magic_num, skip_size));
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
