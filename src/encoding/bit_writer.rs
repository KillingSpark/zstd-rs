//! Use [BitWriter] to write an arbitrary amount of bits into a buffer.
use alloc::vec;
use alloc::vec::Vec;

/// An interface for writing an arbitrary number of bits into a buffer
pub(crate) struct BitWriter {
    /// The buffer that's filled with bits
    output: Vec<u8>,
    /// The index pointing to the next unoccupied bit. Effectively just
    /// the number of bits that have been written into the buffer so far.
    bit_idx: usize,
}

#[derive(Debug, PartialEq)]
pub enum BitWriterError {
    NotByteAligned,
    MoreBitsThanInbuf,
}

impl core::fmt::Display for BitWriterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BitWriterError::NotByteAligned => {
                write!(
                    f,
                    "The number of bits written into the buffer does not cleanly divide by 8."
                )
            }
            BitWriterError::MoreBitsThanInbuf => {
                write!(
                    f,
                    "Asked to write more bits into buffer than were provided by the `bits` buffer."
                )
            }
        }
    }
}

impl BitWriter {
    /// Initialize a new writer. Write new bits into the buffer with `write_bits`, and
    /// obtain the output using `dump`.
    pub fn new() -> Self {
        Self {
            output: Vec::new(),
            bit_idx: 0,
        }
    }

    /// Write `num_bits` from `bits` into the writer, returning the number of bits
    /// read.
    ///
    /// `num_bits` refers to how many bits starting from the *least significant position*,
    /// but the bits will be written starting from the *most significant position*, continuing
    /// to the least significant position.
    ///
    /// It's up to the caller to ensure that any in the cursor beyond `num_bits` is always zero.
    /// If it's not, the output buffer will be corrupt.
    ///
    /// TODO: example usage
    pub fn write_bits(&mut self, bits: &[u8], num_bits: usize) -> usize {
        // Special handling for if both the input and output are byte aligned
        if self.bit_idx % 8 == 0 && num_bits / 8 == bits.len() {
            self.output.extend_from_slice(bits);
            self.bit_idx += num_bits;
            return num_bits;
        }

        // Make sure there's space for the new bits by finding the total size of the buffer in bits, then round up to the nearest multiple of 8
        // to find how many *bytes* that would occupy. After that, expand the vec to the new size.
        let new_size_of_output = (self.bit_idx + num_bits + 7) >> 3;
        let size_of_extension = new_size_of_output - self.output.len();
        let new_chunk: Vec<u8> = vec![0; size_of_extension];
        self.output.extend(new_chunk);

        // We will never need to operate across a byte boundary in a single iteration of the loop.
        let mut num_bits_written: usize = 0;
        while num_bits_written < num_bits {
            // The number of unoccupied bits in the output buffer
            // byte that the cursor is currently indexed into
            let num_bits_left_in_output_byte = 8 - (self.bit_idx % 8);
            // The number of bits left to write in the currently selected input buffer byte
            let mut num_bits_left_in_input_byte = (num_bits - num_bits_written) % 8;
            if num_bits_left_in_input_byte == 0 {
                num_bits_left_in_input_byte = 8;
            }
            // The byte that we're currently reading from in the input
            let input_byte_index: usize = num_bits_written / 8;
            let byte_index_to_update = self.bit_idx / 8;
            if num_bits_left_in_output_byte >= num_bits_left_in_input_byte {
                // Case 1: We read from the input until the next input byte boundary (or end of data), because
                // there's more free space in the output byte then there are bits to read in the input byte.

                // In the below example, we're adding
                // 0b111 to a buffer, then adding 0b000.
                // Because we start from the left, to position
                // 0b111 in the correct position, we want the
                // leftmost bit to be at index 7, and the rightmost
                // bit to be in position 5. To achieve this, you can
                // shift 0b111 over 5 times.
                //
                // 76543210 ◄─── Bit Index
                // 111◄──── Move 0b111 to the left 5 slots so that it
                //          occupies the leftmost space
                // The formula for this would look like (8 - num_bits_added).
                // Then, to write 0b000 into the buffer, we can use the same
                // formula again, but we need to account for the number of bits
                // already written into the buffer. This means our new formula looks
                // like (8 - num_bits_added - num_bits_already_in_buffer). In this case
                // there are 3 bits already in the buffer, and we're writing in 3 bits,
                // so (8 - 3 - 3) = 2.
                //
                // 111◄──── Data already in buffer
                //    000◄─ New data being added into the buffer
                // Then to determine what the final buffer looks like, we can simply OR
                // the two buffers together.
                // 111─────  ◄── The lines mark "Unoccupied space", so they'd just be zeros
                // ───000──
                //
                // 111000──  ◄── The final buffer

                let num_bits_already_in_byte = 8 - num_bits_left_in_output_byte;
                let num_bits_being_added = (num_bits - num_bits_written) % 8;
                if num_bits_left_in_input_byte == 8 && num_bits_left_in_output_byte == 8 {
                    // In this case, we're trying to shift all the way over to the next byte, so just update that next byte
                    self.output[byte_index_to_update] = bits[input_byte_index];
                    num_bits_written += 8;
                    self.bit_idx += 8;
                    continue;
                }
                // Shift the bits left
                let num_spots_to_move_left = 8 - num_bits_being_added - num_bits_already_in_byte;
                // Combine it with the existing data
                let aligned_byte = bits[input_byte_index] << num_spots_to_move_left;
                let merged_byte = self.output[byte_index_to_update] | aligned_byte;
                // Write changes to the output buffer
                self.output[byte_index_to_update] = merged_byte;

                // Advance the bit cursor forwards and update
                // the number of bits being added
                num_bits_written += num_bits_being_added;
                self.bit_idx += num_bits_being_added;
            } else {
                // Case 2: There's not enough free space in the output byte to read till the next input byte boundary, so we
                // read to the next output byte boundary.

                // This looks like reading from input bit index onwards N bits, where N is the number of free bits available in the output byte
                //
                // In the below example, we've already written 3 0s into the buffer, but we want to write
                // 6 1s into the buffer.
                //
                // 76543210◄─── Bit Index
                // 111 ◄─────── Data already in buffer
                //    000000◄── Data we want to add to the buffer (not yet aligned).
                //
                // You'll note that we can't do the same thing we did last time, because we have more data
                // than will fit into the byte, so we need do this in multiple passes, writing data up to the boundary,
                // then writing data into the next byte. Getting that final bit can happen on the next pass, using the first case, where
                // we read until an input byte boundary.
                // Broken down into steps, this looks something like this:
                //
                //  ◄──00000X Because there may be arbitrary data behind the cursor in the
                //            input data, we need to shift left, then right, to mask out that data
                //            and ensure it's all zeros (so that when we OR with the output, we don't corrupt it).
                //            Here, I've replaced that last 0 with an X because it's in the next byte, so it's ignored
                //            until the next pass. The amount we shift left will depend on how far into the input byte
                //            the input cursor is.
                //
                //  ──►00000X Next we move that data to the right N spaces, where N is the number of bits already occupied
                //            in the current byte. In the example, that would be 3.
                //  Our value is now masked and aligned, so we can merge it with the currently selected output byte
                //  and update it, then advance the output and input cursors 8 - N bits, again, where N is the amount
                //  of bits already occupied in the buffer.

                // Shift the bits left to zero out any data behind the read cursor
                let num_spots_to_move_left = (8 - num_bits_left_in_input_byte) % 8;
                let masked_byte = bits[input_byte_index] << num_spots_to_move_left;
                // Shift the bits right so that the data is inserted into the next free spot
                let aligned_byte = masked_byte >> (self.bit_idx % 8);
                // // Combine our newly aligned byte with the output byte
                let merged_byte = self.output[byte_index_to_update] | aligned_byte;
                // Write changes to the output buffer
                self.output[byte_index_to_update] = merged_byte;
                // Advance the bit cursor forwards and update
                // the number of bits being added
                num_bits_written += num_bits_left_in_output_byte;
                self.bit_idx += num_bits_left_in_output_byte;
            }
        }
        num_bits_written
    }

    /// Returns the populated buffer that you've been writing bits into.
    ///
    /// This function consumes the writer, so it cannot be used after
    /// dumping
    pub fn dump(self) -> Result<Vec<u8>, BitWriterError> {
        if self.bit_idx % 8 != 0 {
            return Err(BitWriterError::NotByteAligned);
        }
        Ok(self.output)
    }
}

#[cfg(test)]
mod tests {
    use super::BitWriter;
    use crate::encoding::bit_writer::BitWriterError;
    use std::vec;

    #[test]
    fn single_byte_written_4_4() {
        // Write the first 4 bits as 1s and the last 4 bits as 0s
        // 1010 is used where values should never be read from.
        let mut bw: BitWriter = BitWriter::new();
        bw.write_bits(&[0b010_1111], 4);
        bw.write_bits(&[0b1010_0000], 4);
        let output = bw.dump().unwrap();
        assert!(output.len() == 1, "Single byte written into writer returned a vec that wasn't one byte, vec was {} elements long", output.len());
        assert_eq!(
            0b1111_0000, output[0],
            "4 bits and 4 bits written into buffer"
        );
    }

    #[test]
    fn single_byte_written_3_5() {
        // Write the first 3 bits as 1s and the last 5 bits as 0s
        let mut bw: BitWriter = BitWriter::new();
        bw.write_bits(&[0b0101_0111], 3);
        bw.write_bits(&[0b1010_0000], 5);
        let output = bw.dump().unwrap();
        assert!(output.len() == 1, "Single byte written into writer return a vec that wasn't one byte, vec was {} elements long", output.len());
        assert_eq!(0b1110_0000, output[0], "3 and 5 bits written into buffer");
    }

    #[test]
    fn single_byte_written_1_7() {
        // Write the first bit as a 1 and the last 7 bits as 0s
        let mut bw: BitWriter = BitWriter::new();
        bw.write_bits(&[0b1], 1);
        bw.write_bits(&[0], 7);
        let output = bw.dump().unwrap();
        assert!(output.len() == 1, "Single byte written into writer return a vec that wasn't one byte, vec was {} elements long", output.len());
        assert_eq!(0b1000_0000, output[0], "1 and 7 bits written into buffer");
    }

    #[test]
    fn single_byte_written_8() {
        // Write an entire byte
        let mut bw: BitWriter = BitWriter::new();
        bw.write_bits(&[1], 8);
        let output = bw.dump().unwrap();
        assert!(output.len() == 1, "Single byte written into writer return a vec that wasn't one byte, vec was {} elements long", output.len());
        assert_eq!(1, output[0], "1 and 7 bits written into buffer");
    }

    #[test]
    fn multi_byte_clean_boundary_4_4_4_4() {
        // Writing 4 bits at a time for 2 bytes
        let mut bw = BitWriter::new();
        bw.write_bits(&[0], 4);
        bw.write_bits(&[0b1111], 4);
        bw.write_bits(&[0b1111], 4);
        bw.write_bits(&[0], 4);
        assert_eq!(vec![0b0000_1111, 0b1111_0000], bw.dump().unwrap());
    }

    #[test]
    fn multi_byte_clean_boundary_16_8() {
        // Writing 16 bits at once
        let mut bw = BitWriter::new();
        bw.write_bits(&[1, 0], 16);
        bw.write_bits(&[69], 8);
        assert_eq!(vec![1, 0, 69], bw.dump().unwrap())
    }

    #[test]
    fn multi_byte_boundary_crossed_4_12() {
        // Writing 4 1s and then 12 zeros
        let mut bw = BitWriter::new();
        bw.write_bits(&[0b0000_1111], 4);
        bw.write_bits(&[0b0000_0000, 0b1010_0000], 12);
        assert_eq!(vec![0b1111_0000, 0b0000_0000], bw.dump().unwrap());
    }

    #[test]
    fn multi_byte_boundary_crossed_4_5_7() {
        // Writing 4 1s and then 5 zeros then 7 1s
        let mut bw = BitWriter::new();
        bw.write_bits(&[0b1010_1111], 4);
        bw.write_bits(&[0b1010_0000], 5);
        bw.write_bits(&[0b0111_1111], 7);
        assert_eq!(vec![0b1111_0000, 0b0111_1111], bw.dump().unwrap());
    }

    #[test]
    fn multi_byte_boundary_crossed_1_9_6() {
        // Writing 1 1 and then 9 zeros then 6 1s
        let mut bw = BitWriter::new();
        bw.write_bits(&[0b0000_0001], 1);
        bw.write_bits(&[0, 0b1010_1010], 9);
        bw.write_bits(&[0b0011_1111], 6);
        assert_eq!(vec![0b1000_0000, 0b0011_1111], bw.dump().unwrap());
    }

    #[test]
    fn catches_unaligned_dump() {
        // Write a single bit in then dump it, making sure
        // the correct error is returned
        let mut bw = BitWriter::new();
        bw.write_bits(&[0], 1);
        assert_eq!(Err(BitWriterError::NotByteAligned), bw.dump());
    }

    // #[test]
    // fn catches_more_than_in_buf() {
    //     todo!();
    // }
}
