use crate::encoding::bit_writer::BitWriter;
use core::u8;
use std::vec::Vec;

use super::FSETable;

pub struct FSEEncoder {
    table: FSETable,
    writer: BitWriter,
}

impl FSEEncoder {
    pub fn new() -> Self {
        FSEEncoder {
            table: FSETable::new(u8::MAX),
            writer: BitWriter::new(),
        }
    }

    pub fn encode(&mut self, data: &[u8]) -> Vec<u8> {
        build_table_from_data(data, &mut self.table);

        // TODO encode

        let mut writer = BitWriter::new();
        core::mem::swap(&mut self.writer, &mut writer);
        let bits_to_fill = writer.misaligned();
        if bits_to_fill == 0 {
            writer.write_bits(1u32, 8);
        } else {
            writer.write_bits(1u32, bits_to_fill);
        }
        writer.dump()
    }
}

fn build_table_from_data(data: &[u8], table: &mut FSETable) {
    let mut counts = [0; 256];
    for x in data {
        counts[*x as usize] += 1;
    }
    build_table_from_counts(&counts, table);
}

fn build_table_from_counts(counts: &[usize], table: &mut FSETable) {
    let mut probs = [0; 256];
    let mut min_count = 0;
    for (idx, count) in counts.iter().copied().enumerate() {
        probs[idx] = count as i32;
        if count > 0 && (count < min_count || min_count == 0) {
            min_count = count;
        }
    }

    // shift all probabilities down so that the lowest are 1
    min_count -= 1;
    for prob in probs.iter_mut() {
        if *prob > 0 {
            *prob -= min_count as i32;
        }
    }

    // normalize probabilities to a 2^x
    let sum = probs.iter().sum::<i32>();
    assert!(sum > 0);
    let sum = sum as usize;
    let acc_log = sum.ilog2() as u8 + 1;
    assert!(acc_log < 22); // TODO implement logic to decrease some counts until this fits

    // just raise the maximum probability as much as possible
    // TODO is this optimal?
    let diff = (1 << acc_log) - sum;
    let max = probs.iter_mut().max().unwrap();
    *max += diff as i32;

    table.reset();
    table.build_from_probabilities(acc_log, &probs).unwrap();
}
