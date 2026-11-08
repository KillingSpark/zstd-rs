//! Repeat offsets on the encoding side (RFC 8878, section 3.1.1.5).
//!
//! A sequence can reference one of the three most recent offsets with a short
//! code (1 to 3) instead of the full offset (`offset + 3`), which is much
//! cheaper to encode. The decoder keeps the history across the blocks of a
//! frame, so the encoder mirrors it exactly.

/// The three most recent offsets, as the decoder tracks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OffsetHistory {
    pub(crate) recent: [u32; 3],
}

impl OffsetHistory {
    /// The history at the start of every frame.
    pub(crate) const fn new() -> Self {
        Self { recent: [1, 4, 8] }
    }

    /// The offset value (`Offset_Value` in the RFC) that encodes a match
    /// `offset` bytes back after `literal_len` literals. The history is
    /// updated the way the decoder will update it.
    pub(crate) fn encode(&mut self, offset: u32, literal_len: u32) -> u32 {
        debug_assert!(offset > 0);
        let [r0, r1, r2] = self.recent;
        let value = if literal_len > 0 {
            if offset == r0 {
                1
            } else if offset == r1 {
                2
            } else if offset == r2 {
                3
            } else {
                offset + 3
            }
        } else if offset == r1 {
            1
        } else if offset == r2 {
            2
        } else if r0 > 1 && offset == r0 - 1 {
            3
        } else {
            offset + 3
        };
        // The decoder keeps the history unchanged only when the most recent
        // offset is repeated; otherwise the used offset moves to the front.
        let keep_order = literal_len > 0 && value == 1;
        if !keep_order {
            let moved_from_second =
                (literal_len > 0 && value == 2) || (literal_len == 0 && value == 1);
            self.recent = if moved_from_second {
                [offset, r0, r2]
            } else {
                [offset, r0, r1]
            };
        }
        value
    }

    /// Whether `offset` is one of the offsets a repeat code can reach after
    /// `literal_len` literals.
    pub(crate) fn is_repeat(&self, offset: u32, literal_len: u32) -> bool {
        let [r0, r1, r2] = self.recent;
        if literal_len > 0 {
            offset == r0 || offset == r1 || offset == r2
        } else {
            offset == r1 || offset == r2 || (r0 > 1 && offset == r0 - 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OffsetHistory;
    use crate::decoding::sequence_execution::do_offset_history;

    /// Every code the encoder picks decodes back to the same offset, and both
    /// sides end with the same history.
    #[test]
    fn encoder_and_decoder_histories_agree() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut encoder = OffsetHistory::new();
        let mut decoder = [1, 4, 8];
        let mut repeats = 0;
        for _ in 0..100_000 {
            let r = next();
            // Mostly offsets near the recent ones, so every repeat case occurs.
            let offset = match r % 6 {
                0 => encoder.recent[0],
                1 => encoder.recent[1],
                2 => encoder.recent[2],
                3 => encoder.recent[0].saturating_sub(1).max(1),
                _ => 1 + (r >> 8) as u32 % 64,
            };
            let literal_len = ((r >> 40) % 3) as u32;
            let value = encoder.encode(offset, literal_len);
            if value <= 3 {
                repeats += 1;
            }
            assert_eq!(do_offset_history(value, literal_len, &mut decoder), offset);
            assert_eq!(encoder.recent, decoder);
        }
        assert!(repeats > 10_000);
    }

    #[test]
    fn repeat_reachability_matches_the_codes() {
        let mut history = OffsetHistory::new();
        history.encode(100, 5);
        history.encode(200, 5);
        // recent is now [200, 100, 1]
        assert!(history.is_repeat(200, 3));
        assert!(!history.is_repeat(200, 0));
        assert!(history.is_repeat(199, 0));
        assert!(history.is_repeat(1, 0));
        assert!(!history.is_repeat(50, 2));
    }
}
