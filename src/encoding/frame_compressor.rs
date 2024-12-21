//! Utilities and interfaces for encoding an entire frame.

use alloc::vec::Vec;
use core::convert::TryInto;

use super::{
    block_header::BlockHeader, blocks::compress_block, frame_header::FrameHeader,
    match_generator::MatchGeneratorDriver, Matcher, CompressionLevel
};

use crate::io::{Read, Write};

/// Blocks cannot be larger than 128KB in size.
const MAX_BLOCK_SIZE: usize = 128 * 1024 - 20;

/// An interface for compressing arbitrary data with the ZStandard compression algorithm.
///
/// `FrameCompressor` will generally be used by:
/// 1. Initializing a compressor by providing a buffer of data using `FrameCompressor::new()`
/// 2. Starting compression and writing that compression into a vec using `FrameCompressor::begin`
///
/// # Examples
/// ```
/// use ruzstd::encoding::{FrameCompressor, CompressionLevel};
/// let mock_data: &[_] = &[0x1, 0x2, 0x3, 0x4];
/// let mut output = std::vec::Vec::new();
/// // Initialize a compressor.
/// let mut compressor = FrameCompressor::new(mock_data, &mut output, CompressionLevel::Uncompressed);
///
/// // `compress` writes the compressed output into the provided buffer.
/// compressor.compress();
/// ```
pub struct FrameCompressor<R: Read, W: Write, M: Matcher> {
    uncompressed_data: R,
    compressed_data: W,
    compression_level: CompressionLevel,
    match_generator: M,
}

impl<R: Read, W: Write> FrameCompressor<R, W, MatchGeneratorDriver> {
    /// Create a new `FrameCompressor`
    pub fn new(
        uncompressed_data: R,
        compressed_data: W,
        compression_level: CompressionLevel,
    ) -> Self {
        Self {
            uncompressed_data,
            compressed_data,
            compression_level,
            match_generator: MatchGeneratorDriver::new(1024 * 128, 1),
        }
    }
}

impl<R: Read, W: Write, M: Matcher> FrameCompressor<R, W, M> {
    /// Create a new `FrameCompressor` with a custom matching algorithm implementation
    pub fn new_with_matcher(
        source: R,
        drain: W,
        matcher: M,
        compression_level: CompressionLevel,
    ) -> Self {
        Self {
            uncompressed_data: source,
            compressed_data: drain,
            match_generator: matcher,
            compression_level,
        }
    }

    /// After calling [FrameCompressor::compress] you can replace the source and call [FrameCompressor::compress] again
    pub fn replace_source(&mut self, mut uncompressed_data: R) -> R {
        std::mem::swap(&mut uncompressed_data, &mut self.uncompressed_data);
        uncompressed_data
    }

    /// Before calling [FrameCompressor::compress] you can replace the drain
    pub fn replace_drain(&mut self, mut compressed_data: W) -> W {
        std::mem::swap(&mut compressed_data, &mut self.compressed_data);
        compressed_data
    }

    /// Before calling [FrameCompressor::compress] you can replace the matcher
    pub fn replace_matcher(&mut self, mut match_generator: M) -> M {
        std::mem::swap(&mut match_generator, &mut self.match_generator);
        match_generator
    }

    /// Before calling [FrameCompressor::compress] you can replace the compression level
    pub fn replace_compression_level(
        &mut self,
        mut compression_level: CompressionLevel,
    ) -> CompressionLevel {
        std::mem::swap(&mut compression_level, &mut self.compression_level);
        compression_level
    }

    /// Compress the uncompressed data into a valid Zstd frame and write it into the provided buffer
    pub fn compress(&mut self) {
        self.match_generator.reset(self.compression_level);

        let mut output = Vec::with_capacity(1024 * 130);
        let output = &mut output;
        let header = FrameHeader {
            frame_content_size: None,
            single_segment: false,
            content_checksum: false,
            dictionary_id: None,
            window_size: Some(self.match_generator.window_size()),
        };
        header.serialize(output);

        loop {
            let mut uncompressed_data = self.match_generator.get_next_space();
            let mut read_bytes = 0;
            let last_block;
            'read_loop: loop {
                let new_bytes = self
                    .uncompressed_data
                    .read(&mut uncompressed_data[read_bytes..])
                    .unwrap();
                if new_bytes == 0 {
                    last_block = true;
                    break 'read_loop;
                }
                read_bytes += new_bytes;
                if read_bytes == uncompressed_data.len() {
                    last_block = false;
                    break 'read_loop;
                }
            }
            uncompressed_data.resize(read_bytes, 0);

            // Special handling is needed for compression of a totally empty file (why you'd want to do that, I don't know)
            if uncompressed_data.is_empty() {
                let header = BlockHeader {
                    last_block: true,
                    block_type: crate::blocks::block::BlockType::Raw,
                    block_size: 0,
                };
                // Write the header, then the block
                header.serialize(output);
                self.compressed_data.write_all(output).unwrap();
                output.clear();
                break;
            }

            match self.compression_level {
                CompressionLevel::Uncompressed => {
                    let header = BlockHeader {
                        last_block,
                        block_type: crate::blocks::block::BlockType::Raw,
                        block_size: read_bytes.try_into().unwrap(),
                    };
                    // Write the header, then the block
                    header.serialize(output);
                    output.extend_from_slice(&uncompressed_data);
                }
                CompressionLevel::Fastest => {
                    if uncompressed_data.iter().all(|x| uncompressed_data[0].eq(x)) {
                        let rle_byte = uncompressed_data[0];
                        self.match_generator.commit_space(uncompressed_data);
                        self.match_generator.skip_matching();
                        let header = BlockHeader {
                            last_block,
                            block_type: crate::blocks::block::BlockType::RLE,
                            block_size: read_bytes.try_into().unwrap(),
                        };
                        // Write the header, then the block
                        header.serialize(output);
                        output.push(rle_byte);
                    } else {
                        let mut compressed = Vec::new();
                        self.match_generator.commit_space(uncompressed_data);
                        compress_block(&mut self.match_generator, &mut compressed);
                        if compressed.len() >= MAX_BLOCK_SIZE {
                            let header = BlockHeader {
                                last_block,
                                block_type: crate::blocks::block::BlockType::Raw,
                                block_size: read_bytes.try_into().unwrap(),
                            };
                            // Write the header, then the block
                            header.serialize(output);
                            output.extend_from_slice(self.match_generator.get_last_space());
                        } else {
                            let header = BlockHeader {
                                last_block,
                                block_type: crate::blocks::block::BlockType::Compressed,
                                block_size: (compressed.len()).try_into().unwrap(),
                            };
                            // Write the header, then the block
                            header.serialize(output);
                            output.extend(compressed);
                        }
                    }
                }
                _ => {
                    unimplemented!();
                }
            }
            self.compressed_data.write_all(output).unwrap();
            output.clear();
            if last_block {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::FrameCompressor;
    use crate::decoding::{frame::MAGIC_NUM, FrameDecoder};
    use alloc::vec::Vec;

    #[test]
    fn frame_starts_with_magic_num() {
        let mock_data = [1_u8, 2, 3].as_slice();
        let mut output: Vec<u8> = Vec::new();
        let mut compressor = FrameCompressor::new(
            mock_data,
            &mut output,
            super::CompressionLevel::Uncompressed,
        );
        compressor.compress();
        assert!(output.starts_with(&MAGIC_NUM.to_le_bytes()));
    }

    #[test]
    fn very_simple_raw_compress() {
        let mock_data = [1_u8, 2, 3].as_slice();
        let mut output: Vec<u8> = Vec::new();
        let mut compressor = FrameCompressor::new(
            mock_data,
            &mut output,
            super::CompressionLevel::Uncompressed,
        );
        compressor.compress();
    }

    #[test]
    fn very_simple_compress() {
        let mut mock_data = vec![0; 1 << 17];
        mock_data.extend(vec![1; (1 << 17) - 1]);
        mock_data.extend(vec![2; (1 << 18) - 1]);
        mock_data.extend(vec![2; 1 << 17]);
        mock_data.extend(vec![3; (1 << 17) - 1]);
        let mut output: Vec<u8> = Vec::new();
        let mut compressor = FrameCompressor::new(
            mock_data.as_slice(),
            &mut output,
            super::CompressionLevel::Uncompressed,
        );
        compressor.compress();

        let mut decoder = FrameDecoder::new();
        let mut decoded = Vec::with_capacity(mock_data.len());
        decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
        assert_eq!(mock_data, decoded);

        let mut decoded = Vec::new();
        zstd::stream::copy_decode(output.as_slice(), &mut decoded).unwrap();
        assert_eq!(mock_data, decoded);
    }

    #[test]
    fn rle_compress() {
        let mock_data = vec![0; 1 << 19];
        let mut output: Vec<u8> = Vec::new();
        let mut compressor = FrameCompressor::new(
            mock_data.as_slice(),
            &mut output,
            super::CompressionLevel::Uncompressed,
        );
        compressor.compress();

        let mut decoder = FrameDecoder::new();
        let mut decoded = Vec::with_capacity(mock_data.len());
        decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
        assert_eq!(mock_data, decoded);
    }

    #[test]
    fn aaa_compress() {
        let mock_data = vec![0, 1, 3, 4, 5];
        let mut output: Vec<u8> = Vec::new();
        let mut compressor = FrameCompressor::new(
            mock_data.as_slice(),
            &mut output,
            super::CompressionLevel::Uncompressed,
        );
        compressor.compress();

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
            let mut decoder = crate::decoding::StreamingDecoder::new(data).unwrap();
            let mut result: Vec<u8> = Vec::new();
            decoder.read_to_end(&mut result).expect("Decoding failed");
            result
        }

        fn decode_ruzstd_writer(mut data: impl Read) -> Vec<u8> {
            let mut decoder = crate::decoding::FrameDecoder::new();
            decoder.reset(&mut data).unwrap();
            let mut result = vec![];
            while !decoder.is_finished() || decoder.can_collect() > 0 {
                decoder
                    .decode_blocks(
                        &mut data,
                        crate::decoding::BlockDecodingStrategy::UptoBytes(1024 * 1024),
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
            let mut output = Vec::new();

            let mut compressor = crate::encoding::FrameCompressor::new(
                input.as_slice(),
                &mut output,
                crate::encoding::CompressionLevel::Uncompressed,
            );
            compressor.compress();
            output
        }

        fn encode_ruzstd_compressed(data: &mut dyn std::io::Read) -> Vec<u8> {
            let mut input = Vec::new();
            data.read_to_end(&mut input).unwrap();
            let mut output = Vec::new();

            let mut compressor = crate::encoding::FrameCompressor::new(
                input.as_slice(),
                &mut output,
                crate::encoding::CompressionLevel::Uncompressed,
            );
            compressor.compress();
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
                    let compressed = encode_zstd(data).unwrap();
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
                    let decoded = decode_zstd(&compressed).unwrap();
                    assert_eq!(
                        decoded, data,
                        "Decoded data did not match the original input during compression"
                    );
                    // Compressed encoding
                    let mut input = data;
                    let compressed = encode_ruzstd_compressed(&mut input);
                    let decoded = decode_zstd(&compressed).unwrap();
                    assert_eq!(
                        decoded, data,
                        "Decoded data did not match the original input during compression"
                    );
                }
            }
        }
    }
}
