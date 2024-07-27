//! Utilities and representations for a frame header.

/// A header for a single Zstandard frame.
///
/// https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#frame_header
pub struct FrameHeader {
    /// Optionally, the original (uncompressed) size of the data within the frame in bytes.
    pub frame_content_size: Option<u64>,
    /// If set to true, data must be regenerated within a single
    /// continuous memory segment
    pub single_segment: bool,
    /// If set to true, a 32 bit content checksum will be present
    /// at the end of the frame.
    pub content_checksum: bool,
    /// If a dictionary ID is provided, the ID of that dictionary.
    pub dictionary_id: Option<u64>,
    /// The minimum memory buffer required to compress a frame. If not present,
    /// `single_segment` will be set to true. If present, this value must be greater than 1KB
    /// and less than 3.75TB. Encoders should not generate a frame that requires a window size larger than
    /// 8mb.
    pub window_size: Option<u64>,
}

impl FrameHeader {
    /// Serialize the frame header into a buffer
    pub fn serialize() {}
}
