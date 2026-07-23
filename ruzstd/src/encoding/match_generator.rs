//! Matching algorithm used find repeated parts in the original data
//!
//! The Zstd format relies on finden repeated sequences of data and compressing these sequences as instructions to the decoder.
//! A sequence basically tells the decoder "Go back X bytes and copy Y bytes to the end of your decode buffer".
//!
//! The task here is to efficiently find matches in the already encoded data for the current suffix of the not yet encoded data.

use alloc::vec::Vec;
use core::convert::TryFrom;
use core::num::NonZeroU32;

use super::CompressionLevel;
use super::Matcher;
use super::Sequence;

const MIN_MATCH_LEN: usize = 5;

/// This is the default implementation of the `Matcher` trait. It allocates and reuses the buffers when possible.
pub struct MatchGeneratorDriver {
    vec_pool: Vec<Vec<u8>>,
    suffix_pool: Vec<SuffixStore>,
    match_generator: MatchGenerator,
    slice_size: usize,
}

impl MatchGeneratorDriver {
    /// slice_size says how big the slices should be that are allocated to work with
    /// max_slices_in_window says how many slices should at most be used while looking for matches
    pub(crate) fn new(slice_size: usize, max_slices_in_window: usize) -> Self {
        Self {
            vec_pool: Vec::new(),
            suffix_pool: Vec::new(),
            match_generator: MatchGenerator::new(max_slices_in_window * slice_size),
            slice_size,
        }
    }
}

impl Matcher for MatchGeneratorDriver {
    fn reset(&mut self, _level: CompressionLevel) {
        let vec_pool = &mut self.vec_pool;
        let suffix_pool = &mut self.suffix_pool;

        self.match_generator.reset(|mut data, mut suffixes| {
            data.resize(data.capacity(), 0);
            vec_pool.push(data);
            suffixes.slots.clear();
            suffixes.slots.resize(suffixes.slots.capacity(), None);
            suffix_pool.push(suffixes);
        });
    }

    fn window_size(&self) -> u64 {
        self.match_generator.max_window_size as u64
    }

    fn get_next_space(&mut self) -> Vec<u8> {
        match self.vec_pool.pop() {
            Some(space) => space,
            None => {
                let mut space = alloc::vec![0; self.slice_size];
                space.resize(space.capacity(), 0);
                space
            }
        }
    }

    fn get_last_space(&mut self) -> &[u8] {
        self.match_generator.last_entry().data.as_slice()
    }

    fn commit_space(&mut self, space: Vec<u8>) {
        let vec_pool = &mut self.vec_pool;
        let suffixes = match self.suffix_pool.pop() {
            Some(suffixes) => suffixes,
            None => SuffixStore::with_capacity(space.len()),
        };
        let suffix_pool = &mut self.suffix_pool;
        self.match_generator
            .add_data(space, suffixes, |mut data, mut suffixes| {
                data.resize(data.capacity(), 0);
                vec_pool.push(data);
                suffixes.slots.clear();
                suffixes.slots.resize(suffixes.slots.capacity(), None);
                suffix_pool.push(suffixes);
            });
    }

    fn start_matching(&mut self, mut handle_sequence: impl for<'a> FnMut(Sequence<'a>)) {
        while self.match_generator.next_sequence(&mut handle_sequence) {}
    }
    fn skip_matching(&mut self) {
        self.match_generator.skip_matching();
    }
}

/// This stores the index of a suffix of a string by hashing the first few bytes of that suffix
/// This means that collisions just overwrite and that you need to check validity after a get
struct SuffixStore {
    slots: Vec<Option<Candidates>>,
    len_log: u32,
}

#[derive(Copy, Clone)]
struct Candidates {
    // We need 17 bits per index to store the maximum window size of 128kb.
    // We store indexes using one-based so Option can use a NonZeroU32 niche.
    oldest: NonZeroU32,
    newest: NonZeroU32,
}

struct CandidateIndexes {
    oldest: usize,
    newest: Option<usize>,
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
        let idx = Self::stored_index(idx);
        if let Some(slot) = self.slots[key] {
            self.slots[key] = Some(Candidates {
                oldest: slot.oldest,
                newest: idx,
            });
        } else {
            self.slots[key] = Some(Candidates {
                oldest: idx,
                newest: idx,
            });
        }
    }

    #[inline(always)]
    fn stored_index(idx: usize) -> NonZeroU32 {
        let idx = u32::try_from(idx + 1).expect("suffix index must fit in u32");
        NonZeroU32::new(idx).expect("suffix index is stored one-based")
    }

    #[inline(always)]
    fn candidates(&self, suffix: &[u8]) -> Option<CandidateIndexes> {
        let key = self.key(suffix);
        let slot = self.slots[key]?;
        let oldest = slot.oldest.get() as usize - 1;
        let newest = slot.newest.get() as usize - 1;
        let newest = if oldest == newest { None } else { Some(newest) };
        Some(CandidateIndexes { oldest, newest })
    }

    #[inline(always)]
    fn key(&self, suffix: &[u8]) -> usize {
        let value = u64::from(suffix[0])
            | (u64::from(suffix[1]) << 8)
            | (u64::from(suffix[2]) << 16)
            | (u64::from(suffix[3]) << 24)
            | (u64::from(suffix[4]) << 32);
        let index = value.wrapping_mul(0x9E37_79B1_85EB_CA87);
        let index = index >> (64 - self.len_log);
        index as usize % self.slots.len()
    }
}

/// We keep a window of a few of these entries
/// All of these are valid targets for a match to be generated for
struct WindowEntry {
    data: Vec<u8>,
    /// Stores indexes into data
    suffixes: SuffixStore,
    /// Makes offset calculations efficient
    base_offset: usize,
}

struct MatchCandidateContext<'data> {
    suffix_idx: usize,
    data_slice: &'data [u8],
    key: &'data [u8],
    #[cfg(debug_assertions)]
    last_entry_len: usize,
    #[cfg(debug_assertions)]
    concat_window: &'data [u8],
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

impl MatchGenerator {
    /// max_size defines how many bytes will be used at most in the window used for matching
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

    fn reset(&mut self, mut reuse_space: impl FnMut(Vec<u8>, SuffixStore)) {
        self.window_size = 0;
        #[cfg(debug_assertions)]
        self.concat_window.clear();
        self.suffix_idx = 0;
        self.last_idx_in_sequence = 0;
        self.window.drain(..).for_each(|entry| {
            reuse_space(entry.data, entry.suffixes);
        });
    }

    #[inline(always)]
    fn last_entry(&self) -> &WindowEntry {
        self.window
            .last()
            .expect("match generator requires a committed window entry")
    }

    #[inline(always)]
    fn last_entry_mut(&mut self) -> &mut WindowEntry {
        self.window
            .last_mut()
            .expect("match generator requires a committed window entry")
    }

    #[inline(always)]
    fn last_entry_index(&self) -> usize {
        self.window
            .len()
            .checked_sub(1)
            .expect("match generator requires a committed window entry")
    }

    /// Processes bytes in the current window until either a match is found or no more matches can be found
    /// * If a match is found handle_sequence is called with the Triple variant
    /// * If no more matches can be found but there are bytes still left handle_sequence is called with the Literals variant
    /// * If no more matches can be found and no more bytes are left this returns false
    fn next_sequence(&mut self, mut handle_sequence: impl for<'a> FnMut(Sequence<'a>)) -> bool {
        loop {
            let last_entry_idx = self.last_entry_index();
            let last_entry = &self.window[last_entry_idx];
            let data_slice = &last_entry.data;

            // We already reached the end of the window, check if we need to return a Literals{}
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

            // If the remaining data is smaller than the minimum match length we can stop and return a Literals{}
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

            // This is the key we are looking to find a match for
            let key = &data_slice[..MIN_MATCH_LEN];

            // Look in each window entry
            let mut candidate = None;
            let match_context = MatchCandidateContext {
                suffix_idx: self.suffix_idx,
                data_slice,
                key,
                #[cfg(debug_assertions)]
                last_entry_len: last_entry.data.len(),
                #[cfg(debug_assertions)]
                concat_window: &self.concat_window,
            };
            for (match_entry_idx, match_entry) in self.window.iter().enumerate() {
                let is_last = match_entry_idx == last_entry_idx;
                if let Some(candidates) = match_entry.suffixes.candidates(key) {
                    for match_index in candidates.newest.into_iter().chain([candidates.oldest]) {
                        let Some(found) = Self::match_candidate(
                            match_entry,
                            match_index,
                            is_last,
                            &match_context,
                        ) else {
                            continue;
                        };

                        let (offset, match_len) = found;
                        if let Some((old_offset, old_match_len)) = candidate {
                            if match_len > old_match_len
                                || (match_len == old_match_len && offset < old_offset)
                            {
                                candidate = Some((offset, match_len));
                            }
                        } else {
                            candidate = Some((offset, match_len));
                        }
                    }
                }
            }

            if let Some((offset, match_len)) = candidate {
                // For each index in the match we found we do not need to look for another match
                // But we still want them registered in the suffix store
                self.add_suffixes_till(self.suffix_idx + match_len);

                // All literals that were not included between this match and the last are now included here
                let last_entry_idx = self.last_entry_index();
                let last_entry = &self.window[last_entry_idx];
                let literals = &last_entry.data[self.last_idx_in_sequence..self.suffix_idx];

                // Update the indexes, all indexes upto and including the current index have been included in a sequence now
                self.suffix_idx += match_len;
                self.last_idx_in_sequence = self.suffix_idx;
                handle_sequence(Sequence::Triple {
                    literals,
                    offset,
                    match_len,
                });

                return true;
            }

            let suffix_idx = self.suffix_idx;
            let last_entry = self.last_entry_mut();
            let key = &last_entry.data[suffix_idx..suffix_idx + MIN_MATCH_LEN];
            last_entry.suffixes.insert(key, suffix_idx);
            self.suffix_idx += 1;
        }
    }

    /// Find the common prefix length between two byte slices
    #[inline(always)]
    fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
        Self::mismatch_chunks::<8>(a, b)
    }

    #[inline(always)]
    fn match_candidate(
        match_entry: &WindowEntry,
        match_index: usize,
        is_last: bool,
        context: &MatchCandidateContext<'_>,
    ) -> Option<(usize, usize)> {
        let match_slice = if is_last {
            &match_entry.data[match_index..context.suffix_idx]
        } else {
            &match_entry.data[match_index..]
        };

        if match_slice.len() < MIN_MATCH_LEN || &match_slice[..MIN_MATCH_LEN] != context.key {
            return None;
        }

        let match_len = MIN_MATCH_LEN
            + Self::common_prefix_len(
                &match_slice[MIN_MATCH_LEN..],
                &context.data_slice[MIN_MATCH_LEN..],
            );
        let offset = match_entry.base_offset + context.suffix_idx - match_index;

        #[cfg(debug_assertions)]
        {
            let unprocessed = context.last_entry_len - context.suffix_idx;
            let start = context.concat_window.len() - unprocessed - offset;
            let end = start + match_len;
            let check_slice = &context.concat_window[start..end];
            debug_assert_eq!(check_slice, &match_slice[..match_len]);
        }

        Some((offset, match_len))
    }

    /// Find the common prefix length between two byte slices with a configurable chunk length
    /// This enables vectorization optimizations
    fn mismatch_chunks<const N: usize>(xs: &[u8], ys: &[u8]) -> usize {
        let off = core::iter::zip(xs.chunks_exact(N), ys.chunks_exact(N))
            .take_while(|(x, y)| x == y)
            .count()
            * N;
        off + core::iter::zip(&xs[off..], &ys[off..])
            .take_while(|(x, y)| x == y)
            .count()
    }

    /// Process bytes and add the suffixes to the suffix store up to a specific index
    #[inline(always)]
    fn add_suffixes_till(&mut self, idx: usize) {
        let suffix_idx = self.suffix_idx;
        let last_entry = self.last_entry_mut();
        if last_entry.data.len() < MIN_MATCH_LEN {
            return;
        }
        let slice = &last_entry.data[suffix_idx..idx];
        for (key_index, key) in slice.windows(MIN_MATCH_LEN).enumerate() {
            last_entry.suffixes.insert(key, suffix_idx + key_index);
        }
    }

    /// Skip matching for the whole current window entry
    fn skip_matching(&mut self) {
        let len = self.last_entry().data.len();
        self.add_suffixes_till(len);
        self.suffix_idx = len;
        self.last_idx_in_sequence = len;
    }

    /// Add a new window entry. Will panic if the last window entry hasn't been processed properly.
    /// If any resources are released by pushing the new entry they are returned via the callback
    fn add_data(
        &mut self,
        data: Vec<u8>,
        suffixes: SuffixStore,
        reuse_space: impl FnMut(Vec<u8>, SuffixStore),
    ) {
        assert!(self.window.is_empty() || self.suffix_idx == self.last_entry().data.len());
        assert!(data.len() <= u32::MAX as usize);
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
            suffixes,
            base_offset: 0,
        });
        self.window_size += len;
        self.suffix_idx = 0;
        self.last_idx_in_sequence = 0;
    }

    /// Reserve space for a new window entry
    /// If any resources are released by pushing the new entry they are returned via the callback
    fn reserve(&mut self, amount: usize, mut reuse_space: impl FnMut(Vec<u8>, SuffixStore)) {
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
            reuse_space(leaked_vec, suffixes);
        }
    }
}

#[test]
fn matches() {
    let mut matcher = MatchGenerator::new(1000);
    let mut original_data = Vec::new();
    let mut reconstructed = Vec::new();

    let reconstruct = |seq: Sequence<'_>, reconstructed: &mut Vec<u8>| match seq {
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
    };
    let assert_seq_equal =
        |seq: Sequence<'_>, expected: Sequence<'_>, reconstructed: &mut Vec<u8>| {
            assert_eq!(seq, expected);
            reconstruct(seq, reconstructed);
        };

    matcher.add_data(
        alloc::vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        SuffixStore::with_capacity(100),
        |_, _| {},
    );
    original_data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

    matcher.next_sequence(|seq| {
        assert_eq!(
            seq,
            Sequence::Triple {
                literals: &[0, 0, 0, 0, 0],
                offset: 5,
                match_len: 5,
            },
        );
        reconstruct(seq, &mut reconstructed);
    });

    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(
        alloc::vec![1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 0,],
        SuffixStore::with_capacity(100),
        |_, _| {},
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
        );
    });
    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[],
                offset: 6,
                match_len: 6,
            },
            &mut reconstructed,
        );
    });
    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[],
                offset: 23,
                match_len: 5,
            },
            &mut reconstructed,
        );
    });
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(
        alloc::vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0, 0, 0, 0],
        SuffixStore::with_capacity(100),
        |_, _| {},
    );
    original_data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0, 0, 0, 0]);

    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[],
                offset: 11,
                match_len: 6,
            },
            &mut reconstructed,
        );
    });
    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[7, 8, 9, 10, 11],
                offset: 16,
                match_len: 5,
            },
            &mut reconstructed,
        );
    });
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(
        alloc::vec![0, 0, 0, 0, 0],
        SuffixStore::with_capacity(100),
        |_, _| {},
    );
    original_data.extend_from_slice(&[0, 0, 0, 0, 0]);

    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[],
                offset: 5,
                match_len: 5,
            },
            &mut reconstructed,
        );
    });
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(
        alloc::vec![7, 8, 9, 10, 11],
        SuffixStore::with_capacity(100),
        |_, _| {},
    );
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
        );
    });
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(
        alloc::vec![1, 3, 5, 7, 9],
        SuffixStore::with_capacity(100),
        |_, _| {},
    );
    matcher.skip_matching();
    original_data.extend_from_slice(&[1, 3, 5, 7, 9]);
    reconstructed.extend_from_slice(&[1, 3, 5, 7, 9]);
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(
        alloc::vec![1, 3, 5, 7, 9],
        SuffixStore::with_capacity(100),
        |_, _| {},
    );
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
        );
    });
    assert!(!matcher.next_sequence(|_| {}));

    matcher.add_data(
        alloc::vec![0, 0, 11, 13, 15, 17, 20, 11, 13, 15, 17, 20, 21, 23],
        SuffixStore::with_capacity(100),
        |_, _| {},
    );
    original_data.extend_from_slice(&[0, 0, 11, 13, 15, 17, 20, 11, 13, 15, 17, 20, 21, 23]);

    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Triple {
                literals: &[0, 0, 11, 13, 15, 17, 20],
                offset: 5,
                match_len: 5,
            },
            &mut reconstructed,
        );
    });
    matcher.next_sequence(|seq| {
        assert_seq_equal(
            seq,
            Sequence::Literals {
                literals: &[21, 23],
            },
            &mut reconstructed,
        );
    });
    assert!(!matcher.next_sequence(|_| {}));

    assert_eq!(reconstructed, original_data);
}
