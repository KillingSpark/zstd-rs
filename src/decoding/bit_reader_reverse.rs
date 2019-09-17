extern crate byteorder;
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

    pub fn new(source: &'s [u8]) -> BitReaderReversed {
        BitReaderReversed {
            idx: source.len() as isize * 8,
            source: source,
            bit_container: 0,
            bits_in_container: 0,
        }
    }

    fn refill_container(&mut self) {
        let want_to_read = 64 - self.bits_in_container;
        let can_read = if want_to_read as isize > self.idx {
            self.idx
        } else {
            want_to_read as isize
        };

        match can_read {
            64 => {
                self.bit_container = LittleEndian::read_u64(&self.source[self.byte_idx() - 7..]);
                self.bits_in_container += 64;
                self.idx -= 64;
            }
            48...63 => {
                self.bit_container = self.bit_container << 48;
                self.bits_in_container += 48;
                self.bit_container |= LittleEndian::read_u48(&self.source[self.byte_idx() - 5..]);
                self.idx -= 48;
            }
            32...47 => {
                self.bit_container = self.bit_container << 32;
                self.bits_in_container += 32;
                self.bit_container |= LittleEndian::read_u32(&self.source[self.byte_idx() - 3..]) as u64;
                self.idx -= 32;
            }
            16...31 => {
                self.bit_container = self.bit_container << 16;
                self.bits_in_container += 16;
                self.bit_container |= LittleEndian::read_u16(&self.source[self.byte_idx() - 1..]) as u64;
                self.idx -= 16;
            }
            8...15 => {
                self.bit_container = self.bit_container << 8;
                self.bits_in_container += 8;
                self.bit_container |= self.source[self.byte_idx()] as u64;
                self.idx -= 8;
            }
            _ => panic!("For now panic"),
        }
    }

    fn byte_idx(&self) -> usize {
        ((self.idx as usize - 1) / 8)
    }

    pub fn get_bits(&mut self, n: usize) -> Result<u64, String> {
        if n == 0 {
            return Ok(0);
        }
        if n > 64 {
            return Err("Cant serve this request. The reader is limited to 64bit".to_owned());
        }

        let n = n as isize;

        if self.bits_remaining() <= 0 {
            self.idx -= n;
            return Ok(0);
        }

        if self.bits_remaining() < n {
            //TODO handle correctly. need to fill with 0
            let emulated_read_shift = n - self.bits_remaining();
            let v = self.get_bits(self.bits_remaining() as usize)?;
            assert!(self.idx == 0);
            let value = (v as u64) << emulated_read_shift;
            self.idx -= emulated_read_shift;
            return Ok(value);
        }

        if (self.bits_in_container as isize) < n {
            while (self.bits_in_container <= 56) && (self.bits_in_container as isize) < n {
                self.refill_container();
            }
            if (self.bits_in_container as isize) < n {
                return Err(format!("Cant fullfill read of {} bytes on reversed bitreader even after refill. Would need a bigger container", n));
            }
        }

        //if we reach this point there are enough bits in the container
        let value = self.bit_container >> (self.bits_in_container as isize - n);
        self.bits_in_container -= n as u8;
        let value_masked = value & ((1 << n) - 1);

        //println!("N {}", n);
        //println!("Bits_Container {}", self.bits_in_container);

        assert!(value_masked < (1<<n));

        Ok(value_masked)
    }

    pub fn reset(&mut self, new_source: &'s [u8]) {
        self.idx = new_source.len() as isize * 8;
        self.source = new_source;
        self.bit_container = 0;
        self.bits_in_container = 0;
    }
}
