use alloc::vec::Vec;
use core::cmp::Ordering;

pub struct HuffmanTable {
    /// Index is the symbol, values are the bitstring in the lower bits of the u32 and the amount of bits in the u8
    codes: Vec<(u32, u8)>,
}

impl HuffmanTable {
    pub fn build(weights: &[usize]) -> Self {
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

        let mut current_weight = sorted.last().unwrap().weight;
        let mut current_num_bits = max_num_bits + 1 - current_weight;
        let mut code = (1 << current_num_bits) - 1;
        for idx in (0..sorted.len()).rev() {
            if current_weight != sorted[idx].weight {
                current_weight = sorted[idx].weight;
                let next_num_bits = max_num_bits + 1 - current_weight;
                code = (1 << (next_num_bits - current_num_bits)) - 1;
                current_num_bits = next_num_bits;
            }
            table.codes[sorted[idx].symbol as usize] = (code, current_num_bits as u8);
            code = code.saturating_sub(1);
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
    let table = HuffmanTable::build(&[2, 2, 2, 1, 1]);
    assert_eq!(table.codes[0], (1, 2));
    assert_eq!(table.codes[1], (2, 2));
    assert_eq!(table.codes[2], (3, 2));
    assert_eq!(table.codes[3], (0, 3));
    assert_eq!(table.codes[4], (1, 3));

    let table = HuffmanTable::build(&[4, 3, 2, 0, 1, 1]);
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

fn redistribute_weights(weights: &mut [usize], max_weight: usize) {
    let mut added_weights = 0;
    for weight in weights.iter_mut() {
        if *weight < max_weight {
            for add in *weight..max_weight {
                added_weights += 1 << add;
            }
            *weight += max_weight - *weight;
        }
    }
    while added_weights > 0 {
        let downgrade = weights
            .iter_mut()
            .find(|weight| (1 << **weight) > added_weights);
        let downgrade = if let Some(dowgrade) = downgrade {
            dowgrade
        } else {
            let max = *weights.last_mut().unwrap();
            weights.iter_mut().find(|weight| **weight == max).unwrap()
        };
        *downgrade -= 1;
        added_weights -= 1 << *downgrade;
    }
    if added_weights < 0 {
        panic!("Overshot while redistributing, need to compensate {} {weights:?}", added_weights);
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

        redistribute_weights(&mut weights, amount.ilog2() as usize + 1);
        let sum = weights
            .iter()
            .copied()
            .map(|weight| 1 << weight)
            .sum::<usize>();
        assert!(sum.is_power_of_two());
        assert!(weights.last().unwrap() - weights.first().unwrap() <= amount.ilog2() as usize + 1)
    }
}
