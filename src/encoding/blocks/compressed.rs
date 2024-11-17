use std::dbg;

use alloc::vec::Vec;

use crate::{
    encoding::{
        bit_writer::BitWriter,
        match_generator::{MatchGenerator, Sequence},
    },
    huff0::huff0_encoder,
};

pub fn compress_block<'a>(matcher: &mut MatchGenerator<'a>, data: &'a [u8], output: &mut Vec<u8>) {
    matcher.add_data(data);
    let mut literals_vec = Vec::new();
    let mut sequences = Vec::new();
    loop {
        let Some(seq) = matcher.next_sequence() else {
            break;
        };

        match seq {
            Sequence::Literals { literals } => literals_vec.extend_from_slice(literals),
            Sequence::Triple {
                literals,
                offset,
                match_len,
            } => {
                literals_vec.extend_from_slice(literals);
                sequences.push(crate::blocks::sequence_section::Sequence {
                    ll: literals.len() as u32,
                    ml: match_len as u32,
                    of: offset as u32,
                });
            }
        }
    }

    let mut writer = BitWriter::from(output);
    if literals_vec.len() > 1024 {
        compress_literals(&literals_vec, &mut writer);
    } else {
        raw_literals(&literals_vec, &mut writer);
    }
    //sequences

    if sequences.is_empty() {
        writer.write_bits(0u8, 8);
    } else {
        encode_seqnum(sequences.len(), &mut writer);

        // use standard FSE tables
        writer.write_bits(0u8, 8);

        writer.flush();
    }
}

fn encode_seqnum(seqnum: usize, writer: &mut BitWriter<impl AsMut<Vec<u8>>>) {
    const UPPER_LIMIT: usize = 0xFFFF + 0x7F00;
        match seqnum {
            1..=127 => writer.write_bits(seqnum as u32, 8),
            128..=0x7FFF => {
                let upper = ((seqnum >> 8) & 0x80) as u8;
                let lower = seqnum as u8;
                writer.write_bits(upper, 8);
                writer.write_bits(lower, 8);
            }
            0x8000..=UPPER_LIMIT => {
                let encode = seqnum - 0x7F00;
                let upper = (encode >> 8) as u8;
                let lower = encode as u8;
                writer.write_bits(255u8, 8);
                writer.write_bits(upper, 8);
                writer.write_bits(lower, 8);
            }
            _ => unreachable!()
        }
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
