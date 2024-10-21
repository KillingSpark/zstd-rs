use alloc::vec::Vec;

use crate::{encoding::bit_writer::BitWriter, huff0::huff0_encoder};

pub fn compress_block(data: &[u8], output: &mut Vec<u8>) {
    let mut writer = BitWriter::from(output);
    compress_literals(data, &mut writer);
    //raw_literals(data, &mut writer);
    //sequences
    writer.write_bits(0u8, 8);
    writer.flush();
}

// TODO find usecase fot this
#[allow(dead_code)]
fn raw_literals(literals: &[u8], writer: &mut BitWriter<&mut Vec<u8>>) {
    writer.write_bits(0u8, 2);
    writer.write_bits(0b11u8, 2);
    writer.write_bits(literals.len() as u32, 20);
    writer.append_bytes(literals);
}

fn compress_literals(literals: &[u8], writer: &mut BitWriter<&mut Vec<u8>>) {
    writer.write_bits(2u8, 2); // compressed literals type

    let encoder_table = huff0_encoder::HuffmanTable::build_from_data(literals);

    let (size_format, size_bits) = match literals.len() {
        0..6 => (0b00u8, 10),
        6..1024 => (0b01, 10),
        1024..16384 => (0b10, 14),
        16384..262144 => (0b11, 18),
        _ => unimplemented!("too many literals"),
    };

    writer.write_bits(size_format, 2);
    writer.write_bits(literals.len() as u32, size_bits);
    let size_index = writer.index();
    writer.write_bits(0u32, size_bits);
    let index_before = writer.index();
    let mut encoder = huff0_encoder::HuffmanEncoder::new(encoder_table, writer);
    if size_format == 0 {
        encoder.encode(literals)
    } else {
        encoder.encode4x(literals)
    };
    let encoded_len = (writer.index() - index_before) / 8;
    writer.change_bits(size_index, encoded_len as u64, size_bits);
}
