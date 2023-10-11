use super::super::blocks::literals_section::{LiteralsSection, LiteralsSectionType};
use super::bit_reader_reverse::{BitReaderReversed, GetBitsError};
use super::scratch::HuffmanScratch;
use crate::huff0::{HuffmanDecoder, HuffmanDecoderError, HuffmanTableError};
use alloc::vec::Vec;

#[cfg(not(feature = "std"))]
use crate::std;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecompressLiteralsError {
    #[error(
        "compressed size was none even though it must be set to something for compressed literals"
    )]
    MissingCompressedSize,
    #[error("num_streams was none even though it must be set to something (1 or 4) for compressed literals")]
    MissingNumStreams,
    #[error(transparent)]
    GetBitsError(#[from] GetBitsError),
    #[error(transparent)]
    HuffmanTableError(#[from] HuffmanTableError),
    #[error(transparent)]
    HuffmanDecoderError(#[from] HuffmanDecoderError),
    #[error("Tried to reuse huffman table but it was never initialized")]
    UninitializedHuffmanTable,
    #[error("Need 6 bytes to decode jump header, got {got} bytes")]
    MissingBytesForJumpHeader { got: usize },
    #[error("Need at least {needed} bytes to decode literals. Have: {got} bytes")]
    MissingBytesForLiterals { got: usize, needed: usize },
    #[error("Padding at the end of the sequence_section was more than a byte long: {skipped_bits} bits. Probably caused by data corruption")]
    ExtraPadding { skipped_bits: i32 },
    #[error("Bitstream was read till: {read_til}, should have been: {expected}")]
    BitstreamReadMismatch { read_til: isize, expected: isize },
    #[error("Did not decode enough literals: {decoded}, Should have been: {expected}")]
    DecodedLiteralCountMismatch { decoded: usize, expected: usize },
}

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
                let val = br.get_bits(1)?;
                skipped_bits += 1;
                if val == 1 || skipped_bits > 8 {
                    break;
                }
            }
            if skipped_bits > 8 {
                //if more than 7 bits are 0, this is not the correct end of the bitstream. Either a bug or corrupted data
                return Err(DecompressLiteralsError::ExtraPadding { skipped_bits });
            }
            decoder.init_state(&mut br)?;

            while br.bits_remaining() > -(scratch.table.max_num_bits as isize) {
                target.push(decoder.decode_symbol());
                decoder.next_state(&mut br)?;
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
            let val = br.get_bits(1)?;
            skipped_bits += 1;
            if val == 1 || skipped_bits > 8 {
                break;
            }
        }
        if skipped_bits > 8 {
            //if more than 7 bits are 0, this is not the correct end of the bitstream. Either a bug or corrupted data
            return Err(DecompressLiteralsError::ExtraPadding { skipped_bits });
        }
        decoder.init_state(&mut br)?;
        while br.bits_remaining() > -(scratch.table.max_num_bits as isize) {
            target.push(decoder.decode_symbol());
            decoder.next_state(&mut br)?;
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
