//! Hash-chain based matcher with lazy match evaluation
//!
//! This is the matching algorithm used for [`CompressionLevel::Default`] and above. It follows the general design of
//! the `lazy`/`lazy2` strategies of the reference implementation:
//!
//! * All data of the current window is kept in one contiguous buffer, so matches can span block boundaries and
//!   reach back several blocks (up to the configured window size).
//! * A hash table maps the hash of the next `MIN_MATCH_LEN` bytes at a position to the most recent position with the
//!   same hash. A chain table links each position to the previous position with the same hash, which allows to look at
//!   several candidates and pick the longest match instead of only the most recent one.
//! * Before a match is emitted, the matcher checks whether a match starting one (or two) bytes later would be a
//!   better choice. The decision uses the same heuristic as the reference implementation: a match is worth roughly
//!   four points per matched byte, minus the number of bits needed to encode its offset.
//!
//! Positions in the tables are stored as `u32` relative to the start of the buffer. When the buffer grows past its
//! capacity the oldest data is dropped and all table entries are rebased. The table contents are only ever used as
//! *hints*; every candidate is verified by comparing the actual bytes, so stale entries can never produce an invalid
//! sequence.

use alloc::vec::Vec;
use core::convert::TryInto;

use super::{CompressionLevel, Sequence};
use crate::common::MAX_BLOCK_SIZE;

/// The minimum match length this matcher produces. This is also the number of bytes that are hashed.
const MIN_MATCH_LEN: usize = 5;

/// Multiplier used to hash the first 5 bytes of a suffix. Same constant as used by the reference implementation.
const PRIME_5_BYTES: u64 = 889_523_592_379;

/// In regions where no matches are found the matcher starts to skip positions. The step size grows
/// by one for every `1 << SEARCH_STRENGTH` bytes that passed since the last match.
const SEARCH_STRENGTH: usize = 8;

/// How many full blocks may accumulate on top of the window before the oldest data is dropped from the buffer.
const SLACK_BLOCKS: usize = 4;

/// Parameters of the [`LazyMatcher`]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LazyParams {
    /// log2 of the maximum back-reference distance
    pub(crate) window_log: u32,
    /// log2 of the number of entries in the hash table
    pub(crate) hash_log: u32,
    /// log2 of the number of entries in the chain table
    pub(crate) chain_log: u32,
    /// How many candidates are inspected at most per search
    pub(crate) search_depth: u32,
    /// How many positions after a found match are inspected for a better match (0, 1 or 2)
    pub(crate) lazy_steps: u32,
}

impl LazyParams {
    /// Parameters used for the given compression level, if that level uses this matcher
    pub(crate) fn for_level(level: CompressionLevel) -> Option<Self> {
        match level {
            CompressionLevel::Uncompressed | CompressionLevel::Fastest => None,
            CompressionLevel::Default => Some(Self {
                window_log: 21,
                hash_log: 17,
                chain_log: 16,
                search_depth: 8,
                lazy_steps: 1,
            }),
            CompressionLevel::Better => Some(Self {
                window_log: 22,
                hash_log: 19,
                chain_log: 18,
                search_depth: 32,
                lazy_steps: 2,
            }),
            CompressionLevel::Best => Some(Self {
                window_log: 23,
                hash_log: 20,
                chain_log: 19,
                search_depth: 128,
                lazy_steps: 2,
            }),
        }
    }

    fn window_size(&self) -> usize {
        1 << self.window_log
    }
}

/// Hash and chain tables. Separate from the data buffer so the two can be borrowed independently.
struct Tables {
    hash_log: u32,
    hash: Vec<u32>,
    chain: Vec<u32>,
    chain_mask: usize,
    /// All positions below this index have been inserted into the tables
    next_to_update: usize,
}

impl Tables {
    fn new(params: &LazyParams) -> Self {
        Self {
            hash_log: params.hash_log,
            hash: alloc::vec![0; 1 << params.hash_log],
            chain: alloc::vec![0; 1 << params.chain_log],
            chain_mask: (1 << params.chain_log) - 1,
            next_to_update: 0,
        }
    }

    fn clear(&mut self) {
        self.hash.fill(0);
        self.chain.fill(0);
        self.next_to_update = 0;
    }

    /// All data before `shift` has been dropped from the buffer, adjust the stored positions accordingly
    fn rebase(&mut self, shift: usize) {
        let shift = shift as u32;
        for entry in self.hash.iter_mut() {
            *entry = entry.saturating_sub(shift);
        }
        for entry in self.chain.iter_mut() {
            *entry = entry.saturating_sub(shift);
        }
        self.next_to_update = self.next_to_update.saturating_sub(shift as usize);
    }

    #[inline(always)]
    fn hash_at(&self, buf: &[u8], pos: usize) -> usize {
        let v = if let Some(bytes) = buf.get(pos..pos + 8) {
            u64::from_le_bytes(bytes.try_into().unwrap())
        } else {
            let mut v = 0u64;
            for (i, b) in buf[pos..pos + MIN_MATCH_LEN].iter().enumerate() {
                v |= (*b as u64) << (8 * i);
            }
            v
        };
        ((v << (64 - 8 * MIN_MATCH_LEN)).wrapping_mul(PRIME_5_BYTES) >> (64 - self.hash_log))
            as usize
    }

    /// Insert all positions in `next_to_update..target` into the tables
    #[inline(always)]
    fn insert_up_to(&mut self, buf: &[u8], target: usize) {
        while self.next_to_update < target {
            let pos = self.next_to_update;
            let h = self.hash_at(buf, pos);
            self.chain[pos & self.chain_mask] = self.hash[h];
            self.hash[h] = pos as u32;
            self.next_to_update = pos + 1;
        }
    }
}

/// A [`super::Matcher`] implementation using hash chains and lazy match evaluation, see the module documentation
pub(crate) struct LazyMatcher {
    params: LazyParams,
    /// Contiguous window data, the current block is at the end
    buf: Vec<u8>,
    /// The buffer is never allowed to grow larger than this
    capacity: usize,
    /// Index in `buf` where the current block starts
    block_start: usize,
    tables: Tables,
    /// Buffers handed out by `get_next_space`
    vec_pool: Vec<Vec<u8>>,
}

impl LazyMatcher {
    pub(crate) fn new(params: LazyParams) -> Self {
        let capacity = params.window_size() + SLACK_BLOCKS * MAX_BLOCK_SIZE as usize;
        Self {
            params,
            buf: Vec::new(),
            capacity,
            block_start: 0,
            tables: Tables::new(&params),
            vec_pool: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn params(&self) -> LazyParams {
        self.params
    }

    /// Reset for a new frame. Reconfigures the tables if the parameters changed, otherwise only clears them.
    pub(crate) fn reset(&mut self, params: LazyParams) {
        if params != self.params {
            *self = Self::new(params);
            return;
        }
        self.buf.clear();
        self.block_start = 0;
        self.tables.clear();
    }

    pub(crate) fn window_size(&self) -> u64 {
        self.params.window_size() as u64
    }

    pub(crate) fn get_next_space(&mut self) -> Vec<u8> {
        self.vec_pool.pop().unwrap_or_else(|| {
            let mut space = alloc::vec![0; MAX_BLOCK_SIZE as usize];
            space.resize(space.capacity(), 0);
            space
        })
    }

    pub(crate) fn get_last_space(&self) -> &[u8] {
        &self.buf[self.block_start..]
    }

    pub(crate) fn commit_space(&mut self, mut space: Vec<u8>) {
        assert!(space.len() <= MAX_BLOCK_SIZE as usize);
        if self.buf.len() + space.len() > self.capacity {
            // Drop everything that is older than one window before the start of the new block
            let keep = usize::min(self.buf.len(), self.params.window_size());
            let shift = self.buf.len() - keep;
            self.buf.copy_within(shift.., 0);
            self.buf.truncate(keep);
            self.tables.rebase(shift);
        }
        self.block_start = self.buf.len();
        self.buf.extend_from_slice(&space);

        space.clear();
        space.resize(space.capacity(), 0);
        self.vec_pool.push(space);
    }

    /// Nothing to do here. Positions are inserted into the tables lazily by the next search,
    /// this also covers the suffixes that straddle the block boundary.
    pub(crate) fn skip_matching(&mut self) {}

    pub(crate) fn start_matching(&mut self, mut handle_sequence: impl for<'a> FnMut(Sequence<'a>)) {
        let buf = self.buf.as_slice();
        let tables = &mut self.tables;
        let params = &self.params;
        let block_start = self.block_start;
        let block_end = buf.len();

        if block_end - block_start < MIN_MATCH_LEN {
            if block_end > block_start {
                handle_sequence(Sequence::Literals {
                    literals: &buf[block_start..],
                });
            }
            return;
        }

        let window_size = params.window_size();
        let depth = params.search_depth as usize;
        let chain_size = tables.chain_mask + 1;
        // Last position at which a search can be started
        let ilimit = block_end - MIN_MATCH_LEN;

        // Searches the tables for the longest match at `ip` and inserts `ip` into the tables
        let search = |tables: &mut Tables, ip: usize| -> (usize, usize) {
            tables.insert_up_to(buf, ip);
            let h = tables.hash_at(buf, ip);
            let mut candidate = tables.hash[h] as usize;
            tables.chain[ip & tables.chain_mask] = candidate as u32;
            tables.hash[h] = ip as u32;
            tables.next_to_update = ip + 1;

            let low_limit = ip.saturating_sub(window_size);
            let min_chain = ip.saturating_sub(chain_size);
            let mut best_len = 0;
            let mut best_offset = 0;
            for _ in 0..depth {
                if candidate < low_limit || candidate >= ip {
                    break;
                }
                // Quick rejection: the byte right after the current best match has to match for a longer match
                if buf[candidate + best_len] == buf[ip + best_len] {
                    let len = common_prefix_len(buf, candidate, ip, block_end);
                    if len > best_len {
                        best_len = len;
                        best_offset = ip - candidate;
                        if ip + len == block_end {
                            break;
                        }
                    }
                }
                if candidate <= min_chain {
                    // The chain entry of this candidate may have been overwritten already
                    break;
                }
                let next = tables.chain[candidate & tables.chain_mask] as usize;
                if next >= candidate {
                    break;
                }
                candidate = next;
            }
            (best_len, best_offset)
        };

        let mut ip = block_start;
        let mut anchor = block_start;

        while ip <= ilimit {
            let (mut match_len, mut offset) = search(tables, ip);
            let mut start = ip;
            if match_len < MIN_MATCH_LEN {
                // Nothing found, skip faster the longer we did not find anything
                ip += ((ip - anchor) >> SEARCH_STRENGTH) + 1;
                continue;
            }

            // Lazy evaluation: is a match starting a little later a better choice?
            let mut lazy_step = 0;
            while lazy_step < params.lazy_steps as usize && ip < ilimit {
                ip += 1;
                let (len2, offset2) = search(tables, ip);
                if len2 >= MIN_MATCH_LEN {
                    // The further we already moved away from `start` the more the new match has to gain
                    let gain1 = score(match_len, offset) + 4 + 3 * lazy_step as i64;
                    let gain2 = score(len2, offset2);
                    if gain2 > gain1 {
                        match_len = len2;
                        offset = offset2;
                        start = ip;
                        lazy_step = 0;
                        continue;
                    }
                }
                lazy_step += 1;
            }

            // Catch up: the match might extend backwards into the literals
            while start > anchor && start > offset && buf[start - 1] == buf[start - 1 - offset] {
                start -= 1;
                match_len += 1;
            }

            handle_sequence(Sequence::Triple {
                literals: &buf[anchor..start],
                offset,
                match_len,
            });

            ip = start + match_len;
            anchor = ip;
        }

        if anchor < block_end {
            handle_sequence(Sequence::Literals {
                literals: &buf[anchor..block_end],
            });
        }
    }
}

/// Heuristic value of a match: about 4 points per byte, minus the bits needed to encode the offset
#[inline(always)]
fn score(match_len: usize, offset: usize) -> i64 {
    4 * match_len as i64 - (offset + 1).ilog2() as i64
}

/// Length of the common prefix of `buf[a..]` and `buf[b..end]` where `a < b <= end <= buf.len()`
#[inline(always)]
fn common_prefix_len(buf: &[u8], mut a: usize, mut b: usize, end: usize) -> usize {
    let start = b;
    while b + 8 <= end {
        let x = u64::from_le_bytes(buf[a..a + 8].try_into().unwrap());
        let y = u64::from_le_bytes(buf[b..b + 8].try_into().unwrap());
        let diff = x ^ y;
        if diff != 0 {
            return b - start + (diff.trailing_zeros() / 8) as usize;
        }
        a += 8;
        b += 8;
    }
    while b < end && buf[a] == buf[b] {
        a += 1;
        b += 1;
    }
    b - start
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(seq: Sequence<'_>, reconstructed: &mut Vec<u8>, window_size: usize) {
        match seq {
            Sequence::Literals { literals } => reconstructed.extend_from_slice(literals),
            Sequence::Triple {
                literals,
                offset,
                match_len,
            } => {
                reconstructed.extend_from_slice(literals);
                assert!(offset >= 1);
                assert!(offset <= window_size);
                assert!(match_len >= MIN_MATCH_LEN);
                let start = reconstructed.len() - offset;
                for i in 0..match_len {
                    let byte = reconstructed[start + i];
                    reconstructed.push(byte);
                }
            }
        }
    }

    fn pseudo_random(len: usize, seed: u64) -> Vec<u8> {
        let mut state = seed;
        let mut data = Vec::with_capacity(len);
        while data.len() < len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            data.extend_from_slice(&state.to_le_bytes());
        }
        data.truncate(len);
        data
    }

    /// Feeds data block by block through a matcher and checks that the sequences reproduce the input
    fn roundtrip(matcher: &mut LazyMatcher, data: &[u8], skip_every: Option<usize>) {
        let window_size = matcher.window_size() as usize;
        matcher.reset(matcher.params());
        let mut reconstructed = Vec::with_capacity(data.len());
        let mut sequences = 0;
        for (block_idx, block) in data.chunks(MAX_BLOCK_SIZE as usize).enumerate() {
            let mut space = matcher.get_next_space();
            space.clear();
            space.extend_from_slice(block);
            matcher.commit_space(space);
            assert_eq!(matcher.get_last_space(), block);
            if skip_every.is_some_and(|n| block_idx % n == 1) {
                matcher.skip_matching();
                reconstructed.extend_from_slice(block);
            } else {
                matcher.start_matching(|seq| {
                    sequences += 1;
                    apply(seq, &mut reconstructed, window_size)
                });
            }
            assert_eq!(
                reconstructed.len(),
                block_idx * MAX_BLOCK_SIZE as usize + block.len()
            );
        }
        assert_eq!(reconstructed, data);
        let _ = sequences;
    }

    fn small_params() -> LazyParams {
        LazyParams {
            window_log: 20,
            hash_log: 12,
            chain_log: 10,
            search_depth: 8,
            lazy_steps: 2,
        }
    }

    #[test]
    fn finds_simple_matches() {
        let mut matcher = LazyMatcher::new(small_params());
        let data = b"abcdefghij_abcdefghij_abcdefghijklmnop";
        let mut space = matcher.get_next_space();
        space.clear();
        space.extend_from_slice(data);
        matcher.commit_space(space);
        let mut seqs = Vec::new();
        matcher.start_matching(|seq| match seq {
            Sequence::Literals { literals } => seqs.push((literals.to_vec(), 0, 0)),
            Sequence::Triple {
                literals,
                offset,
                match_len,
            } => seqs.push((literals.to_vec(), offset, match_len)),
        });
        assert_eq!(seqs[0], (b"abcdefghij_".to_vec(), 11, 21));
        assert_eq!(seqs[1], (b"klmnop".to_vec(), 0, 0));
    }

    #[test]
    fn roundtrip_random_and_repetitive() {
        let mut matcher = LazyMatcher::new(small_params());
        roundtrip(&mut matcher, &[], None);
        roundtrip(&mut matcher, b"abc", None);
        roundtrip(&mut matcher, b"abcabcabcabcabcabcabcabc", None);
        roundtrip(&mut matcher, &pseudo_random(300_000, 1), None);
        let mut data = pseudo_random(5000, 2);
        for i in 0..100usize {
            data.extend_from_slice(&pseudo_random(2000 + i, 3 + i as u64));
            data.extend_from_slice(&pseudo_random(5000, 2));
        }
        roundtrip(&mut matcher, &data, None);
        roundtrip(&mut matcher, &data, Some(3));
        let zeros = alloc::vec![0u8; 400_000];
        roundtrip(&mut matcher, &zeros, None);
        roundtrip(&mut matcher, &zeros, Some(2));
    }

    #[test]
    fn roundtrip_across_window_rebase() {
        // Small window so that the buffer gets shifted several times
        let params = LazyParams {
            window_log: 18,
            hash_log: 12,
            chain_log: 10,
            search_depth: 4,
            lazy_steps: 1,
        };
        let mut matcher = LazyMatcher::new(params);
        let unit = pseudo_random(100_000, 7);
        let mut data = Vec::new();
        for i in 0..40u8 {
            data.extend_from_slice(&unit);
            data.extend_from_slice(&[i; 1000]);
        }
        roundtrip(&mut matcher, &data, None);
        roundtrip(&mut matcher, &data, Some(5));
        // Same matcher, a new frame
        roundtrip(&mut matcher, &data[..250_000], None);
    }

    #[test]
    fn reset_with_other_params_reallocates() {
        let mut matcher = LazyMatcher::new(small_params());
        roundtrip(&mut matcher, &pseudo_random(10_000, 9), None);
        let other = LazyParams::for_level(CompressionLevel::Default).unwrap();
        matcher.reset(other);
        assert_eq!(matcher.params(), other);
        assert_eq!(matcher.window_size(), 1 << 21);
        roundtrip(&mut matcher, &pseudo_random(10_000, 9), None);
    }
}
