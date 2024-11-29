use alloc::vec::Vec;

use super::Matcher;

const MIN_MATCH_LEN: usize = 5;

pub(crate) struct MatchGeneratorDriver {
    vec_pool: Vec<Vec<u8>>,
    current_space: Option<Vec<u8>>,
    match_generator: MatchGenerator,
    slice_size: usize,
}

impl MatchGeneratorDriver {
    pub(crate) fn new(slice_size: usize, max_size: usize) -> Self {
        Self {
            vec_pool: Vec::new(),
            current_space: None,
            match_generator: MatchGenerator::new(max_size),
            slice_size,
        }
    }
}

impl Matcher for MatchGeneratorDriver {
    fn get_next_space(&mut self) -> &mut [u8] {
        if self.current_space.is_some() {
            return self.current_space.as_mut().unwrap();
        }

        let space = self.vec_pool.pop();
        if space.is_some() {
            self.current_space = space;
            self.current_space.as_mut().unwrap()
        } else {
            let mut space = alloc::vec![0; self.slice_size];
            space.resize(space.capacity(), 0);
            self.current_space = Some(space);
            self.current_space.as_mut().unwrap()
        }
    }

    fn get_last_space(&mut self) -> &[u8] {
        self.match_generator.window.last().unwrap().data.as_slice()
    }

    fn commit_space(&mut self, len: usize) {
        let vec_pool = &mut self.vec_pool;
        let mut vec = self.current_space.take().unwrap();
        vec.resize(len, 0);

        self.match_generator.add_data_no_matching(vec, |data| {
            vec_pool.push(data);
        });
    }

    fn start_matching(
        &mut self,
        len: usize,
        mut handle_sequence: impl for<'a> FnMut(Sequence<'a>),
    ) {
        let vec_pool = &mut self.vec_pool;
        let mut vec = self.current_space.take().unwrap();
        vec.resize(len, 0);

        self.match_generator.add_data(vec, |data| {
            vec_pool.push(data);
        });

        while self.match_generator.next_sequence(&mut handle_sequence) {}
    }
}

struct WindowEntry {
    data: Vec<u8>,
    suffixes: SuffixStore,
    base_offset: usize,
}

struct SuffixStore {
    slots: Vec<Option<usize>>,
}

impl SuffixStore {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: alloc::vec![None; capacity],
        }
    }

    fn insert(&mut self, suffix: &[u8], idx: usize) {
        let key = self.key(suffix);
        self.slots[key] = Some(idx);
    }

    fn contains_key(&self, suffix: &[u8]) -> bool {
        let key = self.key(suffix);
        self.slots[key].is_some()
    }

    fn get(&self, suffix: &[u8]) -> Option<usize> {
        let key = self.key(suffix);
        self.slots[key]
    }

    fn key(&self, suffix: &[u8]) -> usize {
        let mut index = 0usize;
        for (high, b) in suffix
            .iter()
            .enumerate()
            .map(|(x, b)| (x % 2 == 0, (*b) as usize))
        {
            if high {
                index ^= b << 8;
            } else {
                index ^= b;
            }
        }
        index % self.slots.len()
    }
}

pub(crate) struct MatchGenerator {
    max_window_size: usize,
    /// Data window we are operating on to find matches
    /// The data we want to find matches for is in the last slice
    window: Vec<WindowEntry>,
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

impl MatchGenerator {
    fn new(max_size: usize) -> Self {
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

    fn next_sequence(&mut self, mut handle_sequence: impl for<'a> FnMut(Sequence<'a>)) -> bool {
        loop {
            let last_entry = self.window.last().unwrap();
            let data_slice = &last_entry.data;
            if self.suffix_idx >= data_slice.len() {
                if self.last_idx_in_sequence != self.suffix_idx {
                    let literals = &data_slice[self.last_idx_in_sequence..];
                    self.last_idx_in_sequence = self.suffix_idx;
                    handle_sequence(Sequence::Literals { literals });
                    return true;
                } else {
                    return false;
                }
            }
            let data_slice = &data_slice[self.suffix_idx..];

            if data_slice.len() < MIN_MATCH_LEN {
                let last_idx_in_sequence = self.last_idx_in_sequence;
                self.last_idx_in_sequence = last_entry.data.len();
                self.suffix_idx = last_entry.data.len();
                handle_sequence(Sequence::Literals {
                    literals: &last_entry.data[last_idx_in_sequence..],
                });
                return true;
            }

            let key = &data_slice[..MIN_MATCH_LEN];

            for (match_entry_idx, match_entry) in self.window.iter().enumerate() {
                let is_last = match_entry_idx == self.window.len() - 1;
                if let Some(match_index) = match_entry.suffixes.get(key) {
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
                        let offset = match_entry.base_offset + self.suffix_idx - match_index;

                        #[cfg(debug_assertions)]
                        {
                            let unprocessed = last_entry.data.len() - self.suffix_idx;
                            let start = self.concat_window.len() - unprocessed - offset;
                            let end = start + match_len;
                            let check_slice = &self.concat_window[start..end];
                            debug_assert_eq!(check_slice, &match_slice[..match_len]);
                        }

                        self.add_suffixes_till(self.suffix_idx + match_len);

                        let last_entry = self.window.last().unwrap();
                        let literals = &last_entry.data[self.last_idx_in_sequence..self.suffix_idx];
                        self.suffix_idx += match_len;
                        self.last_idx_in_sequence = self.suffix_idx;
                        handle_sequence(Sequence::Triple {
                            literals,
                            offset,
                            match_len,
                        });

                        return true;
                    }
                }
            }

            let last_entry = self.window.last_mut().unwrap();
            let key = &last_entry.data[self.suffix_idx..self.suffix_idx + MIN_MATCH_LEN];
            if !last_entry.suffixes.contains_key(key) {
                last_entry.suffixes.insert(key, self.suffix_idx);
            }
            self.suffix_idx += 1;
        }
    }

    fn add_suffixes_till(&mut self, idx: usize) {
        let last_entry = self.window.last_mut().unwrap();
        if last_entry.data.len() < MIN_MATCH_LEN {
            return;
        }
        let slice = &last_entry.data[self.suffix_idx..idx];
        for (key_index, key) in slice.windows(MIN_MATCH_LEN).enumerate() {
            if !last_entry.suffixes.contains_key(key) {
                last_entry.suffixes.insert(key, self.suffix_idx + key_index);
            }
        }
    }

    fn add_data_no_matching(&mut self, data: Vec<u8>, reuse_space: impl FnMut(Vec<u8>)) {
        let len = data.len();
        self.add_data(data, reuse_space);
        self.add_suffixes_till(len);
        self.suffix_idx = len;
        self.last_idx_in_sequence = len;
    }
    fn add_data(&mut self, data: Vec<u8>, reuse_space: impl FnMut(Vec<u8>)) {
        assert!(
            self.window.is_empty() || self.suffix_idx == self.window.last().unwrap().data.len()
        );
        self.reserve(data.len(), reuse_space);
        #[cfg(debug_assertions)]
        self.concat_window.extend_from_slice(&data);

        if let Some(last_len) = self.window.last().map(|last| last.data.len()) {
            for entry in self.window.iter_mut() {
                entry.base_offset += last_len;
            }
        }

        let len = data.len();
        self.window.push(WindowEntry {
            data,
            suffixes: SuffixStore::with_capacity(len),
            base_offset: 0,
        });
        self.window_size += len;
        self.suffix_idx = 0;
        self.last_idx_in_sequence = 0;
    }

    fn reserve(&mut self, amount: usize, mut reuse_space: impl FnMut(Vec<u8>)) {
        assert!(self.max_window_size >= amount);
        while self.window_size + amount > self.max_window_size {
            let removed = self.window.remove(0);
            self.window_size -= removed.data.len();
            #[cfg(debug_assertions)]
            self.concat_window.drain(0..removed.data.len());

            let WindowEntry {
                suffixes,
                data: leaked_vec,
                base_offset: _,
            } = removed;
            // Make sure all references into the leaked vec are gone
            drop(suffixes);
            // Then repurpose the vec
            reuse_space(leaked_vec);
        }
    }
}

#[test]
fn matches() {
    let mut matcher = MatchGenerator::new(1000);
    let mut original_data = Vec::new();
    let mut reconstructed = Vec::new();

    let assert_seq_equal = |seq1: Sequence<'_>, seq2: Sequence<'_>, reconstructed: &mut Vec<u8>| {
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

    matcher.add_data(alloc::vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0], |_| {});
    original_data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[0, 0, 0, 0, 0],
                offset: 5,
                match_len: 5,
            },
            &mut reconstructed,
        )
    });

    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(
        alloc::vec![1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 0,],
        |_| {},
    );
    original_data.extend_from_slice(&[
        1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 0,
    ]);

    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[1, 2, 3, 4, 5, 6],
                offset: 6,
                match_len: 6,
            },
            &mut reconstructed,
        )
    });
    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[],
                offset: 12,
                match_len: 6,
            },
            &mut reconstructed,
        )
    });
    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[],
                offset: 28,
                match_len: 5,
            },
            &mut reconstructed,
        )
    });
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(
        alloc::vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0, 0, 0, 0],
        |_| {},
    );
    original_data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0, 0, 0, 0]);

    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[],
                offset: 23,
                match_len: 6,
            },
            &mut reconstructed,
        )
    });
    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[7, 8, 9, 10, 11],
                offset: 44,
                match_len: 5,
            },
            &mut reconstructed,
        )
    });
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(alloc::vec![0, 0, 0, 0, 0], |_| {});
    original_data.extend_from_slice(&[0, 0, 0, 0, 0]);

    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[],
                offset: 49,
                match_len: 5,
            },
            &mut reconstructed,
        )
    });
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(alloc::vec![7, 8, 9, 10, 11], |_| {});
    original_data.extend_from_slice(&[7, 8, 9, 10, 11]);

    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[],
                offset: 15,
                match_len: 5,
            },
            &mut reconstructed,
        )
    });
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data_no_matching(alloc::vec![1, 3, 5, 7, 9], |_| {});
    original_data.extend_from_slice(&[1, 3, 5, 7, 9]);
    reconstructed.extend_from_slice(&[1, 3, 5, 7, 9]);
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(alloc::vec![1, 3, 5, 7, 9], |_| {});
    original_data.extend_from_slice(&[1, 3, 5, 7, 9]);

    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[],
                offset: 5,
                match_len: 5,
            },
            &mut reconstructed,
        )
    });
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(
        alloc::vec![0, 0, 11, 13, 15, 17, 19, 11, 13, 15, 17, 19, 21, 23],
        |_| {},
    );
    original_data.extend_from_slice(&[0, 0, 11, 13, 15, 17, 19, 11, 13, 15, 17, 19, 21, 23]);

    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[0, 0, 11, 13, 15, 17, 19],
                offset: 5,
                match_len: 5,
            },
            &mut reconstructed,
        )
    });
    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Literals {
                literals: &[21, 23],
            },
            &mut reconstructed,
        )
    });
    assert!(!matcher.next_sequence(|_| {}));

    assert_eq!(reconstructed, original_data);
}
