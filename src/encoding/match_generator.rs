use alloc::vec::Vec;

use super::Matcher;

const MIN_MATCH_LEN: usize = 5;

/// Takes care of allocating and reusing vecs
pub(crate) struct MatchGeneratorDriver {
    vec_pool: Vec<Vec<u8>>,
    match_generator: MatchGenerator,
    slice_size: usize,
}

impl MatchGeneratorDriver {
    pub(crate) fn new(slice_size: usize, max_size: usize) -> Self {
        Self {
            vec_pool: Vec::new(),
            match_generator: MatchGenerator::new(max_size),
            slice_size,
        }
    }
}

impl Matcher for MatchGeneratorDriver {
    fn get_next_space(&mut self) -> Vec<u8> {
        self.vec_pool.pop().unwrap_or_else(|| {
            let mut space = alloc::vec![0; self.slice_size];
            space.resize(space.capacity(), 0);
            space
        })
    }

    fn get_last_space(&mut self) -> &[u8] {
        self.match_generator.window.last().unwrap().data.as_slice()
    }

    fn commit_space(&mut self, space: Vec<u8>) {
        let vec_pool = &mut self.vec_pool;
        self.match_generator.add_data(space, |mut data| {
            data.resize(data.capacity(), 0);
            vec_pool.push(data);
        });
    }

    fn start_matching(&mut self, mut handle_sequence: impl for<'a> FnMut(Sequence<'a>)) {
        while self.match_generator.next_sequence(&mut handle_sequence) {}
    }
    fn skip_matching(&mut self) {
        self.match_generator.skip_matching();
    }
}

struct WindowEntry {
    data: Vec<u8>,
    suffixes: SuffixStore,
    base_offset: usize,
}

struct SuffixStore {
    slots: Vec<Option<usize>>,
    len_log: u32,
}

impl SuffixStore {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: alloc::vec![None; capacity],
            len_log: capacity.ilog2(),
        }
    }

    #[inline(always)]
    fn insert(&mut self, suffix: &[u8], idx: usize) {
        let key = self.key(suffix);
        self.slots[key] = Some(idx);
    }

    #[inline(always)]
    fn contains_key(&self, suffix: &[u8]) -> bool {
        let key = self.key(suffix);
        self.slots[key].is_some()
    }

    #[inline(always)]
    fn get(&self, suffix: &[u8]) -> Option<usize> {
        let key = self.key(suffix);
        self.slots[key]
    }

    #[inline(always)]
    fn key(&self, suffix: &[u8]) -> usize {
        let s0 = suffix[0] as u64;
        let s1 = suffix[1] as u64;
        let s2 = suffix[2] as u64;
        let s3 = suffix[3] as u64;
        let s4 = suffix[4] as u64;

        const POLY: u64 = 0xCF3BCCDCABu64;

        let s0 = (s0 << 24).wrapping_mul(POLY);
        let s1 = (s1 << 32).wrapping_mul(POLY);
        let s2 = (s2 << 40).wrapping_mul(POLY);
        let s3 = (s3 << 48).wrapping_mul(POLY);
        let s4 = (s4 << 56).wrapping_mul(POLY);

        let index = s0 ^ s1 ^ s2 ^ s3 ^ s4;
        let index = index >> (64 - self.len_log);
        index as usize % self.slots.len()
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
                    let match_len = Self::common_prefix_len(match_slice, data_slice);

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

    #[inline(always)]
    fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
        Self::mismatch_chunks::<8>(a, b)
    }

    fn mismatch_chunks<const N: usize>(xs: &[u8], ys: &[u8]) -> usize {
        let off = core::iter::zip(xs.chunks_exact(N), ys.chunks_exact(N))
            .take_while(|(x, y)| x == y)
            .count()
            * N;
        off + core::iter::zip(&xs[off..], &ys[off..])
            .take_while(|(x, y)| x == y)
            .count()
    }

    #[inline(always)]
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

    fn skip_matching(&mut self) {
        let len = self.window.last().unwrap().data.len();
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

    matcher.add_data(alloc::vec![1, 3, 5, 7, 9], |_| {});
    matcher.skip_matching();
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
