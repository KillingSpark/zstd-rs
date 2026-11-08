use crate::bit_io::BitWriter;
use alloc::vec::Vec;

pub(crate) struct FSEEncoder<'output, V: AsMut<Vec<u8>>> {
    pub(super) table: FSETable,
    writer: &'output mut BitWriter<V>,
}

impl<V: AsMut<Vec<u8>>> FSEEncoder<'_, V> {
    pub fn new(table: FSETable, writer: &mut BitWriter<V>) -> FSEEncoder<'_, V> {
        FSEEncoder { table, writer }
    }

    #[cfg(any(test, feature = "fuzz_exports"))]
    pub fn into_table(self) -> FSETable {
        self.table
    }

    /// Encodes the data using the provided table
    /// Writes
    /// * Table description
    /// * Encoded data
    /// * Last state index
    /// * Padding bits to fill up last byte
    #[cfg(any(test, feature = "fuzz_exports"))]
    pub fn encode(&mut self, data: &[u8]) {
        self.write_table();

        let mut state = self.table.start_state(data[data.len() - 1]);
        for x in data[0..data.len() - 1].iter().rev().copied() {
            let next = self.table.next_state(x, state.index);
            let diff = state.index - next.baseline;
            self.writer.write_bits(diff as u64, next.num_bits as usize);
            state = next;
        }
        self.writer
            .write_bits(state.index as u64, self.acc_log() as usize);

        let bits_to_fill = self.writer.misaligned();
        if bits_to_fill == 0 {
            self.writer.write_bits(1u32, 8);
        } else {
            self.writer.write_bits(1u32, bits_to_fill);
        }
    }

    /// Encodes the data using the provided table but with two interleaved streams
    /// Writes
    /// * Table description
    /// * Encoded data with two interleaved states
    /// * Both Last state indexes
    /// * Padding bits to fill up last byte
    pub fn encode_interleaved(&mut self, data: &[u8]) {
        self.write_table();

        let mut state_1 = self.table.start_state(data[data.len() - 1]);
        let mut state_2 = self.table.start_state(data[data.len() - 2]);

        // The first two symbols are represented by the start states
        // Then encode the state transitions for two symbols at a time
        let mut idx = data.len() - 4;
        loop {
            {
                let state = state_1;
                let x = data[idx + 1];
                let next = self.table.next_state(x, state.index);
                let diff = state.index - next.baseline;
                self.writer.write_bits(diff as u64, next.num_bits as usize);
                state_1 = next;
            }
            {
                let state = state_2;
                let x = data[idx];
                let next = self.table.next_state(x, state.index);
                let diff = state.index - next.baseline;
                self.writer.write_bits(diff as u64, next.num_bits as usize);
                state_2 = next;
            }

            if idx < 2 {
                break;
            }
            idx -= 2;
        }

        // Determine if we have an even or odd number of symbols to encode
        // If odd we need to encode the last states transition and encode the final states in the flipped order
        if idx == 1 {
            let state = state_1;
            let x = data[0];
            let next = self.table.next_state(x, state.index);
            let diff = state.index - next.baseline;
            self.writer.write_bits(diff as u64, next.num_bits as usize);
            state_1 = next;

            self.writer
                .write_bits(state_2.index as u64, self.acc_log() as usize);
            self.writer
                .write_bits(state_1.index as u64, self.acc_log() as usize);
        } else {
            self.writer
                .write_bits(state_1.index as u64, self.acc_log() as usize);
            self.writer
                .write_bits(state_2.index as u64, self.acc_log() as usize);
        }

        let bits_to_fill = self.writer.misaligned();
        if bits_to_fill == 0 {
            self.writer.write_bits(1u32, 8);
        } else {
            self.writer.write_bits(1u32, bits_to_fill);
        }
    }

    fn write_table(&mut self) {
        self.table.write_table(self.writer);
    }

    pub(super) fn acc_log(&self) -> u8 {
        self.table.acc_log()
    }
}

#[derive(Debug, Clone)]
pub struct FSETable {
    /// Indexed by symbol
    pub(super) states: [SymbolStates; 256],
    /// Sum of all states.states.len()
    pub(crate) table_size: usize,
}

impl FSETable {
    pub(crate) fn next_state(&self, symbol: u8, idx: usize) -> &State {
        let states = &self.states[symbol as usize];
        states.get(idx, self.table_size)
    }

    pub(crate) fn start_state(&self, symbol: u8) -> &State {
        let states = &self.states[symbol as usize];
        &states.states[0]
    }

    pub fn acc_log(&self) -> u8 {
        self.table_size.ilog2() as u8
    }

    /// The normalized probability of `symbol` (`-1` is "less than one",
    /// `0` means the table cannot encode it).
    pub(crate) fn probability(&self, symbol: u8) -> i32 {
        self.states[symbol as usize].probability
    }

    pub(crate) fn write_table<V: AsMut<Vec<u8>>>(&self, writer: &mut BitWriter<V>) {
        writer.write_bits(self.acc_log() - 5, 4);
        let mut probability_counter = 0usize;
        let probability_sum = 1 << self.acc_log();

        let mut prob_idx = 0;
        while probability_counter < probability_sum {
            let max_remaining_value = probability_sum - probability_counter + 1;
            let bits_to_write = max_remaining_value.ilog2() + 1;
            let low_threshold = ((1 << bits_to_write) - 1) - (max_remaining_value);
            let mask = (1 << (bits_to_write - 1)) - 1;

            let prob = self.states[prob_idx].probability;
            prob_idx += 1;
            let value = (prob + 1) as u32;
            if value < low_threshold as u32 {
                writer.write_bits(value, bits_to_write as usize - 1);
            } else if value > mask {
                writer.write_bits(value + low_threshold as u32, bits_to_write as usize);
            } else {
                writer.write_bits(value, bits_to_write as usize);
            }

            if prob == -1 {
                probability_counter += 1;
            } else if prob > 0 {
                probability_counter += prob as usize;
            } else {
                let mut zeros = 0u8;
                while self.states[prob_idx].probability == 0 {
                    zeros += 1;
                    prob_idx += 1;
                    if zeros == 3 {
                        writer.write_bits(3u8, 2);
                        zeros = 0;
                    }
                }
                writer.write_bits(zeros, 2);
            }
        }
        writer.write_bits(0u8, writer.misaligned());
    }
}

#[derive(Debug, Clone)]
pub(super) struct SymbolStates {
    /// Sorted by baseline to allow easy lookup using an index
    pub(super) states: Vec<State>,
    pub(super) probability: i32,
}

impl SymbolStates {
    fn get(&self, idx: usize, max_idx: usize) -> &State {
        let start_search_at = (idx * self.states.len()) / max_idx;
        self.states[start_search_at..]
            .iter()
            .find(|state| state.contains(idx))
            .unwrap()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    /// How many bits the range of this state needs to be encoded as
    pub(crate) num_bits: u8,
    /// The first index targeted by this state
    pub(crate) baseline: usize,
    /// The last index targeted by this state (baseline + the maximum number with numbits bits allows)
    pub(crate) last_index: usize,
    /// Index of this state in the decoding table
    pub(crate) index: usize,
}

impl State {
    fn contains(&self, idx: usize) -> bool {
        self.baseline <= idx && self.last_index >= idx
    }
}

pub fn build_table_from_data(
    data: impl Iterator<Item = u8>,
    max_log: u8,
    avoid_0_numbit: bool,
) -> FSETable {
    let mut counts = [0; 256];
    let mut max_symbol = 0;
    for x in data {
        counts[x as usize] += 1;
    }
    for (idx, count) in counts.iter().copied().enumerate() {
        if count > 0 {
            max_symbol = idx;
        }
    }
    // Keep at least two symbol slots: when only symbol 0 occurs, the
    // `avoid_0_numbit` redistribution needs a second (unused) symbol to give
    // probability to.
    build_table_from_counts(&counts[..=max_symbol.max(1)], max_log, avoid_0_numbit)
}

#[cfg(test)]
mod single_symbol_tests {
    use super::build_table_from_data;

    /// Every sequence of a block can use the same code, e.g. offset code 0
    /// (repeat offset 1); the table must still build.
    #[test]
    fn a_single_symbol_zero_builds_a_table() {
        for max_log in [8, 9] {
            let table = build_table_from_data(core::iter::repeat_n(0u8, 300), max_log, true);
            assert!(table.table_size > 0);
            let _ = table.start_state(0);
        }
    }
}

fn build_table_from_counts(counts: &[usize], max_log: u8, avoid_0_numbit: bool) -> FSETable {
    let (probs, acc_log) = normalize_counts(counts, max_log, avoid_0_numbit);
    build_table_from_probabilities(&probs, acc_log)
}

/// Slots a normalized probability occupies (`-1`, "less than one", takes one).
fn slots(prob: i32) -> i32 {
    if prob == -1 {
        1
    } else {
        prob
    }
}

/// The table log for `total` symbols drawn from `counts.len()` values
/// (zstd's `FSE_optimalTableLog`): no larger than the data warrants, large
/// enough to give every value a slot.
fn table_log(total: usize, symbols: usize, max_log: u8) -> u8 {
    let high_bit = |x: usize| (usize::BITS - 1 - x.max(1).leading_zeros()) as u8;
    let from_source = high_bit(total.saturating_sub(1)).saturating_sub(2);
    let minimum = u8::min(high_bit(total) + 1, high_bit(symbols.saturating_sub(1)) + 2);
    u8::max(u8::min(max_log, from_source), minimum).clamp(5, max_log.max(5))
}

/// Probabilities proportional to `counts` that sum to `1 << acc_log`
/// (zstd's `FSE_normalizeCount`). Symbols too rare for a full slot get the
/// "less than one" probability `-1`. With `avoid_0_numbit`, no probability
/// exceeds half the table, so every state reads at least one bit.
fn normalize_counts(counts: &[usize], max_log: u8, avoid_0_numbit: bool) -> (Vec<i32>, u8) {
    // Rounding thresholds for small probabilities, from zstd.
    const ROUND_UP: [u64; 8] = [0, 473195, 504333, 520860, 550000, 700000, 750000, 830000];
    let total: usize = counts.iter().sum();
    assert!(total > 0);
    let acc_log = table_log(total, counts.len(), max_log);
    let size = 1i64 << acc_log;
    let mut probs = alloc::vec![0i32; counts.len()];

    let scale = 62 - acc_log as u32;
    let step = (1u64 << 62) / total as u64;
    let v_step = 1u64 << (scale - 20);
    let low_threshold = total >> acc_log;
    let mut remaining = size;
    let mut largest = 0;
    let mut largest_prob = 0i32;
    for (symbol, &count) in counts.iter().enumerate() {
        if count == 0 {
            continue;
        }
        if count <= low_threshold {
            probs[symbol] = -1;
            remaining -= 1;
            continue;
        }
        let exact = count as u64 * step;
        let mut prob = (exact >> scale) as i32;
        if prob < 8 {
            let rest = exact - ((prob as u64) << scale);
            if rest > v_step * ROUND_UP[prob as usize] {
                prob += 1;
            }
        }
        if prob > largest_prob {
            largest_prob = prob;
            largest = symbol;
        }
        probs[symbol] = prob;
        remaining -= prob as i64;
    }
    // Hand the rounding error to the most probable symbol, unless that
    // would distort it; then fall back to largest-remainder rounding.
    if -remaining >= (probs[largest] >> 1) as i64 || probs[largest] as i64 + remaining < 1 {
        probs = largest_remainder(counts, total, size);
    } else {
        probs[largest] += remaining as i32;
    }

    if avoid_0_numbit {
        let half = (size / 2) as i32;
        if let Some(max) = (0..probs.len()).max_by_key(|&s| probs[s]) {
            if probs[max] > half {
                let excess = probs[max] - half;
                probs[max] = half;
                // Prefer a symbol that occurs; otherwise lift an unused one.
                let target = (0..probs.len())
                    .filter(|&s| s != max)
                    .max_by_key(|&s| (slots(probs[s]), core::cmp::Reverse(s)))
                    .expect("at least two symbol slots");
                probs[target] = slots(probs[target]) + excess;
                debug_assert!(probs[target] <= half);
            }
        }
    }
    debug_assert_eq!(probs.iter().map(|&p| slots(p) as i64).sum::<i64>(), size);
    (probs, acc_log)
}

/// Proportional probabilities summing to `size`: each occurring symbol gets
/// at least one slot, and leftover slots go to the largest remainders.
fn largest_remainder(counts: &[usize], total: usize, size: i64) -> Vec<i32> {
    let mut probs = alloc::vec![0i32; counts.len()];
    let mut remainders = Vec::new();
    let mut used = 0i64;
    for (symbol, &count) in counts.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let exact = count as u128 * size as u128;
        let prob = ((exact / total as u128) as i64).max(1);
        probs[symbol] = prob as i32;
        used += prob;
        remainders.push((exact % total as u128, symbol));
    }
    remainders.sort_unstable_by(|a, b| b.cmp(a));
    let mut index = 0;
    while used < size {
        probs[remainders[index % remainders.len()].1] += 1;
        used += 1;
        index += 1;
    }
    // Too many slots (from the minimum of one): take them from the largest.
    while used > size {
        let largest = (0..probs.len()).max_by_key(|&s| probs[s]).unwrap();
        probs[largest] -= 1;
        used -= 1;
    }
    probs
}

#[cfg(test)]
mod normalize_tests {
    use super::{normalize_counts, slots};
    use alloc::vec::Vec;

    #[test]
    fn probabilities_fill_the_table_and_follow_the_counts() {
        let mut seed = 0x1234_5678u32;
        let mut cases: Vec<Vec<usize>> = alloc::vec![
            alloc::vec![1000, 1],
            alloc::vec![5, 0],
            alloc::vec![1; 53],
            alloc::vec![100_000, 3, 2, 1, 0, 0, 7],
        ];
        for _ in 0..200 {
            let len = 2 + (seed % 52) as usize;
            cases.push(
                (0..len)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        (seed % 5000) as usize >> (seed % 12)
                    })
                    .collect(),
            );
        }
        for counts in cases {
            if counts.iter().sum::<usize>() == 0 {
                continue;
            }
            for (max_log, avoid) in [(9, true), (8, true), (9, false)] {
                let (probs, acc_log) = normalize_counts(&counts, max_log, avoid);
                assert!((5..=max_log).contains(&acc_log));
                let sum: i32 = probs.iter().map(|&p| slots(p)).sum();
                assert_eq!(sum, 1 << acc_log, "{:?} -> {:?}", counts, probs);
                for (&count, &prob) in counts.iter().zip(&probs) {
                    if count > 0 {
                        assert!(prob == -1 || prob >= 1);
                    }
                }
                if avoid {
                    assert!(probs.iter().all(|&p| p <= 1 << (acc_log - 1)));
                }
            }
        }
    }
}

pub(super) fn build_table_from_probabilities(probs: &[i32], acc_log: u8) -> FSETable {
    let mut states = core::array::from_fn::<SymbolStates, 256, _>(|_| SymbolStates {
        states: Vec::new(),
        probability: 0,
    });

    // distribute -1 symbols
    let mut negative_idx = (1 << acc_log) - 1;
    for (symbol, _prob) in probs
        .iter()
        .copied()
        .enumerate()
        .filter(|prob| prob.1 == -1)
    {
        states[symbol].states.push(State {
            num_bits: acc_log,
            baseline: 0,
            last_index: (1 << acc_log) - 1,
            index: negative_idx,
        });
        states[symbol].probability = -1;
        negative_idx -= 1;
    }

    // distribute other symbols

    // Setup all needed states per symbol with their respective index
    let mut idx = 0;
    for (symbol, prob) in probs.iter().copied().enumerate() {
        if prob <= 0 {
            continue;
        }
        states[symbol].probability = prob;
        let states = &mut states[symbol].states;
        for _ in 0..prob {
            states.push(State {
                num_bits: 0,
                baseline: 0,
                last_index: 0,
                index: idx,
            });

            idx = next_position(idx, 1 << acc_log);
            while idx > negative_idx {
                idx = next_position(idx, 1 << acc_log);
            }
        }
        assert_eq!(states.len(), prob as usize);
    }

    // After all states know their index we can determine the numbits and baselines
    for (symbol, prob) in probs.iter().copied().enumerate() {
        if prob <= 0 {
            continue;
        }
        let prob = prob as u32;
        let state = &mut states[symbol];

        // We process the states in their order in the table
        state.states.sort_by_key(|l| l.index);

        let prob_log = if prob.is_power_of_two() {
            prob.ilog2()
        } else {
            prob.ilog2() + 1
        };
        let rounded_up = 1u32 << prob_log;

        // The lower states target double the amount of indexes -> numbits + 1
        let double_states = rounded_up - prob;
        let single_states = prob - double_states;
        let num_bits = acc_log - prob_log as u8;
        let mut baseline = (single_states as usize * (1 << (num_bits))) % (1 << acc_log);
        for (idx, state) in state.states.iter_mut().enumerate() {
            if (idx as u32) < double_states {
                let num_bits = num_bits + 1;
                state.baseline = baseline;
                state.num_bits = num_bits;
                state.last_index = baseline + ((1 << num_bits) - 1);

                baseline += 1 << num_bits;
                baseline %= 1 << acc_log;
            } else {
                state.baseline = baseline;
                state.num_bits = num_bits;
                state.last_index = baseline + ((1 << num_bits) - 1);
                baseline += 1 << num_bits;
            }
        }

        // For encoding we use the states ordered by the indexes they target
        state.states.sort_by_key(|l| l.baseline);
    }

    FSETable {
        table_size: 1 << acc_log,
        states,
    }
}

/// Calculate the position of the next entry of the table given the current
/// position and size of the table.
fn next_position(mut p: usize, table_size: usize) -> usize {
    p += (table_size >> 1) + (table_size >> 3) + 3;
    p &= table_size - 1;
    p
}

const ML_DIST: &[i32] = &[
    1, 4, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1,
];

const LL_DIST: &[i32] = &[
    4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 1, 1, 1, 1, 1,
    -1, -1, -1, -1,
];

const OF_DIST: &[i32] = &[
    1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1,
];

pub(crate) fn default_ml_table() -> FSETable {
    build_table_from_probabilities(ML_DIST, 6)
}

pub(crate) fn default_ll_table() -> FSETable {
    build_table_from_probabilities(LL_DIST, 6)
}

pub(crate) fn default_of_table() -> FSETable {
    build_table_from_probabilities(OF_DIST, 5)
}
