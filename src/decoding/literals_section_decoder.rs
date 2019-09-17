use super::bit_reader_reverse::BitReaderReversed;
use super::huff0::HuffmanDecoder;
use super::scratch::HuffmanScratch;
use super::super::block::literals_section::LiteralsSection;
use super::super::block::literals_section::LiteralsSectionType;

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
) -> Result<u32, String> {
    if section.compressed_size.is_none() {
        return Err("compressed size was none even though it must be set to something for compressed literals".to_owned());
    }
    if section.num_streams.is_none() {
        return Err("num_streams was none even though it must be set to something (1 or 4) for compressed literals".to_owned());
    }

    target.reserve(section.regenerated_size as usize);
    let source = &source[0..section.compressed_size.unwrap() as usize];
    let mut bytes_read = 0;

    match section.ls_type {
        LiteralsSectionType::Compressed => {
            //read Huffman tree description
            bytes_read += scratch.table.build_decoder(source)?;
            if crate::VERBOSE {
                println!("Built huffman table using {} bytes", bytes_read);
            }
        }
        LiteralsSectionType::Treeless => {
            if scratch.table.max_num_bits == 0 {
                return Err("Tried to reuse huffman table but it was never initialized".to_owned());
            }
        }
        _ => { /* nothing to do, huffman tree has been provided by previous block */ }
    }

    let source = &source[bytes_read as usize..];

    if section.num_streams.unwrap() == 4 {
        //build jumptable
        if source.len() < 6 {
            return Err("Need 6 byte to decode jump header".to_owned());
        }
        let jump1 = source[0] as usize + ((source[1] as usize) << 8);
        let jump2 = jump1 + source[2] as usize + ((source[3] as usize) << 8);
        let jump3 = jump2 + source[4] as usize + ((source[5] as usize) << 8);
        bytes_read += 6;
        let source = &source[6..];

        if source.len() < jump3 as usize {
            return Err(format!(
                "Need at least {} byte to decode literals. Have: {}",
                jump3,
                source.len()
            ));
        }

        //decode 4 streams
        let stream1 = &source[..jump1 as usize];
        let stream2 = &source[jump1 as usize..jump2 as usize];
        let stream3 = &source[jump2 as usize..jump3 as usize];
        let stream4 = &source[jump3 as usize..];

        let streams: [&[u8]; 4] = [stream1, stream2, stream3, stream4];

        for i in 0..4 {
            let stream = &streams[i];
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
                return Err(format!("Padding at the end of the sequence_section was more than a byte long: {}. Probably cause by data corruption", skipped_bits));
            }
            decoder.init_state(&mut br)?;

            while br.bits_remaining() > -(scratch.table.max_num_bits as isize) {
                target.push(decoder.decode_symbol());
                decoder.next_state(&mut br)?;
            }
            if br.bits_remaining() != -(scratch.table.max_num_bits as isize) {
                return Err(format!(
                    "Bitstream was read till: {}, should have been: {}",
                    br.bits_remaining(),
                    -(scratch.table.max_num_bits as isize)
                ));
            }
        }

        bytes_read += source.len() as u32;
    } else {
        //just decode the one stream
        assert!(section.num_streams.unwrap() == 1);
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
            return Err(format!("Padding at the end of the sequence_section was more than a byte long: {}. Probably cause by data corruption", skipped_bits));
        }
        decoder.init_state(&mut br)?;
        while br.bits_remaining() > -(scratch.table.max_num_bits as isize) {
            target.push(decoder.decode_symbol());
            decoder.next_state(&mut br)?;
        }
        bytes_read += source.len() as u32;
    }

    Ok(bytes_read)
}
