//! This module contains the decompress_literals function, used to take a
//! parsed literals header and a source and decompress it.

use super::super::blocks::literals_section::{LiteralsSection, LiteralsSectionType};
use super::scratch::HuffmanScratch;
use crate::bit_io::BitReaderReversed;
use crate::decoding::errors::DecompressLiteralsError;
use crate::huff0::HuffmanDecoder;
use alloc::vec::Vec;

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
        LiteralsSectionType::Treeless if scratch.table.max_num_bits == 0 => {
            return Err(err::UninitializedHuffmanTable);
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
                // Refuse the next symbol before push can grow beyond the declared output.
                if target.len() >= section.regenerated_size as usize {
                    return Err(err::DecodedLiteralCountMismatch {
                        decoded: target.len().checked_add(1).unwrap_or(usize::MAX),
                        expected: section.regenerated_size as usize,
                    });
                }
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
            // Refuse the next symbol before push can grow beyond the declared output.
            if target.len() >= section.regenerated_size as usize {
                return Err(err::DecodedLiteralCountMismatch {
                    decoded: target.len().checked_add(1).unwrap_or(usize::MAX),
                    expected: section.regenerated_size as usize,
                });
            }
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

#[cfg(test)]
mod literal10_tests {
    use super::{decode_literals, HuffmanScratch, LiteralsSection, LiteralsSectionType};
    use crate::decoding::errors::DecompressLiteralsError;
    use alloc::{vec, vec::Vec};
    use core::convert::TryFrom;

    fn stream(symbols: usize) -> Vec<u8> {
        // Mechanism fixture: zero data bits and a terminating one for a two-symbol
        // one-bit table. These bytes are deliberately synthetic, not libzstd output.
        let mut bytes = vec![0; symbols / 8 + 1];
        *bytes.last_mut().unwrap() = 1 << (symbols % 8);
        bytes
    }

    fn payload(counts: &[usize], treeless: bool) -> Vec<u8> {
        let mut bytes = if treeless {
            Vec::new()
        } else {
            vec![128, 0x10]
        };
        if counts.len() == 4 {
            for count in &counts[..3] {
                let size = u16::try_from(count / 8 + 1).unwrap();
                bytes.extend_from_slice(&size.to_le_bytes());
            }
        }
        for count in counts {
            bytes.extend_from_slice(&stream(*count));
        }
        bytes
    }

    fn section(size: u32, bytes: usize, streams: u8, treeless: bool) -> LiteralsSection {
        LiteralsSection {
            regenerated_size: size,
            compressed_size: Some(u32::try_from(bytes).unwrap()),
            num_streams: Some(streams),
            ls_type: if treeless {
                LiteralsSectionType::Treeless
            } else {
                LiteralsSectionType::Compressed
            },
        }
    }

    fn scratch(treeless: bool) -> HuffmanScratch {
        let mut scratch = HuffmanScratch::new();
        if treeless {
            // Direct table seed is a mechanism control, not a genuine prior block.
            assert_eq!(scratch.table.build_decoder(&[128, 0x10]).unwrap(), 2);
        }
        scratch
    }

    #[test]
    fn excess_symbol_is_refused_before_literal_vector_growth() {
        for treeless in [false, true] {
            // Refusal in stream1, on a later stream boundary, and inside stream4.
            for counts in [
                &[1032usize][..],
                &[258, 258, 258, 258][..],
                &[0, 0, 0, 1032][..],
            ] {
                let bytes = payload(counts, treeless);
                for declared in [0u32, 1, 7, 8, 1024] {
                    let mut target = Vec::with_capacity(declared as usize);
                    let capacity = target.capacity();
                    let section = section(declared, bytes.len(), counts.len() as u8, treeless);
                    let error =
                        decode_literals(&section, &mut scratch(treeless), &bytes, &mut target)
                            .unwrap_err();
                    assert!(
                        matches!(error, DecompressLiteralsError::DecodedLiteralCountMismatch {
                        decoded, expected
                    } if decoded == declared as usize + 1 && expected == declared as usize)
                    );
                    assert_eq!(target.len(), declared as usize);
                    assert_eq!(
                        target.capacity(),
                        capacity,
                        "literal allocation grew before refusal"
                    );
                }
            }
        }
    }

    #[test]
    fn exact_and_short_output_keep_existing_count_semantics() {
        for treeless in [false, true] {
            for counts in [
                &[0usize][..],
                &[1][..],
                &[1024][..],
                &[0, 0, 0, 0][..],
                &[1, 1, 1, 1][..],
                &[256, 256, 256, 256][..],
            ] {
                let bytes = payload(counts, treeless);
                let total: usize = counts.iter().sum();
                let mut target = Vec::new();
                let exact = section(total as u32, bytes.len(), counts.len() as u8, treeless);
                assert_eq!(
                    decode_literals(&exact, &mut scratch(treeless), &bytes, &mut target).unwrap()
                        as usize,
                    bytes.len()
                );
                assert_eq!(target.len(), total);
                target.clear();
                let short = section(total as u32 + 1, bytes.len(), counts.len() as u8, treeless);
                assert!(
                    matches!(decode_literals(&short, &mut scratch(treeless), &bytes, &mut target),
                    Err(DecompressLiteralsError::DecodedLiteralCountMismatch { decoded, expected })
                    if decoded == total && expected == total + 1)
                );
                assert_eq!(target.len(), total);
            }
        }
    }
}
