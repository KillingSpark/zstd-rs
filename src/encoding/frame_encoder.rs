//! Utilities and interfaces for encoding an entire frame.

use core::convert::TryInto;
use std::vec::Vec;

use super::{block_header::BlockHeader, blocks::compress_raw_block, frame_header::FrameHeader};

/// Blocks cannot be larger than 128KB in size.
const MAX_BLOCK_SIZE: usize = 128000;

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

pub struct FrameCompressor<'input> {
    uncompressed_data: &'input [u8],
    compression_level: CompressionLevel,
}

impl<'input> FrameCompressor<'input> {
    /// Create a new `FrameCompressor` from the provided slice, but don't start compression yet.
    pub fn new(
        uncompressed_data: &'input [u8],
        compression_level: CompressionLevel,
    ) -> FrameCompressor<'_> {
        Self {
            uncompressed_data,
            compression_level,
        }
    }

    /// Compress the uncompressed data into a valid Zstd frame and write it into the provided buffer
    pub fn compress(&self, output: &mut Vec<u8>) {
        let header = FrameHeader {
            frame_content_size: Some(self.uncompressed_data.len().try_into().unwrap()),
            single_segment: true,
            content_checksum: false,
            dictionary_id: None,
            window_size: None,
        };
        // TODO: real error handling
        header.serialize(output).unwrap();
        match self.compression_level {
            CompressionLevel::Uncompressed => {
                // Blocks are compressed by writing a header, then writing
                // the block in repetition until the last block is reached.
                let mut index = 0;
                while index < self.uncompressed_data.len() {
                    let last_block = index + MAX_BLOCK_SIZE > self.uncompressed_data.len();
                    // We read till the end of the data, or till the max block size, whichever comes sooner
                    let block_size = if last_block {
                        self.uncompressed_data.len() - index
                    } else {
                        MAX_BLOCK_SIZE
                    };
                    let header = BlockHeader {
                        last_block,
                        block_type: crate::blocks::block::BlockType::Raw,
                        block_size: block_size.try_into().unwrap(),
                    };
                    // Write the header, then the block
                    // TODO: remove this unwrap
                    header.serialize(output).unwrap();
                    compress_raw_block(
                        &self.uncompressed_data[index..(index + block_size)],
                        output,
                    );
                    index += block_size;
                }
            }
            _ => {
                unimplemented!();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FrameCompressor;
    use crate::frame::MAGIC_NUM;
    use std::vec::Vec;

    #[test]
    fn frame_starts_with_magic_num() {
        let mock_data = &[1_u8, 2, 3];
        let compressor = FrameCompressor::new(mock_data, super::CompressionLevel::Uncompressed);
        let mut output: Vec<u8> = Vec::new();
        compressor.compress(&mut output);
        assert!(output.starts_with(&MAGIC_NUM.to_le_bytes()));
    }

    #[test]
    fn very_simple_raw_compress() {
        let mock_data = &[1_u8, 2, 3];
        let compressor = FrameCompressor::new(mock_data, super::CompressionLevel::Uncompressed);
        let mut output: Vec<u8> = Vec::new();
        compressor.compress(&mut output);
    }
}
