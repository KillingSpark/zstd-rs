use hashbrown::HashMap;

use alloc::vec::Vec;

const MIN_MATCH_LEN: usize = 5;

struct WindowEntry<'data> {
    data: &'data [u8],
    suffixes: HashMap<[u8; MIN_MATCH_LEN], usize>,
    base_offset: usize,
}

pub(crate) struct MatchGenerator<'data> {
    max_window_size: usize,
    /// Data window we are operating on to find matches
    /// The data we want to find matches for is in the last slice
    window: Vec<WindowEntry<'data>>,
    window_size: usize,
    #[cfg(debug_assertions)]
    concat_window: Vec<u8>,
    /// Index in the last slice that we already processed
    suffix_idx: usize,
    /// Gets updated when a new sequence is returned to point right behind that sequence
    last_idx_in_sequence: usize,
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
            window: Vec::new(),
            window_size: 0,
            #[cfg(debug_assertions)]
            concat_window: Vec::new(),
            suffix_idx: 0,
            last_idx_in_sequence: 0,
        }
    }

    pub(crate) fn next_sequence(&mut self) -> Option<Sequence<'data>> {
        let mut sequence = None;

        while sequence.is_none() {
            let last_entry = self.window.last().unwrap();
            let data_slice = last_entry.data;
            if self.suffix_idx >= data_slice.len() {
                if self.last_idx_in_sequence != self.suffix_idx {
                    let literals = &data_slice[self.last_idx_in_sequence..];
                    self.last_idx_in_sequence = self.suffix_idx;
                    return Some(Sequence::Literals { literals });
                } else {
                    return None;
                }
            }
            let data_slice = &data_slice[self.suffix_idx..];

            if data_slice.len() < MIN_MATCH_LEN {
                let last_idx_in_sequence = self.last_idx_in_sequence;
                self.last_idx_in_sequence = last_entry.data.len();
                self.suffix_idx = last_entry.data.len();
                return Some(Sequence::Literals {
                    literals: &last_entry.data[last_idx_in_sequence..],
                });
            }

            let mut key = [0u8; MIN_MATCH_LEN];
            key.copy_from_slice(&data_slice[..MIN_MATCH_LEN]);

            for (match_entry_idx, match_entry) in self.window.iter().enumerate() {
                let is_last = match_entry_idx == self.window.len() - 1;
                if let Some(match_index) = match_entry.suffixes.get(&key).copied() {
                    let match_slice = if is_last {
                        &match_entry.data[match_index..self.suffix_idx]
                    } else {
                        &match_entry.data[match_index..]
                    };
                    let min_len = usize::min(match_slice.len(), data_slice.len());

                    let mut match_len = 0;
                    for idx in 0..min_len {
                        if match_slice[idx] != data_slice[idx] {
                            break;
                        }
                        match_len = idx + 1;
                    }

                    if match_len >= MIN_MATCH_LEN {
                        let literals = &last_entry.data[self.last_idx_in_sequence..self.suffix_idx];
                        let offset = if is_last {
                            self.suffix_idx - match_index
                        } else {
                            match_entry.base_offset - match_index + self.suffix_idx
                        };

                        #[cfg(debug_assertions)]
                        {
                            let unprocessed = last_entry.data.len() - self.suffix_idx;
                            let start = self.concat_window.len() - unprocessed - offset;
                            let end = start + match_len;
                            let check_slice = &self.concat_window[start..end];
                            debug_assert_eq!(check_slice, &match_slice[..match_len]);
                        }

                        sequence = Some(Sequence::Triple {
                            literals,
                            offset,
                            match_len,
                        });

                        break;
                    }
                }
            }

            if let Some(Sequence::Triple { match_len, .. }) = sequence {
                self.add_suffixes_till(self.suffix_idx + match_len);
                self.suffix_idx += match_len;
                self.last_idx_in_sequence = self.suffix_idx;
            } else {
                let last_entry = self.window.last_mut().unwrap();
                if !last_entry.suffixes.contains_key(&key) {
                    last_entry.suffixes.insert(key, self.suffix_idx);
                }
                self.suffix_idx += 1;
            }
        }

        sequence
    }

    fn add_suffixes_till(&mut self, idx: usize) {
        let last_entry = self.window.last_mut().unwrap();
        let last_idx = usize::min(idx, last_entry.data.len() - MIN_MATCH_LEN);
        for idx in self.suffix_idx..=last_idx {
            let mut key = [0u8; MIN_MATCH_LEN];
            key.copy_from_slice(&last_entry.data[idx..idx + MIN_MATCH_LEN]);
            if !last_entry.suffixes.contains_key(&key) {
                last_entry.suffixes.insert(key, idx);
            }
        }
    }

    pub(crate) fn add_data_no_matching(&mut self, data: &'data [u8]) {
        self.add_data(data);
        self.add_suffixes_till(data.len());
        self.suffix_idx = data.len();
        self.last_idx_in_sequence = data.len();
    }
    pub(crate) fn add_data(&mut self, data: &'data [u8]) {
        assert!(
            self.window.is_empty() || self.suffix_idx == self.window.last().unwrap().data.len()
        );
        self.reserve(data.len());
        #[cfg(debug_assertions)]
        self.concat_window.extend_from_slice(data);

        if let Some(last_len) = self.window.last().map(|last| last.data.len()) {
            for entry in self.window.iter_mut() {
                entry.base_offset += last_len;
            }
        }

        self.window.push(WindowEntry {
            data,
            suffixes: HashMap::with_capacity(data.len()),
            base_offset: 0,
        });
        self.window_size += data.len();
        self.suffix_idx = 0;
        self.last_idx_in_sequence = 0;
    }

    fn reserve(&mut self, amount: usize) {
        assert!(self.max_window_size > amount);
        let mut removed_slices = 0;
        while self.window_size + amount > self.max_window_size {
            let removed = self.window.remove(0);
            self.window_size -= removed.data.len();
            #[cfg(debug_assertions)]
            self.concat_window.drain(0..removed.data.len());
            removed_slices += 1;
        }
        if removed_slices == 0 {
            return;
        }
    }
}

#[test]
fn matches() {
    let mut matcher = MatchGenerator::new(1000);
    let mut original_data = Vec::new();
    let mut reconstructed = Vec::new();

    let mut assert_seq_equal = |seq1, seq2, reconstructed: &mut Vec<u8>| {
        assert_eq!(seq1, seq2);
        match seq2 {
            Sequence::Literals { literals } => reconstructed.extend_from_slice(literals),
            Sequence::Triple {
                literals,
                offset,
                match_len,
            } => {
                reconstructed.extend_from_slice(literals);
                let start = reconstructed.len() - offset;
                let end = start + match_len;
                reconstructed.extend_from_within(start..end);
            }
        }
    };

    matcher.add_data(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    original_data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Triple {
            literals: &[0, 0, 0, 0, 0],
            offset: 5,
            match_len: 5,
        },
        &mut reconstructed,
    );

    assert!(matcher.next_sequence().is_none());

    matcher.add_data(&[
        1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 0,
    ]);
    original_data.extend_from_slice(&[
        1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 0,
    ]);

    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Triple {
            literals: &[1, 2, 3, 4, 5, 6],
            offset: 6,
            match_len: 6,
        },
        &mut reconstructed,
    );
    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Triple {
            literals: &[],
            offset: 12,
            match_len: 6,
        },
        &mut reconstructed,
    );
    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Triple {
            literals: &[],
            offset: 28,
            match_len: 5,
        },
        &mut reconstructed,
    );
    assert!(matcher.next_sequence().is_none());

    matcher.add_data(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0, 0, 0, 0]);
    original_data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0, 0, 0, 0]);

    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Triple {
            literals: &[],
            offset: 23,
            match_len: 6,
        },
        &mut reconstructed,
    );
    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Triple {
            literals: &[7, 8, 9, 10, 11],
            offset: 44,
            match_len: 5,
        },
        &mut reconstructed,
    );
    assert!(matcher.next_sequence().is_none());

    matcher.add_data(&[0, 0, 0, 0, 0]);
    original_data.extend_from_slice(&[0, 0, 0, 0, 0]);

    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Triple {
            literals: &[],
            offset: 49,
            match_len: 5,
        },
        &mut reconstructed,
    );
    assert!(matcher.next_sequence().is_none());

    matcher.add_data(&[7, 8, 9, 10, 11]);
    original_data.extend_from_slice(&[7, 8, 9, 10, 11]);

    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Triple {
            literals: &[],
            offset: 15,
            match_len: 5,
        },
        &mut reconstructed,
    );
    assert!(matcher.next_sequence().is_none());

    matcher.add_data_no_matching(&[1, 3, 5, 7, 9]);
    original_data.extend_from_slice(&[1, 3, 5, 7, 9]);
    reconstructed.extend_from_slice(&[1, 3, 5, 7, 9]);
    assert!(matcher.next_sequence().is_none());

    matcher.add_data(&[1, 3, 5, 7, 9]);
    original_data.extend_from_slice(&[1, 3, 5, 7, 9]);

    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Triple {
            literals: &[],
            offset: 5,
            match_len: 5,
        },
        &mut reconstructed,
    );
    assert!(matcher.next_sequence().is_none());

    matcher.add_data(&[0, 0, 11, 13, 15, 17, 19, 11, 13, 15, 17, 19, 21, 23]);
    original_data.extend_from_slice(&[0, 0, 11, 13, 15, 17, 19, 11, 13, 15, 17, 19, 21, 23]);

    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Triple {
            literals: &[0, 0, 11, 13, 15, 17, 19],
            offset: 5,
            match_len: 5,
        },
        &mut reconstructed,
    );
    assert_seq_equal(
        matcher.next_sequence().unwrap(),
        Sequence::Literals {
            literals: &[21, 23],
        },
        &mut reconstructed,
    );
    assert!(matcher.next_sequence().is_none());

    assert_eq!(reconstructed, original_data);
}
