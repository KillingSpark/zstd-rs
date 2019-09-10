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
            idx: source.len() * 8 - 1,
            source: source,
        }
    }

    pub fn return_bits(&mut self, n: usize) {
        if n > self.idx {
            panic!("Cant return this many bits");
        }
        self.idx -= n;
    }

    pub fn get_bits(&mut self, n: usize) -> Result<u64, String> {
        if self.idx < n {
            //TODO handle correctly. need to fill with 0
            return Err(format!("Cant read n: {} bits. Bits left: {}", n, self.idx));
        }

        let mut value: u64;

        let bits_left_in_current_byte = if self.idx % 8 == 0 { 8 } else { self.idx % 8 };

        if bits_left_in_current_byte >= n {
            //no need for fancy stuff
            let bits_to_keep = bits_left_in_current_byte - n;
            value = (self.source[self.idx / 8] >> bits_to_keep) as u64;
            //mask all but the needed n bit
            value &= (1 << n) - 1;
            self.idx -= n;
        } else {
            let first_byte_mask = (1 << bits_left_in_current_byte) - 1;
            //collect bits from the currently pointed to byte, excluding the ones that were already read
            value = (self.source[self.idx / 8] & first_byte_mask) as u64;

            //n spans over multiple bytes
            let full_bytes_needed = (n - bits_left_in_current_byte) / 8;
            let bits_in_last_byte_needed = n - bits_left_in_current_byte - full_bytes_needed * 8;

            assert!(
                bits_left_in_current_byte + full_bytes_needed * 8 + bits_in_last_byte_needed == n
            );

            self.idx -= bits_left_in_current_byte;
            let mut bit_shift = full_bytes_needed * 8 + bits_in_last_byte_needed;
            value <<= bit_shift;

            assert!(self.idx % 8 == 0);

            //collect full bytes
            for _ in 0..full_bytes_needed {
                value |= ((self.source[self.idx / 8] as u64) << bit_shift);
                self.idx -= 8;
                bit_shift -= 8;
            }

            assert!(bit_shift == bits_in_last_byte_needed);

            let last_byte_shift = 8 - bits_in_last_byte_needed; //need to shift out lower part of the last byte
            let val_last_byte = (self.source[self.idx / 8] >> last_byte_shift) as u64;
            value |= val_last_byte;
            self.idx -= bits_in_last_byte_needed;
        }

        Ok(value)
    }

    pub fn reset(&mut self, new_source: &'s [u8]) {
        self.idx = new_source.len() * 8-1;
        self.source = new_source;
    }
}
