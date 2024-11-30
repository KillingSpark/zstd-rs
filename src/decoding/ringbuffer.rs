use alloc::collections::VecDeque;
use core::{cmp, mem::MaybeUninit};

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
    pub fn extend_from_within(&mut self, start: usize, mut len: usize) {
        if start + len > self.len() {
            panic!(
                "Calls to this functions must respect start ({}) + len ({}) <= self.len() ({})!",
                start,
                len,
                self.len()
            );
        }

        self.reserve(len);

        let mut buf = [MaybeUninit::<u8>::uninit(); 2048];
        let mut start = start;
        while len > 0 {
            let round_len = cmp::min(len, buf.len());
            let mut remaining_len = round_len;

            let (a, b) = self.buf.as_slices();
            let b = if start < a.len() {
                let a = &a[start..];
                let end = cmp::min(a.len(), remaining_len);
                unsafe {
                    buf.as_mut_ptr()
                        .cast::<u8>()
                        .copy_from_nonoverlapping(a.as_ptr(), end);
                }
                remaining_len -= end;
                b
            } else {
                unsafe { b.get_unchecked(start - a.len()..) }
            };

            if remaining_len > 0 {
                unsafe {
                    buf.as_mut_ptr()
                        .cast::<u8>()
                        .add(round_len - remaining_len)
                        .copy_from_nonoverlapping(b.as_ptr(), remaining_len);
                }
            }

            /*
            let mut i = 0;
            self.buf.iter().skip(start).take(len).for_each(|&b| unsafe {
                *buf.get_unchecked_mut(i) = MaybeUninit::new(b);
                i += 1;
            });
            */

            self.buf.extend(unsafe {
                std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), round_len)
            });
            len -= round_len;
            start += round_len;
        }
    }
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
