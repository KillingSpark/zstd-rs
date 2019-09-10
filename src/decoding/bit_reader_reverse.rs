pub struct BitReaderReversed<'s> {
    idx: usize, //index counts bits already read
    source: &'s [u8],
}

impl<'s> BitReaderReversed<'s> {
    pub fn bits_remaining(&self) -> isize {
        self.idx as isize
    }

    pub fn new(source: &'s [u8]) -> BitReaderReversed {
        BitReaderReversed {
            idx: source.len() * 8,
            source: source,
        }
    }

    pub fn return_bits(&mut self, n: usize) {
        if n > self.idx {
            panic!("Cant return this many bits");
        }
        self.idx -= n;
    }

    fn byte_idx(&self) -> usize {
        ((self.idx - 1) / 8)
    }

    pub fn get_bits(&mut self, n: usize) -> Result<u64, String> {
        if n == 0 {
            return Ok(0);
        }
        if self.idx < n {
            //TODO handle correctly. need to fill with 0
            return Err(format!("Cant read n: {} bits. Bits left: {}", n, self.idx));
        }

        let mut value: u64;
        let start_idx = self.idx;

        let bits_left_in_current_byte = if self.idx % 8 == 0 { 8 } else { self.idx % 8 };

        if bits_left_in_current_byte >= n {
            //no need for fancy stuff
            let bits_to_keep = bits_left_in_current_byte - n;
            assert!(
                bits_to_keep < 8,
                format!("bits left in byte: {}, n: {}", bits_left_in_current_byte, n)
            );
            value = (self.source[self.byte_idx()] >> bits_to_keep) as u64;
            //mask all but the needed n bit
            value &= (1 << n) - 1;
            self.idx -= n;
        } else {
            let first_byte_mask = if bits_left_in_current_byte < 8 {
                //mask the upper bits out
                (1 << bits_left_in_current_byte) - 1
            } else {
                0xff //keep all
            };

            //n spans over multiple bytes
            let full_bytes_needed = (n - bits_left_in_current_byte) / 8;
            let bits_in_last_byte_needed = n - bits_left_in_current_byte - full_bytes_needed * 8;

            assert!(
                bits_left_in_current_byte + full_bytes_needed * 8 + bits_in_last_byte_needed == n
            );

            let mut bit_shift = full_bytes_needed * 8 + bits_in_last_byte_needed;

            //collect bits from the currently pointed to byte, excluding the ones that were already read
            value = (self.source[self.byte_idx()] & first_byte_mask) as u64;
            self.idx -= bits_left_in_current_byte;
            value = value << bit_shift;

            assert!(
                value < (1 << n),
                "itermittent value: {} bigger than should be possible reading n: {} bits, maximum: {}",
                value,
                n,
                1 << n
            );

            assert!(self.idx % 8 == 0);

            //collect full bytes
            for _ in 0..full_bytes_needed {
                //make space in shift for 8 more bits
                bit_shift -= 8;

                //add byte to decoded value
                value |= (self.source[self.byte_idx()] as u64) << bit_shift;

                //update index
                self.idx -= 8;
            }

            assert!(bit_shift == bits_in_last_byte_needed);

            if bits_in_last_byte_needed > 0 {
                let last_byte_shift = 8 - bits_in_last_byte_needed; //need to shift out lower part of the last byte
                let val_last_byte = (self.source[self.byte_idx()] >> last_byte_shift) as u64;
                value |= val_last_byte;
                self.idx -= bits_in_last_byte_needed;
            }
        }

        assert!(self.idx == start_idx - n);
        assert!(
            value < (1 << n),
            "value: {} bigger than should be possible reading n: {} bits, maximum: {}",
            value,
            n,
            1 << n
        );
        Ok(value)
    }

    pub fn reset(&mut self, new_source: &'s [u8]) {
        self.idx = new_source.len() * 8;
        self.source = new_source;
    }
}
