use super::scratch::DecoderScratch;

pub fn execute_sequences(scratch: &mut DecoderScratch) {
    let mut literals_copy_counter = 0;
    for seq in &scratch.sequences {
        //println!("{}", seq);
        if seq.ll > 0 {
            let literals = &scratch.literals_buffer
                [literals_copy_counter..literals_copy_counter + seq.ll as usize];
            literals_copy_counter += seq.ll as usize;
            scratch.buffer.push(literals);
        }

        if seq.ml > 0 {
            assert!(seq.of > 0);
            let actual_offset = do_offset_history(seq.of, seq.ll, &mut scratch.offset_hist);
            scratch
                .buffer
                .repeat(actual_offset as usize, seq.ml as usize);
        }
    }
    if literals_copy_counter < scratch.literals_buffer.len() {
        let rest_literals = &scratch.literals_buffer[literals_copy_counter..];
        scratch.buffer.push(rest_literals);
    }
}

fn do_offset_history(offset_value: u32, lit_len: u32, scratch: &mut [u32; 3]) -> u32 {
    let actual_offset = if lit_len > 0 {
        match offset_value {
            1...3 => scratch[offset_value as usize - 1],
            _ => {
                //new offset
                offset_value - 3
            }
        }
    } else {
        match offset_value {
            1...2 => scratch[offset_value as usize],
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
                scratch[0] = scratch[1];
                scratch[0] = actual_offset;
            }
            _ => {
                scratch[2] = scratch[1];
                scratch[1] = scratch[0];
                scratch[0] = actual_offset;
            }
        }
    }else{
        match offset_value {
            1 => {
                scratch[0] = scratch[1];
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
