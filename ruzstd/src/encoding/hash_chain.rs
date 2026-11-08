//! Hash-chain match finder with lazy evaluation, used by
//! [`CompressionLevel::Default`] and [`CompressionLevel::Better`].
//!
//! Every position is inserted into a hash table whose entries chain back to
//! earlier positions with the same hash, so a search visits the most recent
//! candidates first. Matches at one of the repeat offsets are tried first,
//! since they are the cheapest to encode. Before a match is taken, the next
//! position (or two) is searched as well, and a longer match there wins
//! ("lazy" matching, like zstd's lazy and lazy2 strategies). Matches are then
//! extended backwards into the pending literals.

use alloc::vec::Vec;
use core::convert::TryInto;

use super::{offset_history::OffsetHistory, CompressionLevel, Sequence};

/// Search parameters for one level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Params {
    /// The decoder window: matches reach at most `1 << window_log` bytes back.
    window_log: u32,
    /// Size limit of the hash table (it starts smaller for small inputs).
    hash_log: u32,
    /// Size limit of the chain table, bounding how far back chains reach.
    chain_log: u32,
    /// Chain entries examined per search.
    search_depth: u32,
    /// Following positions searched before a match is taken.
    lazy_depth: u32,
    /// Shortest match taken (and the number of bytes hashed).
    min_match: usize,
    /// A match at least this long ends the search early.
    target_len: usize,
}

impl Params {
    pub(crate) fn for_level(level: CompressionLevel) -> Option<Self> {
        match level {
            CompressionLevel::Default => Some(Self {
                window_log: 21,
                hash_log: 17,
                chain_log: 16,
                search_depth: 4,
                lazy_depth: 1,
                min_match: 5,
                target_len: 32,
            }),
            CompressionLevel::Better => Some(Self {
                window_log: 21,
                hash_log: 19,
                chain_log: 18,
                search_depth: 16,
                lazy_depth: 2,
                min_match: 5,
                target_len: 64,
            }),
            _ => None,
        }
    }
}

/// Positions skip ahead faster the longer no match was found, so
/// incompressible data costs few searches (zstd's `kSearchStrength`).
const SKIP_STRENGTH: u32 = 8;
/// Smallest tables allocated.
const MIN_TABLE_LOG: u32 = 10;
/// Stored positions are relative to `base`; rebase before they overflow.
const REBASE_AT: usize = 1 << 31;

#[derive(Clone, Copy)]
struct Match {
    len: usize,
    offset: usize,
}

pub(crate) struct HashChainMatcher {
    params: Params,
    /// The window followed by the block being matched.
    history: Vec<u8>,
    /// Absolute position of `history[0]`.
    history_start: usize,
    /// Absolute position where the last committed block starts.
    block_start: usize,
    /// Head of each hash chain: `position - base + 1`, or 0 for none.
    head: Vec<u32>,
    /// The previous position with the same hash, indexed by position.
    chain: Vec<u32>,
    hash_log: u32,
    chain_log: u32,
    base: usize,
    /// The next absolute position to insert.
    next_insert: usize,
    /// Repeat offsets as the encoder will track them, to search them first.
    offsets: OffsetHistory,
}

impl HashChainMatcher {
    pub(crate) fn new(params: Params) -> Self {
        Self {
            params,
            history: Vec::new(),
            history_start: 0,
            block_start: 0,
            head: Vec::new(),
            chain: Vec::new(),
            hash_log: 0,
            chain_log: 0,
            base: 0,
            next_insert: 0,
            offsets: OffsetHistory::new(),
        }
    }

    pub(crate) fn reset(&mut self, params: Params) {
        self.params = params;
        self.history.clear();
        self.history_start = 0;
        self.block_start = 0;
        self.head.clear();
        self.chain.clear();
        self.hash_log = 0;
        self.chain_log = 0;
        self.base = 0;
        self.next_insert = 0;
        self.offsets = OffsetHistory::new();
    }

    pub(crate) fn window_size(&self) -> u64 {
        1 << self.params.window_log
    }

    fn window(&self) -> usize {
        1 << self.params.window_log
    }

    fn history_end(&self) -> usize {
        self.history_start + self.history.len()
    }

    /// The bytes of the last committed block.
    pub(crate) fn last_block(&self) -> &[u8] {
        &self.history[self.block_start - self.history_start..]
    }

    /// Append a block to match next. Only the window before it is kept.
    pub(crate) fn add_block(&mut self, block: &[u8]) {
        let window = self.window();
        // Trim rarely: only once twice the window has accumulated.
        if self.history.len() > 2 * window {
            let drop = self.history.len() - window;
            self.history.drain(..drop);
            self.history_start += drop;
        }
        self.block_start = self.history_end();
        self.history.extend_from_slice(block);
        if self.history_end() - self.base >= REBASE_AT {
            self.rebase();
        }
        let span = (self.history_end()
            - self
                .history_start
                .max(self.block_start.saturating_sub(window)))
        .max(1);
        self.grow_tables(span);
    }

    /// Size the tables for `span` bytes of window and block, up to the
    /// level's limits. Growing re-inserts the positions already seen.
    fn grow_tables(&mut self, span: usize) {
        let log = usize::BITS - (span - 1).leading_zeros();
        let hash_log = (log + 1).clamp(MIN_TABLE_LOG, self.params.hash_log.max(MIN_TABLE_LOG));
        let chain_log = log.clamp(MIN_TABLE_LOG, self.params.chain_log.max(MIN_TABLE_LOG));
        if hash_log <= self.hash_log && chain_log <= self.chain_log {
            return;
        }
        self.hash_log = hash_log.max(self.hash_log);
        self.chain_log = chain_log.max(self.chain_log);
        self.head = alloc::vec![0; 1 << self.hash_log];
        self.chain = alloc::vec![0; 1 << self.chain_log];
        let inserted = self.next_insert;
        let lowest = self
            .history_start
            .max(inserted.saturating_sub(1 << self.chain_log));
        self.next_insert = lowest.min(inserted);
        self.insert_until(inserted);
    }

    /// Move `base` up to the window so stored positions stay small.
    fn rebase(&mut self) {
        let new_base = self.history_start;
        let shift = new_base - self.base;
        for entry in self.head.iter_mut().chain(self.chain.iter_mut()) {
            *entry = if (*entry as usize) > shift {
                *entry - shift as u32
            } else {
                0
            };
        }
        self.base = new_base;
    }

    fn byte(&self, position: usize) -> u8 {
        self.history[position - self.history_start]
    }

    /// Hash of the `min_match` bytes at `position` (which must all exist).
    fn hash(&self, position: usize) -> usize {
        let start = position - self.history_start;
        let word = match self.history.get(start..start + 8) {
            Some(bytes) => u64::from_le_bytes(bytes.try_into().unwrap()),
            None => {
                let mut bytes = [0u8; 8];
                let tail = &self.history[start..];
                bytes[..tail.len()].copy_from_slice(tail);
                u64::from_le_bytes(bytes)
            }
        };
        let value = word << (64 - 8 * self.params.min_match as u32);
        (value.wrapping_mul(0x9E37_79B1_85EB_CA87) >> (64 - self.hash_log)) as usize
    }

    fn insert_until(&mut self, target: usize) {
        if self.head.is_empty() {
            return;
        }
        let last = self.history_end().saturating_sub(self.params.min_match);
        self.next_insert = self.next_insert.max(self.history_start);
        while self.next_insert < target && self.next_insert <= last {
            let position = self.next_insert;
            let hash = self.hash(position);
            let chain_index = position & ((1 << self.chain_log) - 1);
            self.chain[chain_index] = self.head[hash];
            self.head[hash] = (position - self.base + 1) as u32;
            self.next_insert += 1;
        }
    }

    /// Length of the common prefix at `candidate` and `position`, up to `limit`.
    fn match_len(&self, candidate: usize, position: usize, limit: usize) -> usize {
        let a = &self.history[candidate - self.history_start..];
        let b = &self.history[position - self.history_start..position - self.history_start + limit];
        let mut len = 0;
        for (x, y) in a.chunks_exact(8).zip(b.chunks_exact(8)) {
            if x != y {
                break;
            }
            len += 8;
        }
        len + a[len..]
            .iter()
            .zip(&b[len..])
            .take_while(|(x, y)| x == y)
            .count()
    }

    /// Estimated bits saved by a match (zstd's lazy heuristic): four per
    /// byte, minus the offset's cost.
    fn gain(&self, m: Match, literal_len: usize) -> i64 {
        let offset_cost = if self.offsets.is_repeat(m.offset as u32, literal_len as u32) {
            1
        } else {
            64 - ((m.offset as u64) + 3).leading_zeros() as i64
        };
        m.len as i64 * 4 - offset_cost
    }

    /// The best match starting at `position` before `end`.
    fn find(&self, position: usize, end: usize, literal_len: usize) -> Option<Match> {
        let limit = end - position;
        let lowest = self
            .history_start
            .max(position.saturating_sub(self.window()))
            .max(position.saturating_sub((1 << self.chain_log) - 1));
        let mut best: Option<Match> = None;
        let consider = |m: Match, best: &mut Option<Match>| {
            if m.len >= self.params.min_match
                && best.is_none_or(|b| self.gain(m, literal_len) > self.gain(b, literal_len))
            {
                *best = Some(m);
            }
        };

        // The repeat offsets first: cheap to encode.
        let [r0, r1, r2] = self.offsets.recent;
        let repeats = if literal_len > 0 {
            [r0, r1, r2]
        } else {
            [r1, r2, r0.saturating_sub(1)]
        };
        for &offset in repeats.iter() {
            let offset = offset as usize;
            if offset == 0 || offset > position - self.history_start || offset > self.window() {
                continue;
            }
            let len = self.match_len(position - offset, position, limit);
            consider(Match { len, offset }, &mut best);
        }

        let mut stored = self.head[self.hash(position)] as usize;
        let mut depth = self.params.search_depth;
        while stored != 0 && depth > 0 {
            let candidate = stored - 1 + self.base;
            if candidate < lowest || candidate >= position {
                break;
            }
            let longest = best.map_or(0, |b| b.len);
            // A longer match must also differ from the best one at its end.
            if longest < limit && self.byte(candidate + longest) == self.byte(position + longest) {
                let len = self.match_len(candidate, position, limit);
                consider(
                    Match {
                        len,
                        offset: position - candidate,
                    },
                    &mut best,
                );
                if best.is_some_and(|b| b.len >= self.params.target_len || b.len == limit) {
                    break;
                }
            }
            stored = self.chain[candidate & ((1 << self.chain_log) - 1)] as usize;
            depth -= 1;
        }
        best
    }

    /// Emit the sequences of the last committed block.
    pub(crate) fn start_matching(&mut self, mut handle_sequence: impl for<'a> FnMut(Sequence<'a>)) {
        let end = self.history_end();
        let min_match = self.params.min_match;
        let mut position = self.block_start;
        let mut anchor = position;
        while position + min_match <= end {
            self.insert_until(position);
            let Some(mut best) = self.find(position, end, position - anchor) else {
                position += 1 + ((position - anchor) >> SKIP_STRENGTH);
                continue;
            };
            // Lazy evaluation: a better match one position later wins.
            for _ in 0..self.params.lazy_depth {
                let next = position + 1;
                if next + min_match > end {
                    break;
                }
                self.insert_until(next);
                match self.find(next, end, next - anchor) {
                    Some(candidate)
                        if self.gain(candidate, next - anchor)
                            > self.gain(best, position - anchor) + 4 =>
                    {
                        position = next;
                        best = candidate;
                    }
                    _ => break,
                }
            }
            // Extend backwards into the pending literals.
            while position > anchor
                && best.offset < position - self.history_start
                && self.byte(position - 1) == self.byte(position - 1 - best.offset)
            {
                position -= 1;
                best.len += 1;
            }
            let literals_start = anchor - self.history_start;
            let literals_end = position - self.history_start;
            self.offsets
                .encode(best.offset as u32, (literals_end - literals_start) as u32);
            handle_sequence(Sequence::Triple {
                literals: &self.history[literals_start..literals_end],
                offset: best.offset,
                match_len: best.len,
            });
            position += best.len;
            anchor = position;
        }
        self.insert_until(end);
        if anchor < end {
            handle_sequence(Sequence::Literals {
                literals: &self.history[anchor - self.history_start..],
            });
        }
    }

    /// Index the last committed block without emitting matches (it is
    /// stored raw or as RLE).
    pub(crate) fn skip_matching(&mut self) {
        let end = self.history_end();
        self.insert_until(end);
    }
}

#[cfg(test)]
mod tests {
    use super::{HashChainMatcher, Params};
    use crate::encoding::{CompressionLevel, Sequence};
    use alloc::vec::Vec;

    /// Replays sequences the way the decoder executes them.
    fn replay(matcher: &mut HashChainMatcher, output: &mut Vec<u8>) {
        matcher.start_matching(|sequence| match sequence {
            Sequence::Literals { literals } => output.extend_from_slice(literals),
            Sequence::Triple {
                literals,
                offset,
                match_len,
            } => {
                output.extend_from_slice(literals);
                assert!(offset <= output.len(), "offset reaches before the data");
                assert!(match_len >= 3);
                let start = output.len() - offset;
                for i in 0..match_len {
                    output.push(output[start + i]);
                }
            }
        });
    }

    fn sample(len: usize) -> Vec<u8> {
        let words: [&[u8]; 6] = [
            b"alpha ",
            b"beta ",
            b"gamma\n",
            b"delta ",
            b"\x00\x01\x02",
            b"zeta ",
        ];
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut data = Vec::new();
        while data.len() < len {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            data.extend_from_slice(words[(seed % 6) as usize]);
            if seed.is_multiple_of(11) {
                data.push(seed as u8);
            }
        }
        data.truncate(len);
        data
    }

    #[test]
    fn sequences_reproduce_the_input_across_blocks() {
        for level in [CompressionLevel::Default, CompressionLevel::Better] {
            let mut matcher = HashChainMatcher::new(Params::for_level(level).unwrap());
            let data = sample(600_000);
            let mut output = Vec::new();
            let mut matched = 0;
            for (i, block) in data.chunks(128 * 1024).enumerate() {
                matcher.add_block(block);
                if i == 2 {
                    // A block stored raw is still part of the window.
                    matcher.skip_matching();
                    output.extend_from_slice(block);
                    continue;
                }
                let before = output.len();
                replay(&mut matcher, &mut output);
                assert_eq!(&output[before..], block);
                matched += 1;
            }
            assert_eq!(output, data);
            assert!(matched >= 4);
        }
    }

    #[test]
    fn repeated_data_becomes_few_long_matches() {
        let mut matcher =
            HashChainMatcher::new(Params::for_level(CompressionLevel::Better).unwrap());
        // An incompressible unit, repeated: one run of literals, then matches.
        let mut seed = 0x0123_4567_89AB_CDEFu64;
        let unit: Vec<u8> = (0..1000)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed as u8
            })
            .collect();
        let data: Vec<u8> = unit.iter().cycle().take(50_000).copied().collect();
        matcher.add_block(&data);
        let mut count = 0;
        let mut literal_bytes = 0;
        matcher.start_matching(|sequence| {
            count += 1;
            match sequence {
                Sequence::Literals { literals } => literal_bytes += literals.len(),
                Sequence::Triple { literals, .. } => literal_bytes += literals.len(),
            }
        });
        assert!(count < 10, "{} sequences", count);
        assert!(literal_bytes <= 1000);
    }

    #[test]
    fn short_inputs_and_tails_are_literals() {
        let mut matcher =
            HashChainMatcher::new(Params::for_level(CompressionLevel::Default).unwrap());
        matcher.add_block(b"abc");
        let mut output = Vec::new();
        replay(&mut matcher, &mut output);
        assert_eq!(output, b"abc");
    }

    /// A small window: history is trimmed and positions rebased many times,
    /// and no match reaches further back than the window.
    #[test]
    fn trimming_and_rebasing_keep_matches_valid() {
        let params = Params {
            window_log: 16,
            ..Params::for_level(CompressionLevel::Better).unwrap()
        };
        let mut matcher = HashChainMatcher::new(params);
        let data = sample(1_000_000);
        let mut output = Vec::new();
        for block in data.chunks(40_000) {
            matcher.add_block(block);
            matcher.rebase();
            matcher.start_matching(|sequence| match sequence {
                Sequence::Literals { literals } => output.extend_from_slice(literals),
                Sequence::Triple {
                    literals,
                    offset,
                    match_len,
                } => {
                    output.extend_from_slice(literals);
                    assert!(offset <= 1 << 16, "offset {} beyond the window", offset);
                    let start = output.len() - offset;
                    for i in 0..match_len {
                        output.push(output[start + i]);
                    }
                }
            });
        }
        assert_eq!(output, data);
        assert!(matcher.history_start > 0 && matcher.base > 0);
    }
}
