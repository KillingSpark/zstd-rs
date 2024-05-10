use super::{decodebuffer::DecodebufferError, scratch::DecoderScratch};

#[derive(Debug)]
#[non_exhaustive]
pub enum ExecuteSequencesError {
    DecodebufferError(DecodebufferError),
    NotEnoughBytesForSequence { wanted: usize, have: usize },
    ZeroOffset,
}

impl core::fmt::Display for ExecuteSequencesError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ExecuteSequencesError::DecodebufferError(e) => {
                write!(f, "{:?}", e)
            }
            ExecuteSequencesError::NotEnoughBytesForSequence { wanted, have } => {
                write!(
                    f,
                    "Sequence wants to copy up to byte {}. Bytes in literalsbuffer: {}",
                    wanted, have
                )
            }
            ExecuteSequencesError::ZeroOffset => {
                write!(f, "Illegal offset: 0 found")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ExecuteSequencesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExecuteSequencesError::DecodebufferError(source) => Some(source),
            _ => None,
        }
    }
}

impl From<DecodebufferError> for ExecuteSequencesError {
    fn from(val: DecodebufferError) -> Self {
        Self::DecodebufferError(val)
    }
}

pub fn execute_sequences(scratch: &mut DecoderScratch) -> Result<(), ExecuteSequencesError> {
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
    }
    if literals_copy_counter < scratch.literals_buffer.len() {
        let rest_literals = &scratch.literals_buffer[literals_copy_counter..];
        scratch.buffer.push(rest_literals);
        seq_sum += rest_literals.len() as u32;
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
            3 => scratch[0] - 1,
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
