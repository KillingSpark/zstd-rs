/// Huffman coding is a method of encoding where symbols are assigned a code,
/// and more commonly used symbols get shorter codes, and less commonly
/// used symbols get longer codes. Codes are prefix free, meaning no two codes
/// will start with the same sequence of bits.
mod huff0_decoder;
use alloc::vec::Vec;

pub use huff0_decoder::*;

use crate::decoding::bit_reader_reverse::BitReaderReversed;
mod huff0_encoder;

pub fn round_trip(data: &[u8]) {
    let encoder_table = huff0_encoder::HuffmanTable::build_from_data(data);
    let mut encoder = huff0_encoder::HuffmanEncoder::new(encoder_table);

    encoder.encode(data);
    let encoded = encoder.dump();
    let decoder_table = HuffmanTable::from_weights(encoder.weights());
    let mut decoder = HuffmanDecoder::new(&decoder_table);

    let mut br = BitReaderReversed::new(&encoded);
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

    decoder.init_state(&mut br);
    let mut decoded = Vec::new();
    while br.bits_remaining() > -(decoder_table.max_num_bits as isize) {
        decoded.push(decoder.decode_symbol());
        decoder.next_state(&mut br);
    }
    assert_eq!(&decoded, data);
}

#[test]
fn roundtrip() {
    round_trip(&[1, 1, 1, 1, 2, 3]);
}
