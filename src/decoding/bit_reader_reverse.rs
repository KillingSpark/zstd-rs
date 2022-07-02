pub use super::bit_reader::GetBitsError;
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

    /// We refill the container in full bytes, shifting the still unread portion to the left, and filling the lower bits with new data
    #[inline(always)]
    fn refill_container(&mut self) {
        let byte_idx = self.byte_idx() as usize;

        let retain_bytes = (self.bits_in_container + 7) / 8;
        let want_to_read_bits = 64 - (retain_bytes * 8);

        // if there are >= 8 byte left to read we go a fast path:
        // The slice is looking something like this |U..UCCCCCCCCR..R| Where U are some unread bytes, C are the bytes in the container, and R are already read bytes
        // What we do is, we shift the container by a few bytes to the left by just reading a u64 from the correct position, rereading the portion we did not yet return from the conainer.
        // Technically this would still work for positions lower than 8 but this guarantees that enough bytes are in the source and generally makes for less edge cases
        if byte_idx >= 8 {
            self.refill_fast(byte_idx, retain_bytes, want_to_read_bits)
        } else {
            // In the slow path we just read however many bytes we can
            self.refill_slow(byte_idx, want_to_read_bits)
        }
    }

    #[inline(always)]
    fn refill_fast(&mut self, byte_idx: usize, retain_bytes: u8, want_to_read_bits: u8) {
        let load_from_byte_idx = byte_idx - 7 + retain_bytes as usize;
        let refill = LittleEndian::read_u64(&self.source[load_from_byte_idx..]);
        self.bit_container = refill;
        self.bits_in_container += want_to_read_bits;
        self.idx -= want_to_read_bits as isize;
    }

    #[cold]
    fn refill_slow(&mut self, byte_idx: usize, want_to_read_bits: u8) {
        let can_read_bits = isize::min(want_to_read_bits as isize, self.idx);
        let can_read_bytes = can_read_bits / 8;

        match can_read_bytes {
            8 => {
                self.bit_container = LittleEndian::read_u64(&self.source[byte_idx - 7..]);
                self.bits_in_container += 64;
                self.idx -= 64;
            }
            6..=7 => {
                self.bit_container <<= 48;
                self.bits_in_container += 48;
                self.bit_container |= LittleEndian::read_u48(&self.source[byte_idx - 5..]);
                self.idx -= 48;
            }
            4..=5 => {
                self.bit_container <<= 32;
                self.bits_in_container += 32;
                self.bit_container |=
                    u64::from(LittleEndian::read_u32(&self.source[byte_idx - 3..]));
                self.idx -= 32;
            }
            2..=3 => {
                self.bit_container <<= 16;
                self.bits_in_container += 16;
                self.bit_container |=
                    u64::from(LittleEndian::read_u16(&self.source[byte_idx - 1..]));
                self.idx -= 16;
            }
            1 => {
                self.bit_container <<= 8;
                self.bits_in_container += 8;
                self.bit_container |= u64::from(self.source[byte_idx]);
                self.idx -= 8;
            }
            _ => {
                panic!("This cannot be reached");
            }
        }
    }

    /// Next byte that should be read into the container
    /// Negative values mean that the source buffer as been read into the container completetly.
    fn byte_idx(&self) -> isize {
        (self.idx - 1) / 8
    }

    #[inline(always)]
    pub fn get_bits(&mut self, n: u8) -> Result<u64, GetBitsError> {
        if n == 0 {
            return Ok(0);
        }
        if self.bits_in_container >= n {
            return Ok(self.get_bits_unchecked(n));
        }

        self.get_bits_cold(n)
    }

    #[cold]
    fn get_bits_cold(&mut self, n: u8) -> Result<u64, GetBitsError> {
        if n > 56 {
            return Err(GetBitsError::TooManyBits {
                num_requested_bits: usize::from(n),
                limit: 56,
            });
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

    #[inline(always)]
    pub fn get_bits_triple(
        &mut self,
        n1: u8,
        n2: u8,
        n3: u8,
    ) -> Result<(u64, u64, u64), GetBitsError> {
        let sum = n1 as usize + n2 as usize + n3 as usize;
        if sum == 0 {
            return Ok((0, 0, 0));
        }
        if sum > 56 {
            // try and get the values separatly
            return Ok((self.get_bits(n1)?, self.get_bits(n2)?, self.get_bits(n3)?));
        }
        let sum = sum as u8;

        if self.bits_in_container >= sum {
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

            return Ok((v1, v2, v3));
        }

        self.get_bits_triple_cold(n1, n2, n3, sum)
    }

    #[cold]
    fn get_bits_triple_cold(
        &mut self,
        n1: u8,
        n2: u8,
        n3: u8,
        sum: u8,
    ) -> Result<(u64, u64, u64), GetBitsError> {
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
