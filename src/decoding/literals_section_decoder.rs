//! This module contains the [decompress_literals] function, used to take a
//! parsed literals header and a source and decompress it.

use super::super::blocks::literals_section::{LiteralsSection, LiteralsSectionType};
use super::bit_reader_reverse::{BitReaderReversed, GetBitsError};
use super::scratch::HuffmanScratch;
use crate::huff0::{HuffmanDecoder, HuffmanDecoderError, HuffmanTableError};
use alloc::vec::Vec;

#[derive(Debug)]
#[non_exhaustive]
pub enum DecompressLiteralsError {
    MissingCompressedSize,
    MissingNumStreams,
    GetBitsError(GetBitsError),
    HuffmanTableError(HuffmanTableError),
    HuffmanDecoderError(HuffmanDecoderError),
    UninitializedHuffmanTable,
    MissingBytesForJumpHeader { got: usize },
    MissingBytesForLiterals { got: usize, needed: usize },
    ExtraPadding { skipped_bits: i32 },
    BitstreamReadMismatch { read_til: isize, expected: isize },
    DecodedLiteralCountMismatch { decoded: usize, expected: usize },
}

#[cfg(feature = "std")]
impl std::error::Error for DecompressLiteralsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecompressLiteralsError::GetBitsError(source) => Some(source),
            DecompressLiteralsError::HuffmanTableError(source) => Some(source),
            DecompressLiteralsError::HuffmanDecoderError(source) => Some(source),
            _ => None,
        }
    }
}
impl core::fmt::Display for DecompressLiteralsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecompressLiteralsError::MissingCompressedSize => {
                write!(f,
                    "compressed size was none even though it must be set to something for compressed literals",
                )
            }
            DecompressLiteralsError::MissingNumStreams => {
                write!(f,
                    "num_streams was none even though it must be set to something (1 or 4) for compressed literals",
                )
            }
            DecompressLiteralsError::GetBitsError(e) => write!(f, "{:?}", e),
            DecompressLiteralsError::HuffmanTableError(e) => write!(f, "{:?}", e),
            DecompressLiteralsError::HuffmanDecoderError(e) => write!(f, "{:?}", e),
            DecompressLiteralsError::UninitializedHuffmanTable => {
                write!(
                    f,
                    "Tried to reuse huffman table but it was never initialized",
                )
            }
            DecompressLiteralsError::MissingBytesForJumpHeader { got } => {
                write!(f, "Need 6 bytes to decode jump header, got {} bytes", got,)
            }
            DecompressLiteralsError::MissingBytesForLiterals { got, needed } => {
                write!(
                    f,
                    "Need at least {} bytes to decode literals. Have: {} bytes",
                    needed, got,
                )
            }
            DecompressLiteralsError::ExtraPadding { skipped_bits } => {
                write!(f,
                    "Padding at the end of the sequence_section was more than a byte long: {} bits. Probably caused by data corruption",
                    skipped_bits,
                )
            }
            DecompressLiteralsError::BitstreamReadMismatch { read_til, expected } => {
                write!(
                    f,
                    "Bitstream was read till: {}, should have been: {}",
                    read_til, expected,
                )
            }
            DecompressLiteralsError::DecodedLiteralCountMismatch { decoded, expected } => {
                write!(
                    f,
                    "Did not decode enough literals: {}, Should have been: {}",
                    decoded, expected,
                )
            }
        }
    }
}

impl From<HuffmanDecoderError> for DecompressLiteralsError {
    fn from(val: HuffmanDecoderError) -> Self {
        Self::HuffmanDecoderError(val)
    }
}

impl From<GetBitsError> for DecompressLiteralsError {
    fn from(val: GetBitsError) -> Self {
        Self::GetBitsError(val)
    }
}

impl From<HuffmanTableError> for DecompressLiteralsError {
    fn from(val: HuffmanTableError) -> Self {
        Self::HuffmanTableError(val)
    }
}

/// Decode and decompress the provided literals section into `target`, returning the number of bytes read.
pub fn decode_literals(
    section: &LiteralsSection,
    scratch: &mut HuffmanScratch,
    source: &[u8],
    target: &mut Vec<u8>,
) -> Result<u32, DecompressLiteralsError> {
    match section.ls_type {
        LiteralsSectionType::Raw => {
            target.extend(&source[0..section.regenerated_size as usize]);
            Ok(section.regenerated_size)
        }
        LiteralsSectionType::RLE => {
            target.resize(target.len() + section.regenerated_size as usize, source[0]);
            Ok(1)
        }
        LiteralsSectionType::Compressed | LiteralsSectionType::Treeless => {
            let bytes_read = decompress_literals(section, scratch, source, target)?;

            //return sum of used bytes
            Ok(bytes_read)
        }
    }
}

/// Decompress the provided literals section and source into the provided `target`.
/// This function is used when the literals section is `Compressed` or `Treeless`
///
/// Returns the number of bytes read.
fn decompress_literals(
    section: &LiteralsSection,
    scratch: &mut HuffmanScratch,
    source: &[u8],
    target: &mut Vec<u8>,
) -> Result<u32, DecompressLiteralsError> {
    use DecompressLiteralsError as err;

    let compressed_size = section.compressed_size.ok_or(err::MissingCompressedSize)? as usize;
    let num_streams = section.num_streams.ok_or(err::MissingNumStreams)?;

    target.reserve(section.regenerated_size as usize);
    let source = &source[0..compressed_size];
    let mut bytes_read = 0;

    match section.ls_type {
        LiteralsSectionType::Compressed => {
            //read Huffman tree description
            bytes_read += scratch.table.build_decoder(source)?;
            vprintln!("Built huffman table using {} bytes", bytes_read);
        }
        LiteralsSectionType::Treeless => {
            if scratch.table.max_num_bits == 0 {
                return Err(err::UninitializedHuffmanTable);
            }
        }
        _ => { /* nothing to do, huffman tree has been provided by previous block */ }
    }

    let source = &source[bytes_read as usize..];

    if num_streams == 4 {
        //build jumptable
        if source.len() < 6 {
            return Err(err::MissingBytesForJumpHeader { got: source.len() });
        }
        let jump1 = source[0] as usize + ((source[1] as usize) << 8);
        let jump2 = jump1 + source[2] as usize + ((source[3] as usize) << 8);
        let jump3 = jump2 + source[4] as usize + ((source[5] as usize) << 8);
        bytes_read += 6;
        let source = &source[6..];

        if source.len() < jump3 {
            return Err(err::MissingBytesForLiterals {
                got: source.len(),
                needed: jump3,
            });
        }

        //decode 4 streams
        let stream1 = &source[..jump1];
        let stream2 = &source[jump1..jump2];
        let stream3 = &source[jump2..jump3];
        let stream4 = &source[jump3..];

        for stream in &[stream1, stream2, stream3, stream4] {
            let mut decoder = HuffmanDecoder::new(&scratch.table);
            let mut br = BitReaderReversed::new(stream);
            //skip the 0 padding at the end of the last byte of the bit stream and throw away the first 1 found
            let mut skipped_bits = 0;
            loop {
                let val = br.get_bits(1);
                skipped_bits += 1;
                if val == 1 || skipped_bits > 8 {
                    break;
                }
            }
            if skipped_bits > 8 {
                //if more than 7 bits are 0, this is not the correct end of the bitstream. Either a bug or corrupted data
                return Err(DecompressLiteralsError::ExtraPadding { skipped_bits });
            }
            decoder.init_state(&mut br);

            while br.bits_remaining() > -(scratch.table.max_num_bits as isize) {
                target.push(decoder.decode_symbol());
                decoder.next_state(&mut br);
            }
            if br.bits_remaining() != -(scratch.table.max_num_bits as isize) {
                return Err(DecompressLiteralsError::BitstreamReadMismatch {
                    read_til: br.bits_remaining(),
                    expected: -(scratch.table.max_num_bits as isize),
                });
            }
        }

        bytes_read += source.len() as u32;
    } else {
        //just decode the one stream
        assert!(num_streams == 1);
        let mut decoder = HuffmanDecoder::new(&scratch.table);
        let mut br = BitReaderReversed::new(source);
        let mut skipped_bits = 0;
        loop {
            let val = br.get_bits(1);
            skipped_bits += 1;
            if val == 1 || skipped_bits > 8 {
                break;
            }
        }
        if skipped_bits > 8 {
            //if more than 7 bits are 0, this is not the correct end of the bitstream. Either a bug or corrupted data
            return Err(DecompressLiteralsError::ExtraPadding { skipped_bits });
        }
        decoder.init_state(&mut br);
        while br.bits_remaining() > -(scratch.table.max_num_bits as isize) {
            target.push(decoder.decode_symbol());
            decoder.next_state(&mut br);
        }
        bytes_read += source.len() as u32;
    }

    if target.len() != section.regenerated_size as usize {
        return Err(DecompressLiteralsError::DecodedLiteralCountMismatch {
            decoded: target.len(),
            expected: section.regenerated_size as usize,
        });
    }

    Ok(bytes_read)
}
