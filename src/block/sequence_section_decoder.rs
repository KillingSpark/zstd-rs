use super::super::decoding::bit_reader_reverse::BitReaderReversed;
use super::super::decoding::fse::FSEDecoder;
use super::super::decoding::scratch::FSEScratch;
use super::sequence_section::ModeType;
use super::sequence_section::Sequence;
use super::sequence_section::SequencesHeader;

pub fn decode_sequences(
    section: &SequencesHeader,
    source: &[u8],
    scratch: &mut FSEScratch,
    target: &mut Vec<Sequence>,
) -> Result<(), String> {
    let (bytes_read, ll_rle_byte, of_rle_byte, ml_rle_byte) =
        maybe_update_fse_tables(section, source, scratch)?;

    println!("Updating tables used {} bytes", bytes_read);

    if ll_rle_byte.is_some() || of_rle_byte.is_some() || ml_rle_byte.is_some() {
        //TODO
        unimplemented!("RLE symbols for sequences not yet implemented");
    }

    let bit_stream = &source[bytes_read..];

    let mut br = BitReaderReversed::new(bit_stream);

    //skip the 0 padding at the end of the last byte of the bit stream and throw away the first 1 found
    let mut skipped_bits = 0;
    loop {
        let val = br.get_bits(1)?;
        skipped_bits += 1;
        if val == 1 {
            break;
        }
    }
    if skipped_bits > 8 {
        //if more than 7 bits are 0, this is not the correct end of the bitstream. Either a bug or corrupted data
        return Err(format!("Padding at the end of the sequence_section was more than a byte long: {}. Probably cause by data corruption", skipped_bits));
    }

    let mut ll_dec = FSEDecoder::new(&scratch.literal_lengths);
    let mut ml_dec = FSEDecoder::new(&scratch.match_lengths);
    let mut of_dec = FSEDecoder::new(&scratch.offsets);

    ll_dec.init_state(&mut br)?;
    of_dec.init_state(&mut br)?;
    ml_dec.init_state(&mut br)?;

    target.clear();
    target.reserve(section.num_sequences as usize);

    for i in 0..section.num_sequences {
        let ll_code = ll_dec.decode_symbol();
        let ml_code = ml_dec.decode_symbol();
        let of_code = of_dec.decode_symbol();
        let (ll_value, ll_num_bits) = lookup_ll_code(ll_code);
        let (ml_value, ml_num_bits) = lookup_ml_code(ml_code);

        //println!("of stat: {}", of_dec.state);
        //println!("of Code: {}", of_code);
        //println!("ll stat: {}", ll_dec.state);
        //println!("ll bits: {}", ll_num_bits);
        //println!("ll Code: {}", ll_value);
        //println!("ml stat: {}", ml_dec.state);
        //println!("ml bits: {}", ml_num_bits);
        //println!("ml Code: {}", ml_value);
        //println!("");

        let offset = br.get_bits(of_code as usize)? + (1 << of_code);
        let ml_add = br.get_bits(ml_num_bits as usize)?;
        let ll_add = br.get_bits(ll_num_bits as usize)?;

        let offset_value = if offset > 3 {
            offset as u32 - 3
        } else {
            offset as u32
        };
        target.push(Sequence {
            ll: ll_value as u32 + ll_add as u32,
            ml: ml_value as u32 + ml_add as u32,
            of: offset_value,
        });

        if target.len() < section.num_sequences as usize {
            //println!(
            //    "Bits left: {} ({} bytes)",
            //    br.bits_remaining(),
            //    br.bits_remaining() / 8,
            //);
            ll_dec.update_state(&mut br)?;
            ml_dec.update_state(&mut br)?;
            of_dec.update_state(&mut br)?;
        }
    }

    if br.bits_remaining() > 0 {
        Err(format!(
            "Did not use full bitstream. Bits left: {} ({} bytes)",
            br.bits_remaining(),
            br.bits_remaining() / 8,
        ))
    } else {
        Ok(())
    }
}

fn lookup_ll_code(code: u8) -> (u32, u8) {
    match code {
        0...15 => (code as u32, 0),
        16 => (16, 1),
        17 => (18, 1),
        18 => (20, 1),
        19 => (22, 1),
        20 => (24, 2),
        21 => (28, 2),
        22 => (32, 3),
        23 => (40, 3),
        24 => (48, 4),
        25 => (64, 6),
        26 => (128, 7),
        27 => (256, 8),
        28 => (512, 9),
        29 => (1024, 10),
        30 => (2048, 11),
        31 => (4069, 12),
        32 => (8192, 13),
        33 => (16384, 14),
        34 => (32768, 15),
        35 => (65536, 16),
        _ => panic!(format!("Invalid ll code: {}", code)),
    }
}

fn lookup_ml_code(code: u8) -> (u32, u8) {
    match code {
        0...31 => (code as u32 + 3, 0),
        32 => (35, 1),
        33 => (37, 1),
        34 => (39, 1),
        35 => (41, 1),
        36 => (43, 2),
        37 => (47, 2),
        38 => (51, 3),
        39 => (59, 3),
        40 => (67, 4),
        41 => (83, 4),
        42 => (99, 5),
        43 => (131, 7),
        44 => (259, 8),
        45 => (515, 9),
        46 => (1027, 10),
        47 => (2051, 11),
        48 => (4099, 12),
        49 => (8195, 13),
        50 => (16387, 14),
        51 => (32771, 15),
        52 => (65539, 16),
        _ => panic!(format!("Invalid ml code: {}", code)),
    }
}

const LL_MAX_LOG: u8 = 9;
const ML_MAX_LOG: u8 = 9;
const OF_MAX_LOG: u8 = 8;

fn maybe_update_fse_tables(
    section: &SequencesHeader,
    source: &[u8],
    scratch: &mut FSEScratch,
) -> Result<(usize, Option<u8>, Option<u8>, Option<u8>), String> {
    let modes = section.modes.unwrap();
    let mut bytes_read = 0;

    let ll_rle_byte = match modes.ll_mode() {
        ModeType::FSECompressed => {
            println!("Updating ll table");
            let size = scratch.literal_lengths.build_decoder(source, LL_MAX_LOG)?;
            bytes_read += size;
            println!("LL Size: {}", size);
            println!("LL Acc_Log: {}", scratch.literal_lengths.accuracy_log);
            
            None
        }
        ModeType::RLE => {
            bytes_read += 1;
            Some(source[0])
        }
        ModeType::Predefined => unimplemented!("Predefined tables not yet implemented"),
        ModeType::Repeat => {
            /* Nothing to do */
            None
        }
    };

    let of_source = &source[bytes_read..];

    let of_rle_byte = match modes.of_mode() {
        ModeType::FSECompressed => {
            println!("Updating of table");
            bytes_read += scratch.offsets.build_decoder(of_source, OF_MAX_LOG)?;
            None
        }
        ModeType::RLE => {
            bytes_read += 1;
            Some(of_source[0])
        }
        ModeType::Predefined => unimplemented!("Predefined tables not yet implemented"),
        ModeType::Repeat => {
            /* Nothing to do */
            None
        }
    };

    let ml_source = &source[bytes_read..];

    let ml_rle_byte = match modes.ml_mode() {
        ModeType::FSECompressed => {
            println!("Updating ml table");
            bytes_read += scratch.match_lengths.build_decoder(ml_source, ML_MAX_LOG)?;
            None
        }
        ModeType::RLE => {
            bytes_read += 1;
            Some(ml_source[0])
        }
        ModeType::Predefined => unimplemented!("Predefined tables not yet implemented"),
        ModeType::Repeat => {
            /* Nothing to do */
            None
        }
    };

    Ok((bytes_read, ll_rle_byte, of_rle_byte, ml_rle_byte))
}
