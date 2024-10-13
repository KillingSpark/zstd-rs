use crate::encoding::bit_writer::BitWriter;
use core::{iter::from_fn, u8};
use std::vec::{self, Vec};

pub struct FSEEncoder {
    table: FSETable,
    writer: BitWriter,
}

impl FSEEncoder {
    pub fn new(table: FSETable) -> Self {
        FSEEncoder {
            table,
            writer: BitWriter::new(),
        }
    }

    pub fn encode(&mut self, data: &[u8]) -> Vec<u8> {
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

pub struct FSETable {
    /// Indexed by symbol
    states: [SymbolStates; 256],
    table_size: usize,
}

impl FSETable {
    fn next_state(&self, symbol: u8, idx: usize) -> &State {
        let states = &self.states[symbol as usize];
        states.get(idx)
    }
}

struct SymbolStates {
    /// Sorted by baseline
    states: Vec<State>,
}

impl SymbolStates {
    fn get(&self, idx: usize) -> &State {
        // TODO we can do better, we can determin the correct state from the index with a bit of math
        self.states
            .iter()
            .find(|state| state.contains(idx))
            .unwrap()
    }
}

struct State {
    num_bits: u8,
    baseline: usize,
    last_index: usize,
    /// Index of this state in the decoding table
    index: usize,
}

impl State {
    fn contains(&self, idx: usize) -> bool {
        self.baseline <= idx && self.last_index >= idx
    }
}

fn build_table_from_data(data: &[u8]) -> FSETable {
    let mut counts = [0; 256];
    for x in data {
        counts[*x as usize] += 1;
    }
    build_table_from_counts(&counts)
}

fn build_table_from_counts(counts: &[usize]) -> FSETable {
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

    build_table_from_probabilities(&probs, acc_log)
}

fn build_table_from_probabilities(probs: &[i32], acc_log: u8) -> FSETable {
    let mut states =
        core::array::from_fn::<SymbolStates, 256, _>(|_| SymbolStates { states: Vec::new() });

    let mut indexes_used = alloc::vec![false; 1 << acc_log];

    // distribute -1 symbols
    let mut idx = (1 << acc_log) - 1;    
    for (symbol, prob) in probs.iter().copied().filter(|prob| *prob == -1).enumerate() {
        states[symbol].states.push(State{
            num_bits: acc_log,
            baseline: 0,
            last_index: (1 << acc_log) - 1,
            index: idx,
        });
        indexes_used[idx] = true;
        idx -= 1;
    }

    // distribute other symbols
    let mut idx = 0;
    for (symbol, prob) in probs.iter().copied().enumerate() {
        if prob == 0 {
            continue;
        }
        let states = &mut states[symbol].states;
        let prob_log = (prob as u32).ilog2();
        let rounded_up = 1 << (prob_log + 1);
        let double_states = rounded_up - prob;
        let num_bits = acc_log - prob_log as u8;
        let mut baseline = 0;
        for state_idx in 0..prob {
            if state_idx < double_states {
                let num_bits = num_bits + 1;
                states.push(State{
                    num_bits: num_bits,
                    baseline,
                    last_index: baseline + ((1 << num_bits) - 1),
                    index: idx,
                });
                baseline += 1 << num_bits;
                indexes_used[idx] = true;
            } else {
                states.push(State{
                    num_bits,
                    baseline,
                    last_index: baseline + ((1 << num_bits) - 1),
                    index: idx,
                });
                baseline += 1 << num_bits;
                indexes_used[idx] = true;
            }
            
            while indexes_used[idx] {
                idx = next_position(idx, 1 << acc_log);                
            }
        }
    }

    FSETable {
        table_size: 1 << acc_log,
        states,
    }
}

//utility functions for building the decoding table from probabilities
/// Calculate the position of the next entry of the table given the current
/// position and size of the table.
fn next_position(mut p: usize, table_size: usize) -> usize {
    p += (table_size >> 1) + (table_size >> 3) + 3;
    p &= table_size - 1;
    p
}