use core::cmp::Ordering;

use alloc::vec::Vec;

struct SuffixIndex {
    slice: usize,
    idx: usize,
}

pub(crate) struct MatchGenerator<'data> {
    max_window_size: usize,
    suffixes: Vec<SuffixIndex>,
    /// Data window we are operating on to find matches
    /// The data we want to find matches for is in the last slice
    window: Vec<&'data [u8]>,
    window_size: usize,
    /// Index in the last slice that we already processed
    suffix_idx: usize,
}

#[derive(PartialEq, Eq, Debug)]
pub(crate) enum Sequence<'data> {
    Triple {
        literals: &'data [u8],
        offset: usize,
        match_len: usize,
    },
    Literals {
        literals: &'data [u8],
    },
}

impl<'data> MatchGenerator<'data> {
    pub(crate) fn new(max_size: usize) -> Self {
        Self {
            max_window_size: max_size,
            suffixes: Vec::new(),
            window: Vec::new(),
            window_size: 0,
            suffix_idx: 0,
        }
    }

    pub(crate) fn next_sequence(&mut self) -> Option<Sequence<'data>> {
        let data_slice = self.window.last().unwrap();
        if self.suffix_idx >= data_slice.len() {
            return None;
        }

        let suffix_idx = SuffixIndex {
            slice: self.window.len() - 1,
            idx: self.suffix_idx,
        };
        if self.suffixes.is_empty() {
            assert!(self.window.len() == 1);
            assert!(self.suffix_idx == 0);
            self.suffixes.push(suffix_idx);

            // TODO find a better way instead of just pushing the first block without any sequences
            self.suffix_idx += data_slice.len();
            return Some(Sequence::Literals {
                literals: data_slice,
            });
        }

        let suffix = &data_slice[self.suffix_idx..];

        let mut upper_limit = self.suffixes.len() - 1;
        let mut lower_limit = 0;
        let mut candidate = Vec::with_capacity(self.window.len());
        let insert_idx = loop {
            let search_idx = (upper_limit + lower_limit) / 2;
            let candidate_idx = &self.suffixes[search_idx];
            candidate.clear();
            candidate.extend_from_slice(&self.window[candidate_idx.slice..]);
            candidate[0] = &candidate[0][candidate_idx.idx..];
            let last_idx = candidate.len() - 1;
            candidate[last_idx] = &candidate[last_idx][..self.suffix_idx];

            match compare_suffix(suffix, candidate.as_slice()) {
                Ordering::Equal => {
                    break search_idx;
                }
                Ordering::Less => {
                    upper_limit = search_idx;
                }
                Ordering::Greater => {
                    lower_limit = search_idx;
                }
            }

            if upper_limit - lower_limit <= 1 {
                break search_idx;
            }
        };

        self.suffixes.insert(insert_idx, suffix_idx);
        // TODO find longest matches between the new neighbours
        self.suffix_idx += data_slice.len();
        return Some(Sequence::Literals {
            literals: data_slice,
        });
    }

    pub(crate) fn add_data(&mut self, data: &'data [u8]) {
        assert!(self.window.is_empty() || self.suffix_idx == self.window.last().unwrap().len());
        self.reserve(data.len());
        self.window.push(data);
        self.window_size += data.len();
        self.suffix_idx = 0;
    }

    fn reserve(&mut self, amount: usize) {
        assert!(self.max_window_size > amount);
        let mut removed_slices = 0;
        while self.window_size + amount > self.max_window_size {
            let removed = self.window.remove(0);
            self.window_size -= removed.len();
            removed_slices += 1;
        }
        if removed_slices == 0 {
            return;
        }
        self.suffixes.retain_mut(|suffix_index| {
            if suffix_index.slice < removed_slices {
                false
            } else {
                suffix_index.slice -= removed_slices;
                true
            }
        });
    }
}

fn compare_suffix(suffix: &[u8], window: &[&[u8]]) -> Ordering {
    for (idx, b) in window.iter().flat_map(|slice| slice.iter()).enumerate() {
        if idx > suffix.len() {
            break;
        }
        let cmp = suffix[idx].cmp(b);
        if cmp != Ordering::Equal {
            return cmp;
        }
    }
    Ordering::Equal
}

#[test]
fn matches() {
    let mut matcher = MatchGenerator::new(16);
    matcher.add_data(&[0, 0, 0, 0]);
    let seq = matcher.next_sequence().unwrap();
    assert_eq!(
        seq,
        Sequence::Literals {
            literals: &[0, 0, 0, 0]
        }
    );
}
