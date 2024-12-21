use core::convert::TryInto;

/// Zstandard encodes some types of data in a way that the data must be read
/// back to front to decode it properly. `BitReaderReversed` provides a
/// convenient interface to do that.
pub struct BitReaderReversed<'s> {
    index: usize,

    bits_consumed: u8,

    source: &'s [u8],
    /// The reader doesn't read directly from the source,
    /// it reads bits from here, and the container is
    /// "refilled" as it's emptied.
    bit_container: u64,
}

impl<'s> BitReaderReversed<'s> {
    /// How many bits are left to read by the reader.
    pub fn bits_remaining(&self) -> isize {
        self.index as isize * 8 + (64 - self.bits_consumed as isize)
    }

    pub fn new(source: &'s [u8]) -> BitReaderReversed<'s> {
        BitReaderReversed {
            index: source.len(),
            bits_consumed: 64,
            source,
            bit_container: 0,
        }
    }

    /// We refill the container in full bytes, shifting the still unread portion to the left, and filling the lower bits with new data
    #[inline(always)]
    fn refill_container(&mut self) {
        let bytes_consumed = self.bits_consumed as usize / 8;
        if bytes_consumed == 0 {
            return;
        }

        if self.index >= bytes_consumed {
            self.index -= bytes_consumed;
            self.bits_consumed &= 7;
            self.bit_container =
                u64::from_le_bytes((&self.source[self.index..][..8]).try_into().unwrap());
        } else {
            if self.source.len() >= 8 {
                self.bit_container = u64::from_le_bytes((&self.source[..8]).try_into().unwrap());
            } else {
                let mut value = [0; 8];
                value[..self.source.len()].copy_from_slice(&self.source);
                self.bit_container = u64::from_le_bytes(value);
            }

            self.bits_consumed -= 8 * self.index as u8;
            self.index = 0;
        }
    }


    /// Read `n` number of bits from the source. Will read at most 56 bits.
    /// If there are no more bits to be read from the source zero bits will be returned instead.
    #[inline(always)]
    pub fn get_bits(&mut self, n: u8) -> u64 {
        if self.bits_consumed + n > 64 {
            self.refill_container();
        }

        let value = self.peak_bits(n);
        self.consume(n);
        value
    }

    pub fn peak_bits(&mut self, n: u8) -> u64 {
        if self.bits_consumed >= 64 {
            return 0;
        }

        let mask = (1u64 << n) - 1u64;

        if self.bits_consumed + n > 64 {
            let shift_by = (self.bits_consumed + n) - 64;
            return (self.bit_container << shift_by) & mask;
        }

        let shift_by = 64 - self.bits_consumed - n;
        (self.bit_container >> shift_by) & mask
    }

    pub fn consume(&mut self, n: u8) {
        self.bits_consumed += n;
    }

    /// Same as calling get_bits three times but slightly more performant
    #[inline(always)]
    pub fn get_bits_triple(&mut self, n1: u8, n2: u8, n3: u8) -> (u64, u64, u64) {
        let sum = n1 as usize + n2 as usize + n3 as usize;
        if sum == 0 {
            return (0, 0, 0);
        }

        if sum <= 56 {
            self.refill_container();

            let v1 = self.peak_bits(n1);
            self.consume(n1);
            let v2 = self.peak_bits(n2);
            self.consume(n2);
            let v3 = self.peak_bits(n3);
            self.consume(n3);

            return (v1, v2, v3);
        }

        return (self.get_bits(n1), self.get_bits(n2), self.get_bits(n3));
    }

}


#[cfg(test)]
mod test {

    #[test]
    fn it_works() {
        let data = [0b10101010, 0b01010101];
        let mut br = super::BitReaderReversed::new(&data);
        assert_eq!(br.get_bits(1), 0);
        assert_eq!(br.get_bits(1), 1);
        assert_eq!(br.get_bits(1), 0);
        assert_eq!(br.get_bits(4), 0b1010);
        assert_eq!(br.get_bits(4), 0b1101);
    }
}