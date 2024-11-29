use alloc::collections::VecDeque;
use core::cmp;

pub struct RingBuffer {
    buf: VecDeque<u8>,
}

impl RingBuffer {
    pub fn new() -> Self {
        RingBuffer {
            buf: VecDeque::new(),
        }
    }

    /// Return the number of bytes in the buffer.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Return the total capacity in the buffer
    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Return the amount of available space (in bytes) of the buffer.
    #[cfg(test)]
    pub fn free(&self) -> usize {
        let len = self.buf.len();
        let capacity = self.buf.capacity();

        capacity - len
    }

    /// Empty the buffer and reset the head and tail.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Whether the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Ensure that there's space for `amount` elements in the buffer.
    pub fn reserve(&mut self, additional: usize) {
        self.buf.reserve(additional);
    }

    #[allow(dead_code)]
    pub fn push_back(&mut self, byte: u8) {
        self.buf.push_back(byte);
    }

    /// Fetch the byte stored at the selected index from the buffer, returning it, or
    /// `None` if the index is out of bounds.
    #[allow(dead_code)]
    pub fn get(&self, idx: usize) -> Option<u8> {
        self.buf.get(idx).copied()
    }

    /// Append the provided data to the end of `self`.
    pub fn extend(&mut self, data: &[u8]) {
        self.buf.extend(data);
    }

    /// Advance head past `amount` elements, effectively removing
    /// them from the buffer.
    pub fn drop_first_n(&mut self, amount: usize) {
        debug_assert!(amount <= self.len());
        self.buf.drain(..amount);
    }

    /// Return references to each part of the ring buffer.
    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        self.buf.as_slices()
    }

    /// Copies elements from the provided range to the end of the buffer.
    #[allow(dead_code)]
    pub fn extend_from_within(&mut self, mut start: usize, len: usize) {
        if start + len > self.len() {
            panic!(
                "Calls to this functions must respect start ({}) + len ({}) <= self.len() ({})!",
                start,
                len,
                self.len()
            );
        }

        // Naive and cheaper implementation (for small lengths)
        if len <= 12 {
            self.reserve(len);
            for i in 0..len {
                let byte = self.get(start + i).unwrap();
                self.push_back(byte);
            }

            return;
        }

        let original_len = self.len();
        let mut intermediate = {
            IntermediateRingBuffer {
                this: self,
                original_len,
            }
        };

        intermediate.this.buf.extend((0..len).map(|_| 0));
        debug_assert_eq!(intermediate.this.buf.len(), original_len + len);

        let (a, b, a_spare, b_spare) = intermediate.as_slices_spare_mut();
        debug_assert_eq!(a_spare.len() + b_spare.len(), len);

        let skip = cmp::min(a.len(), start);
        start -= skip;
        let a = &a[skip..];
        let b = &b[start..];

        let mut remaining_copy_len = len;

        // A -> A Spare
        let copy_at_least = cmp::min(cmp::min(a.len(), a_spare.len()), remaining_copy_len);
        copy_bytes_overshooting(a, a_spare, copy_at_least);
        remaining_copy_len -= copy_at_least;

        if remaining_copy_len == 0 {
            return;
        }

        let a = &a[copy_at_least..];
        let a_spare = &mut a_spare[copy_at_least..];

        // A -> B Spare
        let copy_at_least = cmp::min(cmp::min(a.len(), b_spare.len()), remaining_copy_len);
        copy_bytes_overshooting(a, b_spare, copy_at_least);
        remaining_copy_len -= copy_at_least;

        if remaining_copy_len == 0 {
            return;
        }

        let b_spare = &mut b_spare[copy_at_least..];

        // B -> A Spare
        let copy_at_least = cmp::min(cmp::min(b.len(), a_spare.len()), remaining_copy_len);
        copy_bytes_overshooting(b, a_spare, copy_at_least);
        remaining_copy_len -= copy_at_least;

        if remaining_copy_len == 0 {
            return;
        }

        let b = &b[copy_at_least..];

        // B -> B Spare
        let copy_at_least = cmp::min(cmp::min(b.len(), b_spare.len()), remaining_copy_len);
        copy_bytes_overshooting(b, b_spare, copy_at_least);
        remaining_copy_len -= copy_at_least;

        debug_assert_eq!(remaining_copy_len, 0);
    }
}

struct IntermediateRingBuffer<'a> {
    this: &'a mut RingBuffer,
    original_len: usize,
}

impl<'a> IntermediateRingBuffer<'a> {
    // inspired by `Vec::split_at_spare_mut`
    fn as_slices_spare_mut(&mut self) -> (&[u8], &[u8], &mut [u8], &mut [u8]) {
        let (a, b) = self.this.buf.as_mut_slices();
        debug_assert!(a.len() + b.len() >= self.original_len);

        let mut remaining_init_len = self.original_len;
        let a_mid = cmp::min(a.len(), remaining_init_len);
        remaining_init_len -= a_mid;
        let b_mid = remaining_init_len;
        debug_assert!(b.len() >= b_mid);

        let (a, a_spare) = a.split_at_mut(a_mid);
        let (b, b_spare) = b.split_at_mut(b_mid);
        debug_assert!(a_spare.is_empty() || b.is_empty());

        (a, b, a_spare, b_spare)
    }
}

/// Similar to ptr::copy_nonoverlapping
///
/// But it might overshoot the desired copy length if deemed useful
///
/// src and dst specify the entire length they are eligible for reading/writing respectively
/// in addition to the desired copy length.
///
/// This function will then copy in chunks and might copy up to chunk size - 1 more bytes from src to dst
/// if that operation does not read/write memory that does not belong to src/dst.
///
/// The chunk size is not part of the contract and may change depending on the target platform.
///
/// If that isn't possible we just fall back to ptr::copy_nonoverlapping
fn copy_bytes_overshooting(src: &[u8], dst: &mut [u8], copy_at_least: usize) {
    let src = &src[..copy_at_least];
    let dst = &mut dst[..copy_at_least];

    dst.copy_from_slice(src);
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::RingBuffer;

    #[test]
    fn smoke() {
        let mut rb = RingBuffer::new();

        rb.reserve(15);
        assert!(rb.capacity() >= 15);

        rb.extend(b"0123456789");
        assert_eq!(rb.len(), 10);
        assert_eq!(contents(&rb), b"0123456789");

        rb.drop_first_n(5);
        assert_eq!(rb.len(), 5);
        assert_eq!(contents(&rb), b"56789");

        rb.extend_from_within(2, 3);
        assert_eq!(rb.len(), 8);
        assert_eq!(contents(&rb), b"56789789");

        rb.extend_from_within(0, 3);
        assert_eq!(rb.len(), 11);
        assert_eq!(contents(&rb), b"56789789567");

        rb.extend_from_within(0, 2);
        assert_eq!(rb.len(), 13);
        assert_eq!(contents(&rb), b"5678978956756");

        rb.drop_first_n(11);
        assert_eq!(rb.len(), 2);
        assert_eq!(contents(&rb), b"56");

        rb.extend(b"0123456789");
        assert_eq!(rb.len(), 12);
        assert_eq!(contents(&rb), b"560123456789");

        rb.drop_first_n(11);
        assert_eq!(rb.len(), 1);
        assert_eq!(contents(&rb), b"9");

        rb.extend(b"0123456789");
        assert_eq!(rb.len(), 11);
        assert_eq!(contents(&rb), b"90123456789");
    }

    #[test]
    fn edge_cases() {
        // Fill exactly, then empty then fill again
        let mut rb = RingBuffer::new();
        rb.reserve(16);
        let prev_capacity = rb.capacity();
        assert!(prev_capacity >= 16);
        rb.extend(b"0123456789012345");
        assert_eq!(prev_capacity, rb.capacity());
        assert_eq!(16, rb.len());
        assert_eq!(0, rb.free());
        rb.drop_first_n(16);
        assert_eq!(0, rb.len());
        assert_eq!(16, rb.free());
        rb.extend(b"0123456789012345");
        assert_eq!(16, rb.len());
        assert_eq!(0, rb.free());
        assert_eq!(prev_capacity, rb.capacity());
        assert_eq!(16, rb.as_slices().0.len() + rb.as_slices().1.len());

        rb.clear();

        // data in both slices and then reserve
        rb.extend(b"0123456789012345");
        rb.drop_first_n(8);
        rb.extend(b"67890123");
        assert_eq!(16, rb.len());
        assert_eq!(0, rb.free());
        assert_eq!(prev_capacity, rb.capacity());
        assert_eq!(16, rb.as_slices().0.len() + rb.as_slices().1.len());
        rb.reserve(1);
        assert_eq!(16, rb.len());
        assert_eq!(16, rb.free());
        assert!(rb.capacity() >= 17);
        assert_eq!(16, rb.as_slices().0.len() + rb.as_slices().1.len());

        rb.clear();

        // fill exactly, then extend from within
        rb.extend(b"0123456789012345");
        rb.extend_from_within(0, 16);
        assert_eq!(32, rb.len());
        assert_eq!(0, rb.free());
        assert!(rb.capacity() >= 32);
        assert_eq!(32, rb.as_slices().0.len());
        assert_eq!(0, rb.as_slices().1.len());

        // extend from within cases
        let mut rb = RingBuffer::new();
        rb.reserve(8);
        rb.extend(b"01234567");
        rb.drop_first_n(5);
        rb.extend_from_within(0, 3);
        assert_eq!(6, rb.as_slices().0.len() + rb.as_slices().1.len());

        rb.drop_first_n(2);
        assert_eq!(4, rb.as_slices().0.len() + rb.as_slices().1.len());
        rb.extend_from_within(0, 4);
        assert_eq!(8, rb.as_slices().0.len() + rb.as_slices().1.len());

        rb.drop_first_n(2);
        assert_eq!(6, rb.as_slices().0.len() + rb.as_slices().1.len());
        rb.drop_first_n(2);
        assert_eq!(4, rb.as_slices().0.len());
        assert_eq!(0, rb.as_slices().1.len());
        rb.extend_from_within(0, 4);
        assert_eq!(8, rb.as_slices().0.len() + rb.as_slices().1.len());

        let mut rb = RingBuffer::new();
        rb.reserve(8);
        rb.extend(b"11111111");
        rb.drop_first_n(7);
        rb.extend(b"111");
        assert_eq!(4, rb.as_slices().0.len() + rb.as_slices().1.len());
        rb.extend_from_within(0, 4);
        assert_eq!(contents(&rb), b"11111111");
    }

    fn contents(rg: &RingBuffer) -> Vec<u8> {
        let (a, b) = rg.as_slices();
        a.iter().chain(b.iter()).copied().collect()
    }
}
