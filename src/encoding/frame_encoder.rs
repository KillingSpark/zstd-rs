//! Utilities and interfaces for encoding an entire frame.

use std::{io::Write, vec::Vec};

use crate::frame;

pub struct FrameEncoder<'input> {
    uncompressed_data: &'input [u8],
}

impl<'input> FrameEncoder<'input> {
    /// Create a new `FrameEncoder` from the provided slice, but don't start compression yet.
    pub fn new(uncompressed_data: &'input [u8]) -> FrameEncoder<'_> {
        Self { uncompressed_data }
    }

    /// Compress the uncompressed data into a valid Zstd frame and write it into a buffer,
    /// returning that buffer.
    ///
    /// Internally, this function works by defining an index into the uncompressed data,
    /// then studying the data before and after the index to determine:
    ///
    /// - What kind of block should come next
    /// - How large should that block be
    ///
    /// The function then starts compression for that block, and advances
    /// the index to the end of that section, repeating the process until the
    /// end of the buffer is reached.
    pub fn encode_frame(&self) -> Vec<u8> {
        // As the data is compressed, it's written here
        let mut output: Vec<u8> = Vec::with_capacity(4096);
        // A Zstandard frame starts with a magic number (4 bytes),
        // and is followed by a frame header (2-4 bytes).
        output.extend_from_slice(&frame::MAGIC_NUM.to_le_bytes());
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
