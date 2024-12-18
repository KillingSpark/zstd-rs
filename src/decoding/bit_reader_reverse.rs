pub use super::bit_reader::GetBitsError;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

// The `cpufeatures` crate should be used
// as a stopgap until https://github.com/rust-lang/rfcs/pull/2725
// is stabilized.
// Here we check for avx512vl because it's apparently
// always going to be paired with GFNI support
cpufeatures::new!(x86_64_intrinsic_support, "avx512vl", "sse4.1");

/// The size of the chunk, in bytes (128 bits)
const CHUNK_SIZE: usize = 16;


/// Zstandard encodes some types of data in a way that the data must be read
/// back to front to decode it properly. `BitReaderReversed` provides a
/// convenient interface to do that.
///
/// Internally, this implementation makes use of
/// SIMD bit reversal as defined in
/// <https://wunkolo.github.io/post/2020/11/gf2p8affineqb-bit-reversal/>
pub struct BitReaderReversed<'s> {
    /// In *bytes* from the beginning of `source`, keeps
    /// track of what data has been read into `chunk`, and what hasn't.
    source_index: usize,
    /// What the bit reader reads backwards from.
    /// 
    /// 128 bits are read from `source` at a time, flipped, then
    /// stored into `chunk`. When `get_bits` is called, data is read from
    /// `chunk`, with chunk being transparently refilled as needed.
    source: &'s [u8],
    /// 128 bits are read from `source`, flipped, and stored here.
    /// 
    /// `get_bits` then reads forwards from the start of `chunk`, refilling as needed.
    chunk: u128,
    /// In *bits* from the beginning of the chunk, points to the next bit
    /// to be read from the chunk.
    chunk_index: usize,
}

impl<'s> BitReaderReversed<'s> {
    // /// How many bits are left to read by the reader.
    // pub fn bits_remaining(&self) -> isize {
    //     self.idx + self.bits_in_container as isize
    // }

    // pub fn new(source: &'s [u8]) -> BitReaderReversed<'s> {
    //     BitReaderReversed {
    //         idx: source.len() as isize * 8,
    //         source,
    //         bit_container: 0,
    //         bits_in_container: 0,
    //     }
    // }
    pub fn new(source: &'s [u8]) -> BitReaderReversed<'s> {
        return BitReaderReversed {
            source_index: source.len() - 1,
            source,
            chunk: 0,
            chunk_index: 127,
        }
    }

    /// Reads the next 128 bits from `source`, flipping it
    /// and storing it in `chunk`.
    /// 
    /// This function does not update update the chunk index or the source index, that's
    /// left to the caller.
    #[inline(always)]
    fn fill_chunk(&mut self) {
        let (_, x86_64_intrinsics_supported) = x86_64_intrinsic_support::init_get();
        if x86_64_intrinsics_supported {
            // SAFETY: These intrinsics are only unsafe because trying to use them when
            // a CPU doesn't support it results in UB. The above check ensures that GFNI is supported
            unsafe {
                let subslice = self.source[self.source_index..self.source_index + 16].as_ptr();
                // Load a 128 bit vector
                let input = _mm_loadu_si128(subslice as *const __m128i);
                // Reverse the order of the bytes within the register
                _mm_shuffle_epi8(
                // Reverse each of the bits within the byte
                _mm_gf2p8affine_epi64_epi8::<0>(
                    input,
                    _mm_set1_epi64x(
                        // An 8x8 binary matrix with 1s diagonally from top left to bottom right
                        0b10000000_01000000_00100000_00010000_00001000_00000100_00000010_00000001,
                    ),
                ),
                _mm_set_epi8(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15)
            );
                self.chunk = 0;
                // Read the first chunk out of the register
                self.chunk |= _mm_cvtsi128_si64(input) as u128;
                // Read the second chunk out of the register
                self.chunk |= (_mm_extract_epi64(input, 1) as u128) << 8;
            }
        } else {
            unimplemented!("No implementation has been made for your architecture, and no fallback is available.");
        }
    }

    /// Read `n` bits from the source, backwards. 
    pub fn get_bits(&mut self, n: u8) -> u64 {
        // If we can't read enough bits from the chunk to fufill the request,
        // refill the chunk
        if self.chunk_index < n.into() {
            // Determine how many bytes we've fully read, and move the source index back
            let num_to_dispose: usize = (self.chunk_index as f32 / 8.0f32).floor() as usize;
            // Special handling is needed once the end of the source is reached
            if self.source_index <= num_to_dispose {
                let num_bytes_left = self.source_index;
                self.source_index = 0;
                self.fill_chunk();
                self.chunk_index = 127 - (self.chunk_index % 8) - num_bytes_left;
            } else {
                self.source_index -= num_to_dispose;
                self.fill_chunk();
                let num_bits_left = self.chunk_index % 8;
                self.source_index -= CHUNK_SIZE;
                self.chunk_index = 127 - num_bits_left;
            }
        }

        // Shift left to zero out any data already read
        let left_shifted_chunk = self.chunk << self.chunk_index;
        // Move it back so it's up against the right side of the chunk
        let right_shifted_chunk = left_shifted_chunk >> CHUNK_SIZE * 8 - ((n as usize));
        right_shifted_chunk as u64
    }


    // /// We refill the container in full bytes, shifting the still unread portion to the left, and filling the lower bits with new data
    // #[inline(always)]
    // fn refill_container(&mut self) {
    //     let byte_idx = self.byte_idx() as usize;

    //     let retain_bytes = (self.bits_in_container + 7) / 8;
    //     let want_to_read_bits = 64 - (retain_bytes * 8);

    //     // if there are >= 8 byte left to read we go a fast path:
    //     // The slice is looking something like this |U..UCCCCCCCCR..R| Where U are some unread bytes, C are the bytes in the container, and R are already read bytes
    //     // What we do is, we shift the container by a few bytes to the left by just reading a u64 from the correct position, rereading the portion we did not yet return from the conainer.
    //     // Technically this would still work for positions lower than 8 but this guarantees that enough bytes are in the source and generally makes for less edge cases
    //     if byte_idx >= 8 {
    //         self.refill_fast(byte_idx, retain_bytes, want_to_read_bits)
    //     } else {
    //         // In the slow path we just read however many bytes we can
    //         self.refill_slow(byte_idx, want_to_read_bits)
    //     }
    // }

    // ///
    // #[inline(always)]
    // fn refill_fast(&mut self, byte_idx: usize, retain_bytes: u8, want_to_read_bits: u8) {
    //     let load_from_byte_idx = byte_idx - 7 + retain_bytes as usize;
    //     let tmp_bytes: [u8; 8] = (&self.source[load_from_byte_idx..][..8])
    //         .try_into()
    //         .unwrap();
    //     let refill = u64::from_le_bytes(tmp_bytes);
    //     self.bit_container = refill;
    //     self.bits_in_container += want_to_read_bits;
    //     self.idx -= want_to_read_bits as isize;
    // }

    // #[cold]
    // fn refill_slow(&mut self, byte_idx: usize, want_to_read_bits: u8) {
    //     let can_read_bits = isize::min(want_to_read_bits as isize, self.idx);
    //     let can_read_bytes = can_read_bits / 8;
    //     let mut tmp_bytes = [0u8; 8];
    //     let offset @ 1..=8 = can_read_bytes as usize else {
    //         unreachable!()
    //     };
    //     let bits_read = offset * 8;

    //     let _ = (&self.source[byte_idx - (offset - 1)..]).read_exact(&mut tmp_bytes[0..offset]);
    //     self.bits_in_container += bits_read as u8;
    //     self.idx -= bits_read as isize;
    //     if offset < 8 {
    //         self.bit_container <<= bits_read;
    //         self.bit_container |= u64::from_le_bytes(tmp_bytes);
    //     } else {
    //         self.bit_container = u64::from_le_bytes(tmp_bytes);
    //     }
    // }

    // /// Next byte that should be read into the container
    // /// Negative values mean that the source buffer as been read into the container completetly.
    // fn byte_idx(&self) -> isize {
    //     (self.idx - 1) / 8
    // }

    // /// Read `n` number of bits from the source. Will read at most 56 bits.
    // /// If there are no more bits to be read from the source zero bits will be returned instead.
    // #[inline(always)]
    // pub fn get_bits(&mut self, n: u8) -> u64 {
    //     if n == 0 {
    //         return 0;
    //     }
    //     if self.bits_in_container >= n {
    //         return self.get_bits_unchecked(n);
    //     }

    //     self.get_bits_cold(n)
    // }

    // #[cold]
    // fn get_bits_cold(&mut self, n: u8) -> u64 {
    //     let n = u8::min(n, 56);
    //     let signed_n = n as isize;

    //     if self.bits_remaining() <= 0 {
    //         self.idx -= signed_n;
    //         return 0;
    //     }

    //     if self.bits_remaining() < signed_n {
    //         let emulated_read_shift = signed_n - self.bits_remaining();
    //         let v = self.get_bits(self.bits_remaining() as u8);
    //         debug_assert!(self.idx == 0);
    //         let value = v.wrapping_shl(emulated_read_shift as u32);
    //         self.idx -= emulated_read_shift;
    //         return value;
    //     }

    //     while (self.bits_in_container < n) && self.idx > 0 {
    //         self.refill_container();
    //     }

    //     debug_assert!(self.bits_in_container >= n);

    //     //if we reach this point there are enough bits in the container

    //     self.get_bits_unchecked(n)
    // }

    // /// Same as calling get_bits three times but slightly more performant
    // #[inline(always)]
    // pub fn get_bits_triple(&mut self, n1: u8, n2: u8, n3: u8) -> (u64, u64, u64) {
    //     let sum = n1 as usize + n2 as usize + n3 as usize;
    //     if sum == 0 {
    //         return (0, 0, 0);
    //     }
    //     if sum > 56 {
    //         // try and get the values separately
    //         return (self.get_bits(n1), self.get_bits(n2), self.get_bits(n3));
    //     }
    //     let sum = sum as u8;

    //     if self.bits_in_container >= sum {
    //         let v1 = if n1 == 0 {
    //             0
    //         } else {
    //             self.get_bits_unchecked(n1)
    //         };
    //         let v2 = if n2 == 0 {
    //             0
    //         } else {
    //             self.get_bits_unchecked(n2)
    //         };
    //         let v3 = if n3 == 0 {
    //             0
    //         } else {
    //             self.get_bits_unchecked(n3)
    //         };

    //         return (v1, v2, v3);
    //     }

    //     self.get_bits_triple_cold(n1, n2, n3, sum)
    // }

    // #[cold]
    // fn get_bits_triple_cold(&mut self, n1: u8, n2: u8, n3: u8, sum: u8) -> (u64, u64, u64) {
    //     let sum_signed = sum as isize;

    //     if self.bits_remaining() <= 0 {
    //         self.idx -= sum_signed;
    //         return (0, 0, 0);
    //     }

    //     if self.bits_remaining() < sum_signed {
    //         return (self.get_bits(n1), self.get_bits(n2), self.get_bits(n3));
    //     }

    //     while (self.bits_in_container < sum) && self.idx > 0 {
    //         self.refill_container();
    //     }

    //     debug_assert!(self.bits_in_container >= sum);

    //     //if we reach this point there are enough bits in the container

    //     let v1 = if n1 == 0 {
    //         0
    //     } else {
    //         self.get_bits_unchecked(n1)
    //     };
    //     let v2 = if n2 == 0 {
    //         0
    //     } else {
    //         self.get_bits_unchecked(n2)
    //     };
    //     let v3 = if n3 == 0 {
    //         0
    //     } else {
    //         self.get_bits_unchecked(n3)
    //     };

    //     (v1, v2, v3)
    // }

    // #[inline(always)]
    // fn get_bits_unchecked(&mut self, n: u8) -> u64 {
    //     let shift_by = self.bits_in_container - n;
    //     let mask = (1u64 << n) - 1u64;

    //     let value = self.bit_container >> shift_by;
    //     self.bits_in_container -= n;
    //     let value_masked = value & mask;
    //     debug_assert!(value_masked < (1 << n));

    //     value_masked
    // }

    // pub fn reset(&mut self, new_source: &'s [u8]) {
    //     self.idx = new_source.len() as isize * 8;
    //     self.source = new_source;
    //     self.bit_container = 0;
    //     self.bits_in_container = 0;
    // }
}
