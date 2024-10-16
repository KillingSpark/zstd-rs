use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::{
    encoding::bit_writer::BitWriter,
    fse::fse_encoder::{self, FSEEncoder},
};

pub struct HuffmanEncoder {
    table: HuffmanTable,
    writer: BitWriter,
}

impl HuffmanEncoder {
    pub fn new(table: HuffmanTable) -> Self {
        Self {
            table,
            writer: BitWriter::new(),
        }
    }
    pub fn encode(&mut self, data: &[u8]) -> Vec<u8> {
        self.write_table();
        for symbol in data.iter().rev() {
            let (code, num_bits) = self.table.codes[*symbol as usize];
            self.writer.write_bits(code, num_bits as usize);
        }

        Self::encode_stream(&self.table, &mut self.writer, data);

        let mut writer = BitWriter::new();
        core::mem::swap(&mut self.writer, &mut writer);
        writer.dump()
    }
    pub fn encode4x(&mut self, data: &[u8]) -> Vec<u8> {
        assert!(data.len() >= 4);
        let split_size = (data.len() + 3) / 4;
        let src1 = &data[..split_size];
        let src2 = &data[split_size..split_size * 2];
        let src3 = &data[split_size * 2..split_size * 3];
        let src4 = &data[split_size * 3..];

        let mut writer = BitWriter::new();
        Self::encode_stream(&self.table, &mut writer, src1);
        let encoded1 = writer.dump();
        let mut writer = BitWriter::new();
        Self::encode_stream(&self.table, &mut writer, src2);
        let encoded2 = writer.dump();
        let mut writer = BitWriter::new();
        Self::encode_stream(&self.table, &mut writer, src3);
        let encoded3 = writer.dump();
        let mut writer = BitWriter::new();
        Self::encode_stream(&self.table, &mut writer, src4);
        let encoded4 = writer.dump();

        assert!(encoded1.len() as u16 <= u16::MAX);
        assert!(encoded2.len() as u16 <= u16::MAX);
        assert!(encoded3.len() as u16 <= u16::MAX);
        assert!(encoded4.len() as u16 <= u16::MAX);

        self.write_table();
        self.writer.write_bits(encoded1.len() as u16, 16);
        self.writer.write_bits(encoded2.len() as u16, 16);
        self.writer.write_bits(encoded3.len() as u16, 16);

        self.writer.append_bytes(&encoded1);
        self.writer.append_bytes(&encoded2);
        self.writer.append_bytes(&encoded3);
        self.writer.append_bytes(&encoded4);

        let mut writer = BitWriter::new();
        core::mem::swap(&mut self.writer, &mut writer);
        writer.dump()
    }

    fn encode_stream(table: &HuffmanTable, writer: &mut BitWriter, data: &[u8]) {
        for symbol in data.iter().rev() {
            let (code, num_bits) = table.codes[*symbol as usize];
            writer.write_bits(code, num_bits as usize);
        }

        let bits_to_fill = writer.misaligned();
        if bits_to_fill == 0 {
            writer.write_bits(1u32, 8);
        } else {
            writer.write_bits(1u32, bits_to_fill);
        }
    }

    pub(super) fn weights(&self) -> Vec<u8> {
        let max = self.table.codes.iter().map(|(_, nb)| nb).max().unwrap();
        let weights = self
            .table
            .codes
            .iter()
            .copied()
            .map(|(_, nb)| if nb == 0 { 0 } else { max - nb + 1 })
            .collect::<Vec<u8>>();

        weights
    }

    fn write_table(&mut self) {
        // TODO strategy for determining this?
        let weights = self.weights();
        let weights = &weights[..weights.len() - 1]; // dont encode last weight

        if weights.len() > 16 {
            // TODO share output vec between encoders
            // TODO assert that no 0 num_bit states are generated here
            let mut encoder = FSEEncoder::new(fse_encoder::build_table_from_data(&weights, true));
            let encoded = encoder.encode_interleaved(&weights);
            assert!(encoded.len() < 128);
            self.writer.write_bits(encoded.len() as u8, 8);
            self.writer.append_bytes(&encoded);
        } else {
            self.writer.write_bits(weights.len() as u8 + 127, 8);
            let pairs = weights.chunks_exact(2);
            let remainder = pairs.remainder();
            for pair in pairs.into_iter() {
                let weight1 = pair[0];
                let weight2 = pair[1];
                assert!(weight1 < 16);
                assert!(weight2 < 16);
                self.writer.write_bits(weight2, 4);
                self.writer.write_bits(weight1, 4);
            }
            if !remainder.is_empty() {
                let weight = remainder[0];
                assert!(weight < 16);
                self.writer.write_bits(weight << 4, 8);
            }
        }
    }
}

pub struct HuffmanTable {
    /// Index is the symbol, values are the bitstring in the lower bits of the u32 and the amount of bits in the u8
    codes: Vec<(u32, u8)>,
}

impl HuffmanTable {
    pub fn build_from_data(data: &[u8]) -> Self {
        let mut counts = [0; 256];
        let mut max = 0;
        for x in data {
            counts[*x as usize] += 1;
            max = max.max(*x);
        }

        Self::build_from_counts(&counts[..=max as usize])
    }

    pub fn build_from_counts(counts: &[usize]) -> Self {
        assert!(counts.len() <= 256);
        let zeros = counts.iter().filter(|x| **x == 0).count();
        let mut weights = distribute_weights(counts.len() - zeros);
        let limit = weights.len().ilog2() as usize + 2;
        redistribute_weights(&mut weights, limit);

        weights.reverse();
        let mut counts_sorted = counts.iter().enumerate().collect::<Vec<_>>();
        counts_sorted.sort_by(|(_, c1), (_, c2)| c1.cmp(c2));

        let mut weights_distributed = alloc::vec![0; counts.len()];
        for (idx, count) in counts_sorted {
            if *count == 0 {
                weights_distributed[idx] = 0;
            } else {
                weights_distributed[idx] = weights.pop().unwrap();
            }
        }

        Self::build_from_weights(&weights_distributed)
    }

    pub fn build_from_weights(weights: &[usize]) -> Self {
        let mut sorted = Vec::with_capacity(weights.len());
        struct SortEntry {
            symbol: u8,
            weight: usize,
        }
        for (symbol, weight) in weights.iter().copied().enumerate() {
            if weight > 0 {
                sorted.push(SortEntry {
                    symbol: symbol as u8,
                    weight,
                });
            }
        }
        sorted.sort_by(|left, right| match left.weight.cmp(&right.weight) {
            Ordering::Equal => left.symbol.cmp(&right.symbol),
            other => other,
        });

        let mut table = HuffmanTable {
            codes: Vec::with_capacity(weights.len()),
        };
        for _ in 0..weights.len() {
            table.codes.push((0, 0));
        }

        let weight_sum = sorted.iter().map(|e| 1 << (e.weight - 1)).sum::<usize>();
        if !weight_sum.is_power_of_two() {
            panic!("This is an internal error");
        }
        let max_num_bits = highest_bit_set(weight_sum) - 1; // this is a log_2 of a clean power of two

        let mut current_value = 0;
        let mut current_weight = 0;
        let mut current_num_bits = 0;
        for entry in sorted.iter() {
            if current_weight != entry.weight {
                current_value >>= entry.weight - current_weight;
                current_weight = entry.weight;
                current_num_bits = max_num_bits - entry.weight + 1;
            }
            table.codes[entry.symbol as usize] = (current_value as u32, current_num_bits as u8);
            current_value += 1;
        }

        table
    }
}

/// Assert that the provided value is greater than zero, and returns index of the first set bit
fn highest_bit_set(x: usize) -> usize {
    assert!(x > 0);
    usize::BITS as usize - x.leading_zeros() as usize
}

#[test]
fn huffman() {
    let table = HuffmanTable::build_from_weights(&[2, 2, 2, 1, 1]);
    assert_eq!(table.codes[0], (1, 2));
    assert_eq!(table.codes[1], (2, 2));
    assert_eq!(table.codes[2], (3, 2));
    assert_eq!(table.codes[3], (0, 3));
    assert_eq!(table.codes[4], (1, 3));

    let table = HuffmanTable::build_from_weights(&[4, 3, 2, 0, 1, 1]);
    assert_eq!(table.codes[0], (1, 1));
    assert_eq!(table.codes[1], (1, 2));
    assert_eq!(table.codes[2], (1, 3));
    assert_eq!(table.codes[3], (0, 0));
    assert_eq!(table.codes[4], (0, 4));
    assert_eq!(table.codes[5], (1, 4));
}

fn distribute_weights(amount: usize) -> Vec<usize> {
    assert!(amount >= 2);
    assert!(amount <= 256);
    let mut weights = Vec::new();
    let mut target_weight = 1;
    let mut weight_counter = 2;

    weights.push(1);
    weights.push(1);

    while weights.len() < amount {
        let mut add_new = 1 << (weight_counter - target_weight);
        let available_space = amount - weights.len();

        if add_new > available_space {
            target_weight = weight_counter;
            add_new = 1;
        }

        for _ in 0..add_new {
            weights.push(target_weight);
        }
        weight_counter += 1;
    }

    weights
}

fn redistribute_weights(weights: &mut [usize], max_num_bits: usize) {
    let weight_sum = weights
        .iter()
        .copied()
        .map(|x| 1 << x)
        .sum::<usize>()
        .ilog2() as usize;
    if weight_sum < max_num_bits {
        return;
    }
    let decrease_weights_by = weight_sum - max_num_bits + 1;
    let mut added_weights = 0;
    for weight in weights.iter_mut() {
        if *weight < decrease_weights_by {
            for add in *weight..decrease_weights_by {
                added_weights += 1 << add;
            }
            *weight += decrease_weights_by - *weight;
        }
    }

    while added_weights > 0 {
        let mut current_idx = 0;
        let mut current_weight = 0;
        for (idx, weight) in weights.iter().copied().enumerate() {
            if 1 << (weight - 1) > added_weights {
                break;
            }
            if weight > current_weight {
                current_weight = weight;
                current_idx = idx;
            }
        }

        added_weights -= 1 << (current_weight - 1);
        weights[current_idx] -= 1;
    }

    if weights[0] > 1 {
        let offset = weights[0] - 1;
        for weight in weights.iter_mut() {
            *weight -= offset;
        }
    }
}

#[test]
fn weights() {
    // assert_eq!(distribute_weights(5).as_slice(), &[1, 1, 2, 3, 4]);
    for amount in 2..=256 {
        let mut weights = distribute_weights(amount);
        assert_eq!(weights.len(), amount);
        let sum = weights
            .iter()
            .copied()
            .map(|weight| 1 << weight)
            .sum::<usize>();
        assert!(sum.is_power_of_two());

        for num_bit_limit in (amount.ilog2() as usize + 1)..=11 {
            redistribute_weights(&mut weights, num_bit_limit);
            let sum = weights
                .iter()
                .copied()
                .map(|weight| 1 << weight)
                .sum::<usize>();
            assert!(sum.is_power_of_two());
            assert!(
                sum.ilog2() <= 11,
                "Max bits too big: sum: {} {weights:?}",
                sum
            );

            let codes = HuffmanTable::build_from_weights(&weights).codes;
            for (code, num_bits) in codes.iter().copied() {
                for (code2, num_bits2) in codes.iter().copied() {
                    if num_bits == 0 || num_bits2 == 0 || (code, num_bits) == (code2, num_bits2) {
                        continue;
                    }
                    if num_bits <= num_bits2 {
                        let code2_shifted = code2 >> (num_bits2 - num_bits);
                        assert_ne!(
                            code, code2_shifted,
                            "{:b},{num_bits:} is prefix of {:b},{num_bits2:}",
                            code, code2
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn counts() {
    let counts = &[3, 0, 4, 1, 5];
    let table = HuffmanTable::build_from_counts(counts).codes;

    assert_eq!(table[1].1, 0);
    assert!(table[3].1 >= table[0].1);
    assert!(table[0].1 >= table[2].1);
    assert!(table[2].1 >= table[4].1);

    let counts = &[3, 0, 4, 0, 7, 2, 2, 2, 0, 2, 2, 1, 5];
    let table = HuffmanTable::build_from_counts(counts).codes;

    assert_eq!(table[1].1, 0);
    assert_eq!(table[3].1, 0);
    assert_eq!(table[8].1, 0);
    assert!(table[11].1 >= table[5].1);
    assert!(table[5].1 >= table[6].1);
    assert!(table[6].1 >= table[7].1);
    assert!(table[7].1 >= table[9].1);
    assert!(table[9].1 >= table[10].1);
    assert!(table[10].1 >= table[0].1);
    assert!(table[0].1 >= table[2].1);
    assert!(table[2].1 >= table[12].1);
    assert!(table[12].1 >= table[4].1);
}

#[test]
fn from_data() {
    let counts = &[3, 0, 4, 1, 5];
    let table = HuffmanTable::build_from_counts(counts).codes;

    let data = &[0, 2, 4, 4, 0, 3, 2, 2, 0, 2];
    let table2 = HuffmanTable::build_from_data(data).codes;

    assert_eq!(table, table2);
}
