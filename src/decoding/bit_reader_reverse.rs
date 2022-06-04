use byteorder::ByteOrder;
use byteorder::LittleEndian;

pub struct BitReaderReversed<'s> {
    idx: isize, //index counts bits already read
    source: &'s [u8],

    bit_container: u64,
    bits_in_container: u8,
}

impl<'s> BitReaderReversed<'s> {
    pub fn bits_remaining(&self) -> isize {
        self.idx + self.bits_in_container as isize
    }

    pub fn new(source: &'s [u8]) -> BitReaderReversed<'_> {
        BitReaderReversed {
            idx: source.len() as isize * 8,
            source,
            bit_container: 0,
            bits_in_container: 0,
        }
    }

    fn refill_container(&mut self) {
        let want_to_read = 64 - self.bits_in_container;
        let can_read = isize::min(want_to_read as isize, self.idx) / 8;

        match can_read {
            8 => {
                self.bit_container = LittleEndian::read_u64(&self.source[self.byte_idx() - 7..]);
                self.bits_in_container += 64;
                self.idx -= 64;
            }
            6..=7 => {
                self.bit_container <<= 48;
                self.bits_in_container += 48;
                self.bit_container |= LittleEndian::read_u48(&self.source[self.byte_idx() - 5..]);
                self.idx -= 48;
            }
            4..=5 => {
                self.bit_container <<= 32;
                self.bits_in_container += 32;
                self.bit_container |=
                    u64::from(LittleEndian::read_u32(&self.source[self.byte_idx() - 3..]));
                self.idx -= 32;
            }
            2..=3 => {
                self.bit_container <<= 16;
                self.bits_in_container += 16;
                self.bit_container |=
                    u64::from(LittleEndian::read_u16(&self.source[self.byte_idx() - 1..]));
                self.idx -= 16;
            }
            1 => {
                self.bit_container <<= 8;
                self.bits_in_container += 8;
                self.bit_container |= u64::from(self.source[self.byte_idx()]);
                self.idx -= 8;
            }
            _ => panic!("For now panic"),
        }
    }

    fn byte_idx(&self) -> usize {
        (self.idx as usize - 1) / 8
    }

    pub fn get_bits(&mut self, n: u8) -> Result<u64, String> {
        if n == 0 {
            return Ok(0);
        }
        if n > 56 {
            return Err("Cant serve this request. The reader is limited to 56bit".to_owned());
        }

        let signed_n = n as isize;

        if self.bits_remaining() <= 0 {
            self.idx -= signed_n;
            return Ok(0);
        }

        if self.bits_remaining() < signed_n {
            let emulated_read_shift = signed_n - self.bits_remaining();
            let v = self.get_bits(self.bits_remaining() as u8)?;
            debug_assert!(self.idx == 0);
            let value = v << emulated_read_shift;
            self.idx -= emulated_read_shift;
            return Ok(value);
        }

        while (self.bits_in_container < n) && self.idx > 0 {
            self.refill_container();
        }

        debug_assert!(self.bits_in_container >= n);

        //if we reach this point there are enough bits in the container

        Ok(self.get_bits_unchecked(n))
    }

    pub fn get_bits_triple(&mut self, n1: u8, n2: u8, n3: u8) -> Result<(u64, u64, u64), String> {
        let sum = n1 + n2 + n3;
        if sum == 0 {
            return Ok((0, 0, 0));
        }
        if sum > 56 {
            // try and get the values separatly
            return Ok((self.get_bits(n1)?, self.get_bits(n2)?, self.get_bits(n3)?));
        }

        let sum_signed = sum as isize;

        if self.bits_remaining() <= 0 {
            self.idx -= sum_signed;
            return Ok((0, 0, 0));
        }

        if self.bits_remaining() < sum_signed {
            return Ok((self.get_bits(n1)?, self.get_bits(n2)?, self.get_bits(n3)?));
        }

        while (self.bits_in_container < sum) && self.idx > 0 {
            self.refill_container();
        }

        debug_assert!(self.bits_in_container >= sum);

        //if we reach this point there are enough bits in the container

        let v1 = if n1 == 0 {
            0
        } else {
            self.get_bits_unchecked(n1)
        };
        let v2 = if n2 == 0 {
            0
        } else {
            self.get_bits_unchecked(n2)
        };
        let v3 = if n3 == 0 {
            0
        } else {
            self.get_bits_unchecked(n3)
        };

        Ok((v1, v2, v3))
    }

    #[inline(always)]
    fn get_bits_unchecked(&mut self, n: u8) -> u64 {
        let shift_by = self.bits_in_container - n;
        let mask = (1u64 << n) - 1u64;

        let value = self.bit_container >> shift_by;
        self.bits_in_container -= n;
        let value_masked = value & mask;
        debug_assert!(value_masked < (1 << n));

        value_masked
    }

    pub fn reset(&mut self, new_source: &'s [u8]) {
        self.idx = new_source.len() as isize * 8;
        self.source = new_source;
        self.bit_container = 0;
        self.bits_in_container = 0;
    }
}
