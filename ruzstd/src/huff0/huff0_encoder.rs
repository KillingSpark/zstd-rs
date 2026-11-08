use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::{
    bit_io::BitWriter,
    fse::fse_encoder::{self, FSEEncoder},
};

pub(crate) struct HuffmanEncoder<'output, 'table, V: AsMut<Vec<u8>>> {
    table: &'table HuffmanTable,
    writer: &'output mut BitWriter<V>,
}

impl<V: AsMut<Vec<u8>>> HuffmanEncoder<'_, '_, V> {
    pub fn new<'o, 't>(
        table: &'t HuffmanTable,
        writer: &'o mut BitWriter<V>,
    ) -> HuffmanEncoder<'o, 't, V> {
        HuffmanEncoder { table, writer }
    }

    /// Encodes the data using the provided table
    /// Writes
    /// * Table description
    /// * Encoded data
    /// * Padding bits to fill up last byte
    pub fn encode(&mut self, data: &[u8], with_table: bool) {
        if with_table {
            self.write_table();
        }
        Self::encode_stream(self.table, self.writer, data);
    }

    /// Encodes the data using the provided table in 4 concatenated streams
    /// Writes
    /// * Table description
    /// * Jumptable
    /// * Encoded data in 4 streams, each padded to fill the last byte
    pub fn encode4x(&mut self, data: &[u8], with_table: bool) {
        assert!(data.len() >= 4);

        // Split data in 4 equally sized parts (the last one might be a bit smaller than the rest)
        let split_size = data.len().div_ceil(4);
        let src1 = &data[..split_size];
        let src2 = &data[split_size..split_size * 2];
        let src3 = &data[split_size * 2..split_size * 3];
        let src4 = &data[split_size * 3..];

        // Write table description
        if with_table {
            self.write_table();
        }

        // Reserve space for the jump table, will be changed later
        let size_idx = self.writer.index();
        self.writer.write_bits(0u16, 16);
        self.writer.write_bits(0u16, 16);
        self.writer.write_bits(0u16, 16);

        // Write the 4 streams, noting the sizes of the encoded streams
        let index_before = self.writer.index();
        Self::encode_stream(self.table, self.writer, src1);
        let size1 = (self.writer.index() - index_before) / 8;

        let index_before = self.writer.index();
        Self::encode_stream(self.table, self.writer, src2);
        let size2 = (self.writer.index() - index_before) / 8;

        let index_before = self.writer.index();
        Self::encode_stream(self.table, self.writer, src3);
        let size3 = (self.writer.index() - index_before) / 8;

        Self::encode_stream(self.table, self.writer, src4);

        // Sanity check, if this doesn't hold we produce a broken stream
        assert!(size1 <= u16::MAX as usize);
        assert!(size2 <= u16::MAX as usize);
        assert!(size3 <= u16::MAX as usize);

        // Update the jumptable with the real sizes
        self.writer.change_bits(size_idx, size1 as u16, 16);
        self.writer.change_bits(size_idx + 16, size2 as u16, 16);
        self.writer.change_bits(size_idx + 32, size3 as u16, 16);
    }

    /// Encode one stream and pad it to fill the last byte
    fn encode_stream<VV: AsMut<Vec<u8>>>(
        table: &HuffmanTable,
        writer: &mut BitWriter<VV>,
        data: &[u8],
    ) {
        for symbol in data.iter().rev() {
            let (code, num_bits) = table.codes[*symbol as usize];
            debug_assert!(num_bits > 0);
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
            let size_idx = self.writer.index();
            self.writer.write_bits(0u8, 8);
            let idx_before = self.writer.index();
            let mut encoder = FSEEncoder::new(
                fse_encoder::build_table_from_data(weights.iter().copied(), 6, true),
                self.writer,
            );
            encoder.encode_interleaved(weights);
            let encoded_len = (self.writer.index() - idx_before) / 8;
            assert!(encoded_len < 128);
            self.writer.change_bits(size_idx, encoded_len as u8, 8);
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

#[derive(Clone)]
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

    /// A table of optimal code lengths for `counts` (at most 11 bits).
    /// At least two symbols must occur.
    pub fn build_from_counts(counts: &[usize]) -> Self {
        assert!(counts.len() <= 256);
        let lengths = code_lengths(counts, MAX_NUM_BITS);
        let max_len = lengths.iter().copied().max().unwrap_or(0);
        let weights: Vec<usize> = lengths
            .iter()
            .map(|&len| if len == 0 { 0 } else { max_len + 1 - len })
            .collect();
        Self::build_from_weights(&weights)
    }

    pub fn build_from_weights(weights: &[usize]) -> Self {
        let mut sorted = Vec::with_capacity(weights.len());
        struct SortEntry {
            symbol: u8,
            weight: usize,
        }

        // TODO this doesn't need to be a temporary Vec, it could be done in a [_; 264]
        // only non-zero weights are interesting here
        for (symbol, weight) in weights.iter().copied().enumerate() {
            if weight > 0 {
                sorted.push(SortEntry {
                    symbol: symbol as u8,
                    weight,
                });
            }
        }
        // We process symbols ordered by weight and then ordered by symbol
        sorted.sort_by(|left, right| match left.weight.cmp(&right.weight) {
            Ordering::Equal => left.symbol.cmp(&right.symbol),
            other => other,
        });

        // Prepare huffman table with placeholders
        let mut table = HuffmanTable {
            codes: Vec::with_capacity(weights.len()),
        };
        for _ in 0..weights.len() {
            table.codes.push((0, 0));
        }

        // Determine the number of bits needed for codes with the lowest weight
        let weight_sum = sorted.iter().map(|e| 1 << (e.weight - 1)).sum::<usize>();
        if !weight_sum.is_power_of_two() {
            panic!("This is an internal error");
        }
        let max_num_bits = highest_bit_set(weight_sum) - 1; // this is a log_2 of a clean power of two

        // Starting at the symbols with the lowest weight we update the placeholders in the table
        let mut current_code = 0;
        let mut current_weight = 0;
        let mut current_num_bits = 0;
        for entry in sorted.iter() {
            // If the entry isn't the same weight as the last one we need to change a few things
            if current_weight != entry.weight {
                // The code shifts by the difference of the weights to allow for enough unique values
                current_code >>= entry.weight - current_weight;
                // Encoding a symbol of this weight will take less bits than the previous weight
                current_num_bits = max_num_bits - entry.weight + 1;
                // Run the next update when the weight changes again
                current_weight = entry.weight;
            }
            table.codes[entry.symbol as usize] = (current_code as u32, current_num_bits as u8);
            current_code += 1;
        }

        table
    }

    pub fn can_encode(&self, other: &Self) -> Option<usize> {
        if other.codes.len() > self.codes.len() {
            return None;
        }
        let mut sum = 0;
        for ((_, other_num_bits), (_, self_num_bits)) in other.codes.iter().zip(self.codes.iter()) {
            if *other_num_bits != 0 && *self_num_bits == 0 {
                return None;
            }
            sum += other_num_bits.abs_diff(*self_num_bits) as usize;
        }
        Some(sum)
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
/// Literals are coded with at most this many bits (RFC 8878, section 4.2.1).
const MAX_NUM_BITS: usize = 11;

/// Optimal prefix-code lengths for `counts`, none longer than `limit`.
/// Symbols with a zero count get length 0. At least two symbols must occur.
fn code_lengths(counts: &[usize], limit: usize) -> Vec<usize> {
    // Leaves in ascending (count, symbol) order.
    let mut leaves: Vec<(usize, usize)> = counts
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, count)| *count > 0)
        .map(|(symbol, count)| (count, symbol))
        .collect();
    leaves.sort_unstable();
    let n = leaves.len();
    assert!(n >= 2, "a Huffman table needs at least two symbols");
    assert!(n <= 1 << limit);
    let mut lengths = alloc::vec![0; counts.len()];
    let depths = huffman_depths(&leaves);
    let depths = if depths.iter().copied().max().unwrap_or(0) <= limit {
        depths
    } else {
        package_merge_depths(&leaves, limit)
    };
    for (&(_, symbol), depth) in leaves.iter().zip(depths) {
        lengths[symbol] = depth;
    }
    lengths
}

/// Huffman code lengths for leaves sorted by count (two-queue construction).
fn huffman_depths(leaves: &[(usize, usize)]) -> Vec<usize> {
    let n = leaves.len();
    let mut weight: Vec<u64> = leaves.iter().map(|&(count, _)| count as u64).collect();
    weight.resize(2 * n - 1, 0);
    let mut parent = alloc::vec![0usize; 2 * n - 1];
    let (mut next_leaf, mut next_node) = (0, n);
    for node in n..2 * n - 1 {
        let mut take = || {
            let leaf_first =
                next_leaf < n && (next_node >= node || weight[next_leaf] <= weight[next_node]);
            if leaf_first {
                next_leaf += 1;
                next_leaf - 1
            } else {
                next_node += 1;
                next_node - 1
            }
        };
        let (a, b) = (take(), take());
        weight[node] = weight[a] + weight[b];
        parent[a] = node;
        parent[b] = node;
    }
    let mut depth = alloc::vec![0usize; 2 * n - 1];
    for node in (0..2 * n - 2).rev() {
        depth[node] = depth[parent[node]] + 1;
    }
    depth.truncate(n);
    depth
}

/// Length-limited code lengths (package-merge) for leaves sorted by count.
fn package_merge_depths(leaves: &[(usize, usize)], limit: usize) -> Vec<usize> {
    let n = leaves.len();
    // An item is a weight and how often each leaf occurs in it.
    let singles: Vec<(u64, Vec<u16>)> = leaves
        .iter()
        .enumerate()
        .map(|(index, &(count, _))| {
            let mut members = alloc::vec![0u16; n];
            members[index] = 1;
            (count as u64, members)
        })
        .collect();
    let mut list = singles.clone();
    for _ in 1..limit {
        let packages: Vec<(u64, Vec<u16>)> = list
            .chunks_exact(2)
            .map(|pair| {
                let members = pair[0]
                    .1
                    .iter()
                    .zip(&pair[1].1)
                    .map(|(a, b)| a + b)
                    .collect();
                (pair[0].0 + pair[1].0, members)
            })
            .collect();
        let mut merged = Vec::with_capacity(singles.len() + packages.len());
        let (mut s, mut p) = (singles.iter().peekable(), packages.into_iter().peekable());
        loop {
            let leaf_first = match (s.peek(), p.peek()) {
                (Some(leaf), Some(package)) => leaf.0 <= package.0,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if leaf_first {
                merged.push(s.next().unwrap().clone());
            } else {
                merged.push(p.next().unwrap());
            }
        }
        list = merged;
    }
    let mut depths = alloc::vec![0usize; n];
    for (_, members) in list.iter().take(2 * n - 2) {
        for (depth, &count) in depths.iter_mut().zip(members) {
            *depth += count as usize;
        }
    }
    depths
}

#[test]
fn code_lengths_are_complete_prefix_codes_within_the_limit() {
    // Skewed (Fibonacci-like) counts force the length limit.
    let mut cases: Vec<Vec<usize>> = Vec::new();
    let mut fibonacci = alloc::vec![1usize, 1];
    while fibonacci.len() < 40 {
        let next = fibonacci[fibonacci.len() - 1] + fibonacci[fibonacci.len() - 2];
        fibonacci.push(next);
    }
    cases.push(fibonacci);
    cases.push((1..=256).collect());
    cases.push(alloc::vec![1; 256]);
    cases.push(alloc::vec![5, 0, 0, 7]);
    let mut seed = 0x9E37_79B9u32;
    for _ in 0..50 {
        let len = 2 + (seed % 255) as usize;
        let counts = (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed % 1000) as usize >> (seed % 9)
            })
            .collect::<Vec<_>>();
        if counts.iter().filter(|c| **c > 0).count() >= 2 {
            cases.push(counts);
        }
    }
    for counts in cases {
        let lengths = code_lengths(&counts, MAX_NUM_BITS);
        let kraft: f64 = lengths
            .iter()
            .filter(|l| **l > 0)
            .map(|l| 1.0 / (1u64 << l) as f64)
            .sum();
        assert!((kraft - 1.0).abs() < 1e-9, "incomplete code {:?}", lengths);
        assert!(lengths.iter().all(|l| *l <= MAX_NUM_BITS));
        for (a, b) in counts.iter().zip(&lengths) {
            assert_eq!(*a == 0, *b == 0);
        }
        // Rarer symbols never get shorter codes.
        for (i, (ci, li)) in counts.iter().zip(&lengths).enumerate() {
            for (cj, lj) in counts.iter().zip(&lengths).skip(i + 1) {
                if *ci > 0 && *cj > 0 && ci < cj {
                    assert!(li >= lj);
                }
            }
        }
        let codes = HuffmanTable::build_from_counts(&counts).codes;
        for (code, num_bits) in codes.iter().copied() {
            for (code2, num_bits2) in codes.iter().copied() {
                if num_bits == 0 || num_bits2 == 0 || (code, num_bits) == (code2, num_bits2) {
                    continue;
                }
                if num_bits <= num_bits2 {
                    assert_ne!(code, code2 >> (num_bits2 - num_bits), "prefix collision");
                }
            }
        }
    }
}

/// Optimal lengths beat the old rank-based assignment on a skewed
/// distribution: total bits equal the Huffman optimum.
#[test]
fn code_lengths_are_optimal() {
    let counts = [100, 50, 25, 12, 6, 3, 2, 1, 1];
    let lengths = code_lengths(&counts, MAX_NUM_BITS);
    let bits: usize = counts.iter().zip(&lengths).map(|(c, l)| c * l).sum();
    assert_eq!(bits, 100 + 100 + 75 + 48 + 30 + 18 + 14 + 8 + 8);
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
    // The counts of `data` below.
    let counts = &[3, 0, 4, 1, 2];
    let table = HuffmanTable::build_from_counts(counts).codes;

    let data = &[0, 2, 4, 4, 0, 3, 2, 2, 0, 2];
    let table2 = HuffmanTable::build_from_data(data).codes;

    assert_eq!(table, table2);
}
