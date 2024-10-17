use alloc::vec::Vec;

use crate::{encoding::bit_writer::BitWriter, huff0::huff0_encoder};

pub fn compress_block(data: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter::new();
    compress_literals(data, &mut writer);
    //raw_literals(data, &mut writer);
    writer.dump()
}

// TODO find usecase fot this
#[allow(dead_code)]
fn raw_literals(literals: &[u8], writer: &mut BitWriter) {
    writer.write_bits(0u8, 2);
    writer.write_bits(0b11u8, 2);
    writer.write_bits(literals.len() as u32, 20);
    writer.append_bytes(literals);

    //sequences
    writer.write_bits(0u8, 8);
}

fn compress_literals(literals: &[u8], writer: &mut BitWriter) {
    writer.write_bits(2u8, 2); // compressed bock type

    let encoder_table = huff0_encoder::HuffmanTable::build_from_data(literals);
    let mut encoder = huff0_encoder::HuffmanEncoder::new(encoder_table);

    let (size_format, size_bits) = match literals.len() {
        0..6 => (0b00u8, 10),
        6..1024 => (0b01, 10),
        1024..16384 => (0b10, 14),
        16384..262144 => (0b11, 18),
        _ => unimplemented!("too many literals"),
    };

    let encoded;
    if size_format == 0 {
        encoded = encoder.encode(literals);
    } else {
        encoded = encoder.encode4x(literals);
    }

    writer.write_bits(size_format, 2);
    writer.write_bits(literals.len() as u32, size_bits);
    writer.write_bits(encoded.len() as u32, size_bits);
    writer.append_bytes(&encoded);

    //sequences
    writer.write_bits(0u8, 8);
}
