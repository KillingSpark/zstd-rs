//! Use [BitWriter] to write an arbitrary amount of bits into a buffer.
use alloc::vec::Vec;

/// An interface for writing an arbitrary number of bits into a buffer. Write new bits into the buffer with `write_bits`, and
/// obtain the output using `dump`.
#[derive(Debug)]
pub(crate) struct BitWriter {
    /// The buffer that's filled with bits
    output: Vec<u8>,
    /// holds a partially filled byte which gets put in outpu when it's fill with a write_bits call
    partial: u8,
    /// The index pointing to the next unoccupied bit. Effectively just
    /// the number of bits that have been written into the buffer so far.
    bit_idx: usize,
}

impl BitWriter {
    /// Initialize a new writer.
    pub fn new() -> Self {
        Self {
            output: Vec::new(),
            partial: 0,
            bit_idx: 0,
        }
    }

    /// Wrap a writer around an existing vec.
    ///
    /// Currently unused, but will almost certainly be used later upon further optimizing
    #[allow(unused)]
    pub fn from(buf: Vec<u8>) -> Self {
        Self {
            bit_idx: buf.len() * 8,
            output: buf,
            partial: 0,
        }
    }

    pub fn write_bits(&mut self, bits: impl Into<u64>, num_bits: usize) {
        self.write_bits_64(bits.into(), num_bits);
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
    /// Refer to tests for example usage.
    // TODO: Because bitwriter isn't directly public, any errors would be caused by internal library bugs,
    // and so this function should just panic if it encounters issues.
    pub fn write_bits_64(&mut self, bits: u64, num_bits: usize) {
        if num_bits > 64 {
            panic!(
                "asked to write more than 64 bits into buffer ({})",
                num_bits
            );
        }

        if bits > 0 {
            assert!(bits.ilog2() <= num_bits as u32);
        }
        // Special handling for if both the input and output are byte aligned
        if self.bit_idx % 8 == 0 && num_bits % 8 == 0 {
            self.output
                .extend_from_slice(&bits.to_le_bytes()[..num_bits / 8]);
            self.bit_idx += num_bits;
            return;
        }

        // fill partial byte first
        let bits_free_in_partial = self.misaligned();
        if bits_free_in_partial > 0 {
            if num_bits >= bits_free_in_partial {
                let mask = (1 << bits_free_in_partial) - 1;
                let part = (bits & mask) << (8 - bits_free_in_partial);
                debug_assert!(part <= 256);
                let merged = self.partial | part as u8;
                self.output.push(merged);
                self.partial = 0;
                self.bit_idx += bits_free_in_partial;
            } else {
                let part = bits << (8 - bits_free_in_partial);
                debug_assert!(part <= 256);
                let merged = self.partial | part as u8;
                self.partial = merged;
                self.bit_idx += num_bits;
                return;
            }
        }
        let mut num_bits = num_bits - bits_free_in_partial;
        let mut bits = bits >> bits_free_in_partial;

        while num_bits / 8 > 0 {
            let byte = bits as u8;
            self.output.push(byte);
            num_bits -= 8;
            self.bit_idx += 8;
            bits >>= 8;
        }

        debug_assert!(num_bits < 8);
        if num_bits > 0 {
            let mask = (1 << num_bits) - 1;
            self.partial = (bits & mask) as u8;
        }
        self.bit_idx += num_bits;
    }

    /// Returns the populated buffer that you've been writing bits into.
    ///
    /// This function consumes the writer, so it cannot be used after
    /// dumping
    pub fn dump(self) -> Vec<u8> {
        if self.bit_idx % 8 != 0 {
            panic!("`dump` was called on a bit writer but an even number of bytes weren't written into the buffer. Was: {}", self.bit_idx)
        }
        debug_assert_eq!(self.partial, 0);
        self.output
    }

    /// Returns how many bits are missing for an even byte
    pub fn misaligned(&self) -> usize {
        if self.bit_idx % 8 == 0 {
            0
        } else {
            8 - (self.bit_idx % 8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BitWriter;
    use alloc::vec;

    #[test]
    fn from_existing() {
        // Define an existing vec, write some bits into it
        let existing_vec = vec![255_u8];
        let mut bw = BitWriter::from(existing_vec);
        bw.write_bits(0u8, 8);
        assert_eq!(vec![255, 0], bw.dump());
    }

    #[test]
    fn single_byte_written_4_4() {
        // Write the first 4 bits as 1s and the last 4 bits as 0s
        // 1010 is used where values should never be read from.
        let mut bw: BitWriter = BitWriter::new();
        bw.write_bits(0b1111u8, 4);
        bw.write_bits(0b0000u8, 4);
        let output = bw.dump();
        assert!(output.len() == 1, "Single byte written into writer returned a vec that wasn't one byte, vec was {} elements long", output.len());
        assert_eq!(
            0b0000_1111, output[0],
            "4 bits and 4 bits written into buffer"
        );
    }

    #[test]
    fn single_byte_written_3_5() {
        // Write the first 3 bits as 1s and the last 5 bits as 0s
        let mut bw: BitWriter = BitWriter::new();
        bw.write_bits(0b111u8, 3);
        bw.write_bits(0b0_0000u8, 5);
        let output = bw.dump();
        assert!(output.len() == 1, "Single byte written into writer return a vec that wasn't one byte, vec was {} elements long", output.len());
        assert_eq!(0b0000_0111, output[0], "3 and 5 bits written into buffer");
    }

    #[test]
    fn single_byte_written_1_7() {
        // Write the first bit as a 1 and the last 7 bits as 0s
        let mut bw: BitWriter = BitWriter::new();
        bw.write_bits(0b1u8, 1);
        bw.write_bits(0u8, 7);
        let output = bw.dump();
        assert!(output.len() == 1, "Single byte written into writer return a vec that wasn't one byte, vec was {} elements long", output.len());
        assert_eq!(0b0000_0001, output[0], "1 and 7 bits written into buffer");
    }

    #[test]
    fn single_byte_written_8() {
        // Write an entire byte
        let mut bw: BitWriter = BitWriter::new();
        bw.write_bits(1u8, 8);
        let output = bw.dump();
        assert!(output.len() == 1, "Single byte written into writer return a vec that wasn't one byte, vec was {} elements long", output.len());
        assert_eq!(1, output[0], "1 and 7 bits written into buffer");
    }

    #[test]
    fn multi_byte_clean_boundary_4_4_4_4() {
        // Writing 4 bits at a time for 2 bytes
        let mut bw = BitWriter::new();
        bw.write_bits(0u8, 4);
        bw.write_bits(0b1111u8, 4);
        bw.write_bits(0b1111u8, 4);
        bw.write_bits(0u8, 4);
        assert_eq!(vec![0b1111_0000, 0b0000_1111], bw.dump());
    }

    #[test]
    fn multi_byte_clean_boundary_16_8() {
        // Writing 16 bits at once
        let mut bw = BitWriter::new();
        bw.write_bits(0x0100u16, 16);
        bw.write_bits(69u8, 8);
        assert_eq!(vec![0, 1, 69], bw.dump())
    }

    #[test]
    fn multi_byte_boundary_crossed_4_12() {
        // Writing 4 1s and then 12 zeros
        let mut bw = BitWriter::new();
        bw.write_bits(0b1111u8, 4);
        bw.write_bits(0b0000_0011_0100_0010u16, 12);
        assert_eq!(vec![0b0010_1111, 0b0011_0100], bw.dump());
    }

    #[test]
    fn multi_byte_boundary_crossed_4_5_7() {
        // Writing 4 1s and then 5 zeros then 7 1s
        let mut bw = BitWriter::new();
        bw.write_bits(0b1111u8, 4);
        bw.write_bits(0b0_0000u8, 5);
        bw.write_bits(0b111_1111u8, 7);
        assert_eq!(vec![0b0000_1111, 0b1111_1110], bw.dump());
    }

    #[test]
    fn multi_byte_boundary_crossed_1_9_6() {
        // Writing 1 1 and then 9 zeros then 6 1s
        let mut bw = BitWriter::new();
        bw.write_bits(0b1u8, 1);
        bw.write_bits(0b0_0000_0000u16, 9);
        bw.write_bits(0b11_1111u8, 6);
        assert_eq!(vec![0b0000_0001, 0b1111_1100], bw.dump());
    }

    #[test]
    #[should_panic]
    fn catches_unaligned_dump() {
        // Write a single bit in then dump it, making sure
        // the correct error is returned
        let mut bw = BitWriter::new();
        bw.write_bits(0u8, 1);
        bw.dump();
    }

    #[test]
    #[should_panic]
    fn catches_dirty_upper_bits() {
        let mut bw = BitWriter::new();
        bw.write_bits(10u8, 1);
    }

    #[test]
    fn add_multiple_aligned() {
        let mut bw = BitWriter::new();
        bw.write_bits(0x00_0F_F0_FFu32, 32);
        assert_eq!(vec![0xFF, 0xF0, 0x0F, 0x00], bw.dump());
    }

    // #[test]
    // fn catches_more_than_in_buf() {
    //     todo!();
    // }
}
