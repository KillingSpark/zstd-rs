use super::super::blocks::sequence_section::Sequence;
use super::scratch::SequenceExecutionScratch;
use crate::decoding::errors::ExecuteSequencesError;

/// Take the provided decoder and execute the sequences stored within
pub fn execute_sequence(
    scratch: &mut SequenceExecutionScratch,
    seq: Sequence,
) -> Result<(), ExecuteSequencesError> {
    if seq.ll > 0 {
        let high = scratch.literals_copy_counter + seq.ll as usize;
        if high > scratch.literals_buffer.len() {
            return Err(ExecuteSequencesError::NotEnoughBytesForSequence {
                wanted: high,
                have: scratch.literals_buffer.len(),
            });
        }
        let literals = &scratch.literals_buffer[scratch.literals_copy_counter..high];
        scratch.literals_copy_counter += seq.ll as usize;

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
    use super::do_offset_history;

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
