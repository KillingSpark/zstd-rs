pub struct BitReader {
    idx: usize, //index counts bits already read
}

impl BitReader {
    pub fn new() -> BitReader {
        BitReader{
            idx: 0,
        }
    }

    pub fn return_bits(&mut self, n: usize) {
        if n > self.idx {
            panic!("Cant return this many bits");
        }
        self.idx -= n;
    }

    //it is the responsibility of the user to ensure that this only gets called on the same source or 'reset' is called when changing the source
    pub fn get_bits(&mut self, n: usize, source: &[u8]) -> Result<u64, String> {
        if n > 64 {
            panic!("Cant do that");
        }
        if (self.idx + n) / 8 >= source.len() {
            return Err(format!("Cant read n: {} bits. Bits left: {}", n, source.len()*8 - self.idx));
        }

        let mut value: u64;

        let bits_left_in_current_byte = 8 - (self.idx % 8);
        let bits_not_needed_in_current_byte = 8 - bits_left_in_current_byte;

        if bits_left_in_current_byte >= n {
            //no need for fancy stuff
            value = (source[self.idx / 8] >> bits_not_needed_in_current_byte) as u64;
            //mask the all but the needed n bit
            value &= (1 << n) - 1;
            self.idx += n;
        } else {
            //n spans over multiple bytes
            let full_bytes_needed = (n - bits_left_in_current_byte) / 8;
            let bits_in_last_byte_needed = n - bits_left_in_current_byte - full_bytes_needed * 8;

            assert!(
                bits_left_in_current_byte + full_bytes_needed * 8 + bits_in_last_byte_needed == n
            );

            //collect bits from the currently pointed to byte
            value = (source[self.idx / 8] >> bits_not_needed_in_current_byte) as u64;

            self.idx += bits_left_in_current_byte;
            let mut bit_shift = bits_left_in_current_byte; //this many bits are already set in value

            assert!(self.idx % 8 == 0);

            //collect full bytes
            for _ in 0..full_bytes_needed {
                value |= (source[self.idx / 8] << bit_shift) as u64;
                self.idx += 8;
                bit_shift += 8;
            }

            let val_las_byte = (source[self.idx / 8] as u64) & (1 << bits_in_last_byte_needed) - 1;
            value |= val_las_byte << bit_shift;
            self.idx += bits_in_last_byte_needed;
        }

        Ok(value)
    }

    pub fn reset(&mut self) {
        self.idx = 0;
    }
}
