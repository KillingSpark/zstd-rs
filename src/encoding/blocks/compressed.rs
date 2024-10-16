use alloc::vec::Vec;

use crate::{encoding::bit_writer::BitWriter, huff0::huff0_encoder};

pub fn compress_block(data: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter::new();
    compress_literals(data, &mut writer);
    writer.dump()
}

fn compress_literals(literals: &[u8], writer: &mut BitWriter) {
    writer.write_bits(2u8, 2); // compressed bock type

    let encoder_table = huff0_encoder::HuffmanTable::build_from_data(literals);
    let mut encoder = huff0_encoder::HuffmanEncoder::new(encoder_table);
    let encoded = encoder.encode4x(literals);

    let (size_format, size_bits) = match literals.len() {
        0..6 => unimplemented!("should probably just be a raw block"),
        6..1024 => (0b01u8, 10),
        1024..16384 => (0b10, 14),
        16384..262144 => (0b11, 18),
        _ => unimplemented!("too many literals"),
    };
    writer.write_bits(size_format, 2);
    writer.write_bits(literals.len() as u32, size_bits);
    writer.write_bits(encoded.len() as u32, size_bits);
    writer.append_bytes(&encoded);

    //sequences
    writer.write_bits(0u8, 8);
    writer.write_bits(0u8, 8);
}
