use super::scratch::DecoderScratch;
use crate::decoding::errors::ExecuteSequencesError;

/// Take the provided decoder and execute the sequences stored within
pub fn execute_sequences(
    scratch: &mut DecoderScratch,
    max_block_decoded_len: usize,
) -> Result<(), ExecuteSequencesError> {
    // Every literal is emitted once; matches add their lengths on top.
    // RFC 8878 3.1.1.2.4 bounds the entire block, so reject before any
    // sequence changes the history buffer or repeat-offset state.
    let regenerated = scratch
        .sequences
        .iter()
        .try_fold(scratch.literals_buffer.len(), |size, seq| {
            size.checked_add(seq.ml as usize)
        });
    if regenerated.is_none_or(|size| size > max_block_decoded_len) {
        return Err(ExecuteSequencesError::TooManyBytesGenerated {
            max: max_block_decoded_len,
            decoded: regenerated.unwrap_or(usize::MAX),
        });
    }

    let mut literals_copy_counter = 0;
    let old_buffer_size = scratch.buffer.len();
    let mut seq_sum = 0;

    for idx in 0..scratch.sequences.len() {
        let seq = scratch.sequences[idx];

        if seq.ll > 0 {
            let high = literals_copy_counter + seq.ll as usize;
            if high > scratch.literals_buffer.len() {
                return Err(ExecuteSequencesError::NotEnoughBytesForSequence {
                    wanted: high,
                    have: scratch.literals_buffer.len(),
                });
            }
            let literals = &scratch.literals_buffer[literals_copy_counter..high];
            literals_copy_counter += seq.ll as usize;

            scratch.buffer.push(literals);
        }

        let actual_offset = do_offset_history(seq.of, seq.ll, &mut scratch.offset_hist);
        if actual_offset == 0 {
            return Err(ExecuteSequencesError::ZeroOffset);
        }
        if seq.ml > 0 {
            scratch
                .buffer
                .repeat(actual_offset as usize, seq.ml as usize)?;
        }

        seq_sum += seq.ml;
        seq_sum += seq.ll;

        if seq_sum as usize > max_block_decoded_len {
            return Err(ExecuteSequencesError::TooManyBytesGenerated {
                max: max_block_decoded_len,
                decoded: seq_sum as usize,
            });
        }
    }
    if literals_copy_counter < scratch.literals_buffer.len() {
        let rest_literals = &scratch.literals_buffer[literals_copy_counter..];
        scratch.buffer.push(rest_literals);
        seq_sum += rest_literals.len() as u32;
        if seq_sum as usize > max_block_decoded_len {
            return Err(ExecuteSequencesError::TooManyBytesGenerated {
                max: max_block_decoded_len,
                decoded: seq_sum as usize,
            });
        }
    }

    let diff = scratch.buffer.len() - old_buffer_size;
    assert!(
        seq_sum as usize == diff,
        "Seq_sum: {} is different from the difference in buffersize: {}",
        seq_sum,
        diff
    );
    Ok(())
}

/// Update the most recently used offsets to reflect the provided offset value, and return the
/// "actual" offset needed because offsets are not stored in a raw way, some transformations are needed
/// before you get a functional number.
fn do_offset_history(offset_value: u32, lit_len: u32, scratch: &mut [u32; 3]) -> u32 {
    let actual_offset = if lit_len > 0 {
        match offset_value {
            1..=3 => scratch[offset_value as usize - 1],
            _ => {
                //new offset
                offset_value - 3
            }
        }
    } else {
        match offset_value {
            1..=2 => scratch[offset_value as usize],
            // A malformed dictionary can seed scratch[0] with 0; saturate so this
            // resolves to 0 (rejected upstream as ZeroOffset) instead of
            // underflowing. See #115.
            3 => scratch[0].saturating_sub(1),
            _ => {
                //new offset
                offset_value - 3
            }
        }
    };

    //update history
    if lit_len > 0 {
        match offset_value {
            1 => {
                //nothing
            }
            2 => {
                scratch[1] = scratch[0];
                scratch[0] = actual_offset;
            }
            _ => {
                scratch[2] = scratch[1];
                scratch[1] = scratch[0];
                scratch[0] = actual_offset;
            }
        }
    } else {
        match offset_value {
            1 => {
                scratch[1] = scratch[0];
                scratch[0] = actual_offset;
            }
            2 => {
                scratch[2] = scratch[1];
                scratch[1] = scratch[0];
                scratch[0] = actual_offset;
            }
            _ => {
                scratch[2] = scratch[1];
                scratch[1] = scratch[0];
                scratch[0] = actual_offset;
            }
        }
    }

    actual_offset
}

#[cfg(test)]
mod tests {
    use super::{do_offset_history, execute_sequences};
    use crate::blocks::sequence_section::Sequence;
    use crate::decoding::errors::ExecuteSequencesError;
    use crate::decoding::scratch::DecoderScratch;

    #[test]
    fn oversized_sequences_are_rejected_before_history_growth() {
        let mut scratch = DecoderScratch::new(1024);
        scratch.buffer.push(b"AAAA");
        // Each sequence is individually below the cap. The second makes
        // the whole block too large, so even the first must not execute.
        scratch.sequences.extend([
            Sequence {
                ll: 0,
                ml: 600,
                of: 4,
            },
            Sequence {
                ll: 0,
                ml: 600,
                of: 4,
            },
        ]);
        let offsets = scratch.offset_hist;
        let result = execute_sequences(&mut scratch, 1024);
        assert!(matches!(
            result,
            Err(ExecuteSequencesError::TooManyBytesGenerated {
                max: 1024,
                decoded: 1200,
            })
        ));
        assert_eq!(scratch.buffer.len(), 4);
        assert_eq!(scratch.offset_hist, offsets);
    }

    #[test]
    fn sequence_output_at_block_maximum_is_accepted() {
        let mut scratch = DecoderScratch::new(1024);
        scratch.buffer.push(b"AAAA");
        scratch.literals_buffer.extend_from_slice(b"B");
        scratch.sequences.push(Sequence {
            ll: 0,
            ml: 1023,
            of: 4,
        });
        execute_sequences(&mut scratch, 1024).unwrap();
        assert_eq!(scratch.buffer.len(), 4 + 1024);
    }

    #[test]
    fn repeat_offset_minus_one_with_zero_history_does_not_underflow() {
        // A malformed dictionary can seed offset history slot 0 with 0. With
        // literal length 0 and offset code 3 ("repeat the most recent offset,
        // minus one"), `scratch[0] - 1` must not underflow; it should resolve to
        // 0, which the caller rejects as ExecuteSequencesError::ZeroOffset rather
        // than panicking (debug) or wrapping to u32::MAX (release). See #115.
        let mut scratch = [0u32, 4, 8];
        assert_eq!(do_offset_history(3, 0, &mut scratch), 0);
    }
}
