//! FSE, short for Finite State Entropy, is an encoding technique
//! that assigns shorter codes to symbols that appear more frequently in data,
//! and longer codes to less frequent symbols.
//!
//! FSE works by mutating a state and using that state to index into a table.
//!
//! Zstandard uses two different kinds of entropy encoding: FSE, and Huffman coding.
//! Huffman is used to compress literals,
//! while FSE is used for all other symbols (literal length code, match length code, offset code).
//!
//! <https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#fse>
//!
//! <https://arxiv.org/pdf/1311.2540>

mod fse_decoder;

pub use fse_decoder::*;
use fse_encoder::FSEEncoder;

use crate::decoding::bit_reader_reverse::BitReaderReversed;
pub mod fse_encoder;

#[test]
fn tables_equal() {
    let probs = &[0, 0, -1, 3, 2, 2, (1 << 6) - 8];
    let mut dec_table = FSETable::new(255);
    dec_table.build_from_probabilities(6, probs).unwrap();
    let enc_table = fse_encoder::build_table_from_probabilities(probs, 6);

    check_tables(&dec_table, &enc_table);
}

fn check_tables(dec_table: &fse_decoder::FSETable, enc_table: &fse_encoder::FSETable) {
    for (idx, dec_state) in dec_table.decode.iter().enumerate() {
        let enc_states = &enc_table.states[dec_state.symbol as usize];
        let enc_state = enc_states
            .states
            .iter()
            .find(|state| state.index == idx)
            .unwrap();
        assert_eq!(enc_state.baseline, dec_state.base_line as usize);
        assert_eq!(enc_state.num_bits, dec_state.num_bits);
    }
}

#[test]
fn roundtrip() {
    round_trip(&(0..64).collect::<alloc::vec::Vec<_>>());
    let mut data = alloc::vec![];
    data.extend(0..32);
    data.extend(0..32);
    data.extend(0..32);
    data.extend(0..32);
    data.extend(0..32);
    data.extend(20..32);
    data.extend(20..32);
    data.extend(0..32);
    data.extend(20..32);
    data.extend(100..255);
    data.extend(20..32);
    data.extend(20..32);
    round_trip(&data);

    #[cfg(feature = "std")]
    if std::fs::exists("fuzz/artifacts/fse").unwrap_or(false) {
        for file in std::fs::read_dir("fuzz/artifacts/fse").unwrap() {
            if file.as_ref().unwrap().file_type().unwrap().is_file() {
                let data = std::fs::read(file.unwrap().path()).unwrap();
                round_trip(&data);
            }
        }
    }
}

pub fn round_trip(data: &[u8]) {
    if data.len() < 2 {
        return;
    }
    if data.iter().all(|x| *x == data[0]) {
        return;
    }
    if data.len() < 64 {
        return;
    }

    let mut encoder: FSEEncoder = FSEEncoder::new(fse_encoder::build_table_from_data(data, false));
    let mut dec_table = FSETable::new(255);

    let encoded = encoder.encode(data);

    let table_bytes = dec_table
        .build_decoder(&encoded, encoder.acc_log())
        .unwrap();
    let encoded = &encoded[table_bytes..];
    let mut decoder = FSEDecoder::new(&dec_table);

    check_tables(&dec_table, &encoder.table);

    let mut br = BitReaderReversed::new(encoded);
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
        panic!("Corrupted end marker");
    }
    decoder.init_state(&mut br).unwrap();
    let mut decoded = alloc::vec::Vec::new();

    for x in data {
        let w = decoder.decode_symbol();
        assert_eq!(w, *x);
        decoded.push(w);
        if decoded.len() < data.len() {
            decoder.update_state(&mut br);
        }
    }

    assert_eq!(&decoded, data);

    assert_eq!(br.bits_remaining(), 0);
}
