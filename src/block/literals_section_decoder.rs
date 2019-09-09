use super::super::decoding::bit_reader::BitReader;
use super::super::decoding::huff0::HuffmanDecoder;
use super::super::decoding::scratch::HuffmanScratch;
use super::literals_section::LiteralsSection;
use super::literals_section::LiteralsSectionType;

pub fn decode_literals(
    section: &LiteralsSection,
    scratch: &mut HuffmanScratch,
    source: &[u8],
    target: &mut Vec<u8>,
) -> Result<u32, String> {
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
            target.reserve(section.regenerated_size as usize);

            let source = &source[0..section.compressed_size.unwrap() as usize];
            let mut bytes_read = 0;

            match section.ls_type {
                LiteralsSectionType::Compressed => {
                    //read Huffman tree description
                    bytes_read += scratch.table.build_decoder(source)?;
                }
                _ => { /* nothing to do, huffman tree has been provided by previous block */ }
            }

            let source = &source[bytes_read as usize..];

            if section.num_streams.unwrap() == 4 {
                //build jumptable
                let jump1 = source[0] as u16 + ((source[1] as u16) << 8);
                let jump2 = source[2] as u16 + ((source[3] as u16) << 8);
                let jump3 = source[4] as u16 + ((source[5] as u16) << 8);
                bytes_read += 6;
                let source = &source[6..];

                //decode 4 streams
                let stream1 = &source[..jump1 as usize];
                let stream2 = &source[jump1 as usize..jump2 as usize];
                let stream3 = &source[jump2 as usize..jump3 as usize];
                let stream4 = &source[jump3 as usize..];

                let streams: [&[u8]; 4] = [stream1, stream2, stream3, stream4];

                for stream in &streams[..] {
                    let mut br = BitReader::new(stream);
                    let mut decoder = HuffmanDecoder::new(&scratch.table);
                    while br.bits_left() != 0 {
                        decoder.next_state(&mut br)?;
                        target.push(decoder.decode_symbol());
                    }
                }

                bytes_read += source.len() as u32;
            } else {
                //just decode the one stream
                assert!(section.num_streams.unwrap() == 1);
                let mut br = BitReader::new(source);
                let mut decoder = HuffmanDecoder::new(&scratch.table);
                while br.bits_left() != 0 {
                    decoder.next_state(&mut br)?;
                    target.push(decoder.decode_symbol());
                }
            }

            //return sum of used bytes
            Ok(bytes_read)
        }
    }
}
