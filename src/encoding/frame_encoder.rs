//! Utilities and interfaces for encoding an entire frame.

use std::{io::Write, vec::Vec};

use crate::frame;

/// The compression mode used impacts the speed of compression,
/// and resulting compression ratios. Faster compression will result
/// in worse compression ratios, and vice versa.
pub enum CompressionLevel {
    /// This level does not compress the data at all, and simply wraps
    /// it in a Zstandard frame.
    Uncompressed,
    /// This level is roughly equivalent to Zstd compression level 1
    ///
    /// UNIMPLEMENTED
    Fastest,
    /// This level is roughly equivalent to Zstd level 3,
    /// or the one used by the official compressor when no level
    /// is specified.
    ///
    /// UNIMPLEMENTED
    Default,
    /// This level is roughly equivalent to Zstd level 7.
    ///
    /// UNIMPLEMENTED
    Better,
    /// This level is roughly equivalent to Zstd level 11.
    ///
    /// UNIMPLEMENTED
    Best,
}

pub struct FrameEncoder<'input> {
    uncompressed_data: &'input [u8],
    compression_level: CompressionLevel,
}

impl<'input> FrameEncoder<'input> {
    /// Create a new `FrameEncoder` from the provided slice, but don't start compression yet.
    pub fn new(
        uncompressed_data: &'input [u8],
        compression_level: CompressionLevel,
    ) -> FrameEncoder<'_> {
        Self {
            uncompressed_data,
            compression_level,
        }
    }

    /// Compress the uncompressed data into a valid Zstd frame and write it into a buffer,
    /// returning that buffer.
    pub fn encode(&self) -> Vec<u8> {
        // As the data is compressed, it's written here
        let mut output: Vec<u8> = Vec::with_capacity(4096);
        // A Zstandard frame starts with a magic number (4 bytes),
        // and is followed by a frame header (2-4 bytes).
        todo!();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn frame_starts_with_magic_num() {
        todo!();
    }
}
