use core::cmp::Ordering;
use std::eprintln;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

pub struct HuffmanTable {
    /// Index is the symbol, values are the bitstring in the lower bits of the u32 and the amount of bits in the u8
    codes: Vec<(u32, u8)>,
}

impl HuffmanTable {
    pub fn build(weights: &[usize], maxNumBits: usize) -> Self {
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

        let mut currentWeight = sorted.last().unwrap().weight;
        let mut currentNumBits = maxNumBits + 1 - currentWeight;
        let mut code = (1 << currentNumBits) - 1;
        for idx in (0..sorted.len()).rev() {
            if currentWeight != sorted[idx].weight {
                currentWeight = sorted[idx].weight;
                let nextNumBits = maxNumBits + 1 - currentWeight;
                code = (1 << (nextNumBits - currentNumBits)) - 1;
                currentNumBits = nextNumBits;
            }
            eprintln!("Symbol: {}", sorted[idx].symbol);
            table.codes[sorted[idx].symbol as usize] = (code, currentNumBits as u8);
            code = code.saturating_sub(1);
        }

        table
    }
}

#[test]
fn huffman() {
    let table = HuffmanTable::build(&[2, 2, 2, 1, 1], 3);
    assert_eq!(table.codes[0], (1, 2));
    assert_eq!(table.codes[1], (2, 2));
    assert_eq!(table.codes[2], (3, 2));
    assert_eq!(table.codes[3], (0, 3));
    assert_eq!(table.codes[4], (1, 3));

    let table = HuffmanTable::build(&[4, 3, 2, 0, 1, 1], 4);
    assert_eq!(table.codes[0], (1, 1));
    assert_eq!(table.codes[1], (1, 2));
    assert_eq!(table.codes[2], (1, 3));
    assert_eq!(table.codes[3], (0, 0));
    assert_eq!(table.codes[4], (0, 4));
    assert_eq!(table.codes[5], (1, 4));
}
