use super::literals_section::LiteralsSection;
use super::literals_section::LiteralsSectionType;
use super::super::decoding::scratch::HuffmanScratch;

pub fn decode_literals(section: &LiteralsSection, scratch: &mut HuffmanScratch, source: &[u8], target: &mut Vec<u8>) -> Result<u32, String> {
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
            let mut bytes_read = 0;

            //TODO
            match section.ls_type {
                LiteralsSectionType::Compressed => {
                    //read Huffman tree description
                    bytes_read += scratch.decoder.build_decoder(source)?;
                }
                _ => { /* nothing to do, huffman tree has been provided by previous block */ }
            }

            if section.num_streams.unwrap() == 4 {
                //build jumptable
                //decode 4 streams
            } else {
                assert!(section.num_streams.unwrap() == 1);
                //decode one stream
            }

            //return sum of used bytes
            Ok(bytes_read)
        }
    }
}
