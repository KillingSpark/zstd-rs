//! Utilities and interfaces for encoding an entire frame.

use alloc::vec::Vec;
use core::convert::TryInto;

use super::{
    block_header::BlockHeader,
    blocks::{compress_block, compress_raw_block},
    frame_header::FrameHeader,
};

/// Blocks cannot be larger than 128KB in size.
const MAX_BLOCK_SIZE: usize = 128 * 1024 - 20;

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
/// An interface for compressing arbitrary data with the ZStandard compression algorithm.
///
/// `FrameCompressor` will generally be used by:
/// 1. Initializing a compressor by providing a buffer of data using `FrameCompressor::new()`
/// 2. Starting compression and writing that compression into a vec using `FrameCompressor::begin`
///
/// # Examples
/// ```
/// use ruzstd::encoding::{FrameCompressor, CompressionLevel};
/// let mock_data = &[0x1, 0x2, 0x3, 0x4];
/// // Initialize a compressor.
/// let compressor = FrameCompressor::new(mock_data, CompressionLevel::Uncompressed);
///
/// let mut output = Vec::new();
/// // `compress` writes the compressed output into the provided buffer.
/// compressor.compress(&mut output);
/// ```
pub struct FrameCompressor<'input> {
    uncompressed_data: &'input [u8],
    compression_level: CompressionLevel,
}

impl<'input> FrameCompressor<'input> {
    /// Create a new `FrameCompressor` from the provided slice, but don't start compression yet.
    pub fn new(
        uncompressed_data: &'input [u8],
        compression_level: CompressionLevel,
    ) -> FrameCompressor<'input> {
        Self {
            uncompressed_data,
            compression_level,
        }
    }

    /// Compress the uncompressed data into a valid Zstd frame and write it into the provided buffer
    pub fn compress(&self, output: &mut Vec<u8>) {
        let header = FrameHeader {
            frame_content_size: None,
            single_segment: false,
            content_checksum: false,
            dictionary_id: None,
            window_size: Some(256 * 1024),
        };
        header.serialize(output);
        // Special handling is needed for compression of a totally empty file (why you'd want to do that, I don't know)
        if self.uncompressed_data.is_empty() {
            let header = BlockHeader {
                last_block: true,
                block_type: crate::blocks::block::BlockType::Raw,
                block_size: 0,
            };
            // Write the header, then the block
            header.serialize(output);
        }
        match self.compression_level {
            CompressionLevel::Uncompressed => {
                // Blocks are compressed by writing a header, then writing
                // the block in repetition until the last block is reached.
                let mut index = 0;
                while index < self.uncompressed_data.len() {
                    let last_block = index + MAX_BLOCK_SIZE >= self.uncompressed_data.len();
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
                    header.serialize(output);
                    compress_raw_block(
                        &self.uncompressed_data[index..(index + block_size)],
                        output,
                    );
                    index += block_size;
                }
            }
            CompressionLevel::Fastest => {
                let mut index = 0;
                while index < self.uncompressed_data.len() {
                    let last_block = index + MAX_BLOCK_SIZE >= self.uncompressed_data.len();
                    // We read till the end of the data, or till the max block size, whichever comes sooner
                    let block_size = if last_block {
                        self.uncompressed_data.len() - index
                    } else {
                        MAX_BLOCK_SIZE
                    };

                    let uncompressed = &self.uncompressed_data[index..(index + block_size)];

                    if uncompressed.iter().all(|x| uncompressed[0].eq(x)) {
                        let header = BlockHeader {
                            last_block,
                            block_type: crate::blocks::block::BlockType::RLE,
                            block_size: uncompressed.len().try_into().unwrap(),
                        };
                        // Write the header, then the block
                        header.serialize(output);
                        output.push(uncompressed[0]);
                    } else {
                        let compressed = compress_block(uncompressed);
                        let header = BlockHeader {
                            last_block,
                            block_type: crate::blocks::block::BlockType::Compressed,
                            block_size: (compressed.len()).try_into().unwrap(),
                        };
                        // Write the header, then the block
                        header.serialize(output);
                        output.extend(compressed);
                    }
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
    use alloc::vec;

    use super::FrameCompressor;
    use crate::{frame::MAGIC_NUM, FrameDecoder};
    use alloc::vec::Vec;

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

    #[test]
    fn very_simple_compress() {
        let mut mock_data = vec![0; 1 << 17];
        mock_data.extend(vec![1; (1 << 17) - 1]);
        mock_data.extend(vec![2; (1 << 18) - 1]);
        mock_data.extend(vec![2; 1 << 17]);
        mock_data.extend(vec![3; (1 << 17) - 1]);
        let compressor = FrameCompressor::new(&mock_data, super::CompressionLevel::Fastest);
        let mut output: Vec<u8> = Vec::new();
        compressor.compress(&mut output);

        let mut decoder = FrameDecoder::new();
        let mut decoded = Vec::with_capacity(mock_data.len());
        decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
        assert_eq!(mock_data, decoded);
    }

    #[test]
    fn rle_compress() {
        let mock_data = vec![0; 1 << 19];
        let compressor = FrameCompressor::new(&mock_data, super::CompressionLevel::Fastest);
        let mut output: Vec<u8> = Vec::new();
        compressor.compress(&mut output);

        let mut decoder = FrameDecoder::new();
        let mut decoded = Vec::with_capacity(mock_data.len());
        decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
        assert_eq!(mock_data, decoded);
    }

    #[test]
    fn aaa_compress() {
        let mock_data = vec![0, 1, 3, 4, 5];
        let compressor = FrameCompressor::new(&mock_data, super::CompressionLevel::Fastest);
        let mut output: Vec<u8> = Vec::new();
        compressor.compress(&mut output);

        let mut decoder = FrameDecoder::new();
        let mut decoded = Vec::with_capacity(mock_data.len());
        decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
        assert_eq!(mock_data, decoded);

        let mut decoded = Vec::new();
        zstd::stream::copy_decode(output.as_slice(), &mut decoded).unwrap();
        assert_eq!(mock_data, decoded);
    }

    #[cfg(feature = "std")]
    #[test]
    fn fuzz_targets() {
        use std::io::Read;
        fn decode_ruzstd(data: &mut dyn std::io::Read) -> Vec<u8> {
            let mut decoder = crate::StreamingDecoder::new(data).unwrap();
            let mut result: Vec<u8> = Vec::new();
            decoder.read_to_end(&mut result).expect("Decoding failed");
            result
        }

        fn decode_ruzstd_writer(mut data: impl Read) -> Vec<u8> {
            let mut decoder = crate::FrameDecoder::new();
            decoder.reset(&mut data).unwrap();
            let mut result = vec![];
            while !decoder.is_finished() || decoder.can_collect() > 0 {
                decoder
                    .decode_blocks(
                        &mut data,
                        crate::BlockDecodingStrategy::UptoBytes(1024 * 1024),
                    )
                    .unwrap();
                decoder.collect_to_writer(&mut result).unwrap();
            }
            result
        }

        fn encode_zstd(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
            zstd::stream::encode_all(std::io::Cursor::new(data), 3)
        }

        fn encode_ruzstd_uncompressed(data: &mut dyn std::io::Read) -> Vec<u8> {
            let mut input = Vec::new();
            data.read_to_end(&mut input).unwrap();
            let compressor = crate::encoding::FrameCompressor::new(
                &input,
                crate::encoding::CompressionLevel::Uncompressed,
            );
            let mut output = Vec::new();
            compressor.compress(&mut output);
            output
        }

        fn encode_ruzstd_compressed(data: &mut dyn std::io::Read) -> Vec<u8> {
            let mut input = Vec::new();
            data.read_to_end(&mut input).unwrap();
            let compressor = crate::encoding::FrameCompressor::new(
                &input,
                crate::encoding::CompressionLevel::Fastest,
            );
            let mut output = Vec::new();
            compressor.compress(&mut output);
            output
        }

        fn decode_zstd(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
            let mut output = Vec::new();
            zstd::stream::copy_decode(data, &mut output)?;
            Ok(output)
        }
        if std::fs::exists("fuzz/artifacts/interop").unwrap_or(false) {
            for file in std::fs::read_dir("fuzz/artifacts/interop").unwrap() {
                if file.as_ref().unwrap().file_type().unwrap().is_file() {
                    let data = std::fs::read(file.unwrap().path()).unwrap();
                    let data = data.as_slice();
                    // Decoding
                    let compressed = encode_zstd(&data).unwrap();
                    let decoded = decode_ruzstd(&mut compressed.as_slice());
                    let decoded2 = decode_ruzstd_writer(&mut compressed.as_slice());
                    assert!(
                        decoded == data,
                        "Decoded data did not match the original input during decompression"
                    );
                    assert_eq!(
                        decoded2, data,
                        "Decoded data did not match the original input during decompression"
                    );

                    // Encoding
                    // Uncompressed encoding
                    let mut input = data;
                    let compressed = encode_ruzstd_uncompressed(&mut input);
                    let mut input = data;
                    let compressed2 = encode_ruzstd_compressed(&mut input);
                    let decoded = decode_zstd(&compressed).unwrap();
                    assert_eq!(
                        decoded, data,
                        "Decoded data did not match the original input during compression"
                    );
                    let decoded2 = decode_zstd(&compressed2).unwrap();
                    assert_eq!(
                        decoded2, data,
                        "Decoded data did not match the original input during compression"
                    );
                }
            }
        }
    }
}
