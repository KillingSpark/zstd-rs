use hashbrown::HashMap;

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
            let mut space = Vec::new();
            space.resize(self.slice_size, 0);
            space.resize(space.capacity(), 0);
            self.current_space = Some(space);
            self.current_space.as_mut().unwrap()
        }
    }

    fn get_last_space(&mut self) -> &[u8] {
        self.match_generator.window.last().unwrap().leaked_vec.data
    }

    fn commit_space(&mut self, len: usize) {
        let vec_pool = &mut self.vec_pool;
        let vec = self.current_space.take().unwrap();
        let slice_size = self.slice_size;

        self.match_generator.add_data_no_matching(
            LeakedVec {
                cap: vec.capacity(),
                data: &vec.leak()[..len],
            },
            |data| {
                let vec = unsafe {
                    alloc::vec::Vec::from_raw_parts(
                        data.data.as_ptr() as *mut u8,
                        slice_size,
                        data.cap,
                    )
                };
                vec_pool.push(vec);
            },
        );
    }

    fn start_matching(
        &mut self,
        len: usize,
        mut handle_sequence: impl for<'a> FnMut(Sequence<'a>),
    ) {
        let vec_pool = &mut self.vec_pool;
        let vec = self.current_space.take().unwrap();
        let slice_size = self.slice_size;

        self.match_generator.add_data(
            LeakedVec {
                cap: vec.capacity(),
                data: &vec.leak()[..len],
            },
            |data| {
                let vec = unsafe {
                    alloc::vec::Vec::from_raw_parts(
                        data.data.as_ptr() as *mut u8,
                        slice_size,
                        data.cap,
                    )
                };
                vec_pool.push(vec);
            },
        );

        while let Some(seq) = self.match_generator.next_sequence() {
            handle_sequence(seq);
        }
    }
}

struct LeakedVec {
    data: &'static [u8],
    cap: usize,
}

struct WindowEntry {
    leaked_vec: LeakedVec,
    suffixes: HashMap<&'static [u8], usize>,
    base_offset: usize,
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

    fn next_sequence(&mut self) -> Option<Sequence<'static>> {
        let mut sequence = None;

        while sequence.is_none() {
            let last_entry = self.window.last().unwrap();
            let data_slice = last_entry.leaked_vec.data;
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
                self.last_idx_in_sequence = last_entry.leaked_vec.data.len();
                self.suffix_idx = last_entry.leaked_vec.data.len();
                return Some(Sequence::Literals {
                    literals: &&last_entry.leaked_vec.data[last_idx_in_sequence..],
                });
            }

            let key = &data_slice[..MIN_MATCH_LEN];

            for (match_entry_idx, match_entry) in self.window.iter().enumerate() {
                let is_last = match_entry_idx == self.window.len() - 1;
                if let Some(match_index) = match_entry.suffixes.get(&key).copied() {
                    let match_slice = if is_last {
                        &match_entry.leaked_vec.data[match_index..self.suffix_idx]
                    } else {
                        &match_entry.leaked_vec.data[match_index..]
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
                        let literals = &&last_entry.leaked_vec.data
                            [self.last_idx_in_sequence..self.suffix_idx];
                        let offset = match_entry.base_offset + self.suffix_idx - match_index;

                        #[cfg(debug_assertions)]
                        {
                            let unprocessed = last_entry.leaked_vec.data.len() - self.suffix_idx;
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
                let key =
                    &last_entry.leaked_vec.data[self.suffix_idx..self.suffix_idx + MIN_MATCH_LEN];
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
        if last_entry.leaked_vec.data.len() < MIN_MATCH_LEN {
            return;
        }
        let slice = &last_entry.leaked_vec.data[self.suffix_idx..idx];
        for (key_index, key) in slice.windows(MIN_MATCH_LEN).enumerate() {
            if !last_entry.suffixes.contains_key(&key) {
                last_entry.suffixes.insert(key, self.suffix_idx + key_index);
            }
        }
    }

    fn add_data_no_matching(&mut self, data: LeakedVec, reuse_space: impl FnMut(LeakedVec)) {
        let len = data.data.len();
        self.add_data(data, reuse_space);
        self.add_suffixes_till(len);
        self.suffix_idx = len;
        self.last_idx_in_sequence = len;
    }
    fn add_data(&mut self, data: LeakedVec, reuse_space: impl FnMut(LeakedVec)) {
        assert!(
            self.window.is_empty()
                || self.suffix_idx == self.window.last().unwrap().leaked_vec.data.len()
        );
        self.reserve(data.data.len(), reuse_space);
        #[cfg(debug_assertions)]
        self.concat_window.extend_from_slice(data.data);

        if let Some(last_len) = self.window.last().map(|last| last.leaked_vec.data.len()) {
            for entry in self.window.iter_mut() {
                entry.base_offset += last_len;
            }
        }

        let len = data.data.len();
        self.window.push(WindowEntry {
            leaked_vec: data,
            suffixes: HashMap::with_capacity(len),
            base_offset: 0,
        });
        self.window_size += len;
        self.suffix_idx = 0;
        self.last_idx_in_sequence = 0;
    }

    fn reserve(&mut self, amount: usize, mut reuse_space: impl FnMut(LeakedVec)) {
        assert!(self.max_window_size >= amount);
        while self.window_size + amount > self.max_window_size {
            let removed = self.window.remove(0);
            self.window_size -= removed.leaked_vec.data.len();
            #[cfg(debug_assertions)]
            self.concat_window.drain(0..removed.leaked_vec.data.len());
            reuse_space(removed.leaked_vec);
        }
    }
}

#[test]
fn matches() {
    let mut matcher = MatchGenerator::new(1000);
    let mut original_data = Vec::new();
    let mut reconstructed = Vec::new();

    let assert_seq_equal = |seq1, seq2, reconstructed: &mut Vec<u8>| {
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

    matcher.add_data(
        LeakedVec {
            cap: 0,
            data: &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        },
        |_| {},
    );
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

    matcher.add_data(
        LeakedVec {
            cap: 0,
            data: &[
                1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 0,
            ],
        },
        |_| {},
    );
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

    matcher.add_data(
        LeakedVec {
            cap: 0,
            data: &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0, 0, 0, 0],
        },
        |_| {},
    );
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

    matcher.add_data(
        LeakedVec {
            cap: 0,
            data: &[0, 0, 0, 0, 0],
        },
        |_| {},
    );
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

    matcher.add_data(
        LeakedVec {
            cap: 0,
            data: &[7, 8, 9, 10, 11],
        },
        |_| {},
    );
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

    matcher.add_data_no_matching(
        LeakedVec {
            cap: 0,
            data: &[1, 3, 5, 7, 9],
        },
        |_| {},
    );
    original_data.extend_from_slice(&[1, 3, 5, 7, 9]);
    reconstructed.extend_from_slice(&[1, 3, 5, 7, 9]);
    assert!(matcher.next_sequence().is_none());

    matcher.add_data(
        LeakedVec {
            cap: 0,
            data: &[1, 3, 5, 7, 9],
        },
        |_| {},
    );
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

    matcher.add_data(
        LeakedVec {
            cap: 0,
            data: &[0, 0, 11, 13, 15, 17, 19, 11, 13, 15, 17, 19, 21, 23],
        },
        |_| {},
    );
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
