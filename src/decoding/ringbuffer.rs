use alloc::boxed::Box;
use core::{mem::MaybeUninit, slice};

pub struct RingBuffer {
    // Safety invariants:
    //
    // 1. If tail≥head
    //    a. `head..tail` must contain initialized memory.
    //    b. Else, `head..` and `..tail` must be initialized
    // 2. `head` and `tail` are in bounds (≥ 0 and < cap)
    // 3. `tail` is never `cap` except for a full buffer, and instead uses the value `0`. In other words, `tail` always points to the place
    //    where the next element would go (if there is space)
    buf: Box<[MaybeUninit<u8>]>,
    head: usize,
    tail: usize,
}

impl RingBuffer {
    pub fn new() -> Self {
        RingBuffer {
            buf: Box::new_uninit_slice(0),
            // SAFETY: Upholds invariant 1-3
            head: 0,
            tail: 0,
        }
    }

    /// Return the number of bytes in the buffer.
    pub fn len(&self) -> usize {
        let (x, y) = self.data_slice_lengths();
        x + y
    }

    /// Return the total capacity in the buffer
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Return the amount of available space (in bytes) of the buffer.
    pub fn free(&self) -> usize {
        let (x, y) = self.free_slice_lengths();
        (x + y).saturating_sub(1)
    }

    /// Empty the buffer and reset the head and tail.
    pub fn clear(&mut self) {
        // SAFETY: Upholds invariant 1, trivially
        // SAFETY: Upholds invariant 2; 0 is always valid
        self.head = 0;
        self.tail = 0;
    }

    /// Whether the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// Ensure that there's space for `amount` elements in the buffer.
    pub fn reserve(&mut self, amount: usize) {
        let free = self.free();
        if free >= amount {
            return;
        }

        self.reserve_amortized(amount - free);
    }

    #[inline(never)]
    #[cold]
    fn reserve_amortized(&mut self, amount: usize) {
        // Always have at least 1 unused element as the sentinel.
        let new_cap = usize::max(
            self.capacity().next_power_of_two(),
            (self.capacity() + amount).next_power_of_two(),
        ) + 1;

        let mut new_buf = Box::new_uninit_slice(new_cap);

        // If we had data before, copy it over to the newly alloced memory region
        if self.capacity() > 0 {
            let (a, b) = self.as_slices();

            let new_buf_ptr = new_buf.as_mut_ptr().cast::<u8>();
            unsafe {
                // SAFETY: Upholds invariant 1, we end up populating (0..(len₁ + len₂))
                new_buf_ptr.copy_from_nonoverlapping(a.as_ptr(), a.len());
                new_buf_ptr
                    .add(a.len())
                    .copy_from_nonoverlapping(b.as_ptr(), b.len());
            }

            // SAFETY: Upholds invariant 2, head is 0 and in bounds, tail is only ever `cap` if the buffer
            // is entirely full
            self.tail = a.len() + b.len();
            self.head = 0;
        }

        self.buf = new_buf;
    }

    #[allow(dead_code)]
    pub fn push_back(&mut self, byte: u8) {
        self.reserve(1);

        // SAFETY: Upholds invariant 1 by writing initialized memory
        unsafe { *self.buf.get_unchecked_mut(self.tail) = MaybeUninit::new(byte) }
        // SAFETY: Upholds invariant 2 by wrapping `tail` around
        self.tail = (self.tail + 1) % self.capacity();
    }

    /// Fetch the byte stored at the selected index from the buffer, returning it, or
    /// `None` if the index is out of bounds.
    #[allow(dead_code)]
    pub fn get(&self, idx: usize) -> Option<u8> {
        if idx < self.len() {
            // SAFETY: Establishes invariants on memory being initialized and the range being in-bounds
            // (Invariants 1 & 2)
            let idx = (self.head + idx) % self.capacity();
            Some(unsafe { self.buf.get_unchecked(idx).assume_init_read() })
        } else {
            None
        }
    }

    /// Append the provided data to the end of `self`.
    pub fn extend(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        self.reserve(data.len());

        let (a, b) = self.free_slice_parts();
        if let Some((src1, src2)) = data.split_at_checked(a.len()) {
            debug_assert!(
                src1.len() <= a.len(),
                "{} does not fit {}",
                src1.len(),
                a.len()
            );
            debug_assert!(
                src2.len() <= b.len(),
                "{} does not fit {}",
                src2.len(),
                a.len()
            );

            // SAFETY: `in_f₁ + in_f₂ = len`, so this writes `len` bytes total
            // upholding invariant 1
            unsafe {
                a.as_mut_ptr()
                    .cast::<u8>()
                    .copy_from_nonoverlapping(src1.as_ptr(), src1.len());
                b.as_mut_ptr()
                    .cast::<u8>()
                    .copy_from_nonoverlapping(src2.as_ptr(), src2.len());
            }
        } else {
            debug_assert!(
                data.len() <= a.len(),
                "{} does not fit {}",
                data.len(),
                a.len()
            );

            // SAFETY: `in_f₁ + in_f₂ = len`, so this writes `len` bytes total
            // upholding invariant 1
            unsafe {
                a.as_mut_ptr()
                    .cast::<u8>()
                    .copy_from_nonoverlapping(data.as_ptr(), data.len());
            }
        }

        // SAFETY: Upholds invariant 3 by wrapping `tail` around.
        self.tail = (self.tail + data.len()) % self.capacity();
    }

    /// Advance head past `amount` elements, effectively removing
    /// them from the buffer.
    pub fn drop_first_n(&mut self, amount: usize) {
        debug_assert!(amount <= self.len());
        let amount = usize::min(amount, self.len());
        // SAFETY: we maintain invariant 2 here since this will always lead to a smaller buffer
        // for amount≤len
        self.head = (self.head + amount) % self.capacity();
    }

    /// Return the size of the two contiguous occupied sections of memory used
    /// by the buffer.
    // SAFETY: other code relies on this pointing to initialized halves of the buffer only
    fn data_slice_lengths(&self) -> (usize, usize) {
        let len_after_head;
        let len_to_tail;

        // TODO can we do this branchless?
        if self.tail >= self.head {
            len_after_head = self.tail - self.head;
            len_to_tail = 0;
        } else {
            len_after_head = self.capacity() - self.head;
            len_to_tail = self.tail;
        }
        (len_after_head, len_to_tail)
    }

    /// Return references to each part of the ring buffer.
    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        let (len_after_head, len_to_tail) = self.data_slice_lengths();

        let buf_ptr = self.buf.as_ptr().cast::<u8>();
        (
            unsafe { slice::from_raw_parts(buf_ptr.add(self.head), len_after_head) },
            unsafe { slice::from_raw_parts(buf_ptr, len_to_tail) },
        )
    }

    // SAFETY: other code relies on this producing the lengths of free zones
    // at the beginning/end of the buffer. Everything else must be initialized
    /// Returns the size of the two unoccupied sections of memory used by the buffer.
    fn free_slice_lengths(&self) -> (usize, usize) {
        let len_to_head;
        let len_after_tail;

        // TODO can we do this branchless?
        if self.tail < self.head {
            len_after_tail = self.head - self.tail;
            len_to_head = 0;
        } else {
            len_after_tail = self.capacity() - self.tail;
            len_to_head = self.head;
        }
        (len_to_head, len_after_tail)
    }

    /// Returns mutable references to the available space and the size of that available space,
    /// for the two sections in the buffer.
    // SAFETY: Other code relies on this pointing to the free zones, data after the first and before the second must
    // be valid
    fn free_slice_parts(&mut self) -> (&mut [MaybeUninit<u8>], &mut [MaybeUninit<u8>]) {
        let (len_to_head, len_after_tail) = self.free_slice_lengths();

        let buf_ptr = self.buf.as_mut_ptr();
        (
            unsafe { slice::from_raw_parts_mut(buf_ptr.add(self.tail), len_after_tail) },
            unsafe { slice::from_raw_parts_mut(buf_ptr, len_to_head) },
        )
    }

    /// Copies elements from the provided range to the end of the buffer.
    #[allow(dead_code)]
    pub fn extend_from_within(&mut self, start: usize, len: usize) {
        if start + len > self.len() {
            panic!(
                "Calls to this functions must respect start ({}) + len ({}) <= self.len() ({})!",
                start,
                len,
                self.len()
            );
        }

        self.reserve(len);

        // SAFETY: Requirements checked:
        // 2. explicitly checked above, resulting in a panic if it does not hold
        // 3. explicitly reserved enough memory
        unsafe { self.extend_from_within_unchecked(start, len) }
    }

    /// Copies data from the provided range to the end of the buffer, without
    /// first verifying that the unoccupied capacity is available.
    ///
    /// SAFETY:
    /// For this to be safe two requirements need to hold:
    /// 2. start + len <= self.len() so we do not copy uninitialised memory
    /// 3. More then len reserved space so we do not write out-of-bounds
    #[warn(unsafe_op_in_unsafe_fn)]
    pub unsafe fn extend_from_within_unchecked(&mut self, start: usize, len: usize) {
        debug_assert!(start + len <= self.len());
        debug_assert!(self.free() >= len);

        let capacity = self.capacity();
        let buf_ptr = self.buf.as_mut_ptr().cast::<u8>();

        if self.head < self.tail {
            // Continuous source section and possibly non continuous write section:
            //
            //            H           T
            // Read:  ____XXXXSSSSXXXX________
            // Write: ________________DDDD____
            //
            // H: Head position (first readable byte)
            // T: Tail position (first writable byte)
            // X: Uninvolved bytes in the readable section
            // S: Source bytes, to be copied to D bytes
            // D: Destination bytes, going to be copied from S bytes
            // _: Uninvolved bytes in the writable section
            let after_tail = usize::min(len, capacity - self.tail);

            let src = (
                // SAFETY: `len <= isize::MAX` and fits the memory range of `buf`
                unsafe { buf_ptr.add(self.head + start) }.cast_const(),
                // Src length (see above diagram)
                self.tail - self.head - start,
            );

            let dst = (
                // SAFETY: `len <= isize::MAX` and fits the memory range of `buf`
                unsafe { buf_ptr.add(self.tail) },
                // Dst length (see above diagram)
                capacity - self.tail,
            );

            // SAFETY: `src` points at initialized data, `dst` points to writable memory
            // and does not overlap `src`.
            unsafe { copy_bytes_overshooting(src, dst, after_tail) }

            if after_tail < len {
                // The write section was not continuous:
                //
                //            H           T
                // Read:  ____XXXXSSSSXXXX__
                // Write: DD______________DD
                //
                // H: Head position (first readable byte)
                // T: Tail position (first writable byte)
                // X: Uninvolved bytes in the readable section
                // S: Source bytes, to be copied to D bytes
                // D: Destination bytes, going to be copied from S bytes
                // _: Uninvolved bytes in the writable section

                let src = (
                    // SAFETY: we are still within the memory range of `buf`
                    unsafe { src.0.add(after_tail) },
                    // Src length (see above diagram)
                    src.1 - after_tail,
                );
                let dst = (
                    buf_ptr, // Dst length overflowing (see above diagram)
                    self.head,
                );

                // SAFETY: `src` points at initialized data, `dst` points to writable memory
                // and does not overlap `src`.
                unsafe { copy_bytes_overshooting(src, dst, len - after_tail) }
            }
        } else {
            if self.head + start > capacity {
                // Continuous read section and destination section:
                //
                //                  T           H
                // Read:  XXSSSSXXXX____________XX
                // Write: __________DDDD__________
                //
                // H: Head position (first readable byte)
                // T: Tail position (first writable byte)
                // X: Uninvolved bytes in the readable section
                // S: Source bytes, to be copied to D bytes
                // D: Destination bytes, going to be copied from S bytes
                // _: Uninvolved bytes in the writable section

                let start = (self.head + start) % capacity;

                let src = (
                    // SAFETY: `len <= isize::MAX` and fits the memory range of `buf`
                    unsafe { buf_ptr.add(start) }.cast_const(),
                    // Src length (see above diagram)
                    self.tail - start,
                );

                let dst = (
                    // SAFETY: `len <= isize::MAX` and fits the memory range of `buf`
                    unsafe { buf_ptr.add(self.tail) }, // Dst length (see above diagram)
                    // Dst length (see above diagram)
                    self.head - self.tail,
                );

                // SAFETY: `src` points at initialized data, `dst` points to writable memory
                // and does not overlap `src`.
                unsafe { copy_bytes_overshooting(src, dst, len) }
            } else {
                // Possibly non continuous read section and continuous destination section:
                //
                //            T           H
                // Read:  XXXX____________XXSSSSXX
                // Write: ____DDDD________________
                //
                // H: Head position (first readable byte)
                // T: Tail position (first writable byte)
                // X: Uninvolved bytes in the readable section
                // S: Source bytes, to be copied to D bytes
                // D: Destination bytes, going to be copied from S bytes
                // _: Uninvolved bytes in the writable section

                let after_start = usize::min(len, capacity - self.head - start);

                let src = (
                    // SAFETY: `len <= isize::MAX` and fits the memory range of `buf`
                    unsafe { buf_ptr.add(self.head + start) }.cast_const(),
                    // Src length - chunk 1 (see above diagram on the right)
                    capacity - self.head - start,
                );

                let dst = (
                    // SAFETY: `len <= isize::MAX` and fits the memory range of `buf`
                    unsafe { buf_ptr.add(self.tail) },
                    // Dst length (see above diagram)
                    self.head - self.tail,
                );

                // SAFETY: `src` points at initialized data, `dst` points to writable memory
                // and does not overlap `src`.
                unsafe { copy_bytes_overshooting(src, dst, after_start) }

                if after_start < len {
                    // The read section was not continuous:
                    //
                    //                T           H
                    // Read:  SSXXXXXX____________XXSS
                    // Write: ________DDDD____________
                    //
                    // H: Head position (first readable byte)
                    // T: Tail position (first writable byte)
                    // X: Uninvolved bytes in the readable section
                    // S: Source bytes, to be copied to D bytes
                    // D: Destination bytes, going to be copied from S bytes
                    // _: Uninvolved bytes in the writable section

                    let src = (
                        buf_ptr.cast_const(),
                        // Src length - chunk 2 (see above diagram on the left)
                        self.tail,
                    );

                    let dst = (
                        // SAFETY: we are still within the memory range of `buf`
                        unsafe { dst.0.add(after_start) },
                        // Dst length (see above diagram)
                        dst.1 - after_start,
                    );

                    // SAFETY: `src` points at initialized data, `dst` points to writable memory
                    // and does not overlap `src`.
                    unsafe { copy_bytes_overshooting(src, dst, len - after_start) }
                }
            }
        }

        self.tail = (self.tail + len) % capacity;
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
#[inline(always)]
unsafe fn copy_bytes_overshooting(
    src: (*const u8, usize),
    dst: (*mut u8, usize),
    copy_at_least: usize,
) {
    // By default use usize as the copy size
    #[cfg(all(not(target_feature = "sse2"), not(target_feature = "neon")))]
    type CopyType = usize;

    // Use u128 if we detect a simd feature
    #[cfg(target_feature = "neon")]
    type CopyType = u128;
    #[cfg(target_feature = "sse2")]
    type CopyType = u128;

    const COPY_AT_ONCE_SIZE: usize = core::mem::size_of::<CopyType>();
    let min_buffer_size = usize::min(src.1, dst.1);

    // Can copy in just one read+write, very common case
    if min_buffer_size >= COPY_AT_ONCE_SIZE && copy_at_least <= COPY_AT_ONCE_SIZE {
        dst.0
            .cast::<CopyType>()
            .write_unaligned(src.0.cast::<CopyType>().read_unaligned())
    } else {
        let copy_multiple = copy_at_least.next_multiple_of(COPY_AT_ONCE_SIZE);
        // Can copy in multiple simple instructions
        if min_buffer_size >= copy_multiple {
            let mut src_ptr = src.0.cast::<CopyType>();
            let src_ptr_end = src.0.add(copy_multiple).cast::<CopyType>();
            let mut dst_ptr = dst.0.cast::<CopyType>();

            while src_ptr < src_ptr_end {
                dst_ptr.write_unaligned(src_ptr.read_unaligned());
                src_ptr = src_ptr.add(1);
                dst_ptr = dst_ptr.add(1);
            }
        } else {
            // Fall back to standard memcopy
            dst.0.copy_from_nonoverlapping(src.0, copy_at_least);
        }
    }

    debug_assert_eq!(
        slice::from_raw_parts(src.0, copy_at_least),
        slice::from_raw_parts(dst.0, copy_at_least)
    );
}

#[cfg(test)]
mod tests {
    use super::RingBuffer;

    #[test]
    fn smoke() {
        let mut rb = RingBuffer::new();

        rb.reserve(15);
        assert_eq!(17, rb.capacity());

        rb.extend(b"0123456789");
        assert_eq!(rb.len(), 10);
        assert_eq!(rb.as_slices().0, b"0123456789");
        assert_eq!(rb.as_slices().1, b"");

        rb.drop_first_n(5);
        assert_eq!(rb.len(), 5);
        assert_eq!(rb.as_slices().0, b"56789");
        assert_eq!(rb.as_slices().1, b"");

        rb.extend_from_within(2, 3);
        assert_eq!(rb.len(), 8);
        assert_eq!(rb.as_slices().0, b"56789789");
        assert_eq!(rb.as_slices().1, b"");

        rb.extend_from_within(0, 3);
        assert_eq!(rb.len(), 11);
        assert_eq!(rb.as_slices().0, b"56789789567");
        assert_eq!(rb.as_slices().1, b"");

        rb.extend_from_within(0, 2);
        assert_eq!(rb.len(), 13);
        assert_eq!(rb.as_slices().0, b"567897895675");
        assert_eq!(rb.as_slices().1, b"6");

        rb.drop_first_n(11);
        assert_eq!(rb.len(), 2);
        assert_eq!(rb.as_slices().0, b"5");
        assert_eq!(rb.as_slices().1, b"6");

        rb.extend(b"0123456789");
        assert_eq!(rb.len(), 12);
        assert_eq!(rb.as_slices().0, b"5");
        assert_eq!(rb.as_slices().1, b"60123456789");

        rb.drop_first_n(11);
        assert_eq!(rb.len(), 1);
        assert_eq!(rb.as_slices().0, b"9");
        assert_eq!(rb.as_slices().1, b"");

        rb.extend(b"0123456789");
        assert_eq!(rb.len(), 11);
        assert_eq!(rb.as_slices().0, b"9012345");
        assert_eq!(rb.as_slices().1, b"6789");
    }

    #[test]
    fn edge_cases() {
        // Fill exactly, then empty then fill again
        let mut rb = RingBuffer::new();
        rb.reserve(16);
        assert_eq!(17, rb.capacity());
        rb.extend(b"0123456789012345");
        assert_eq!(17, rb.capacity());
        assert_eq!(16, rb.len());
        assert_eq!(0, rb.free());
        rb.drop_first_n(16);
        assert_eq!(0, rb.len());
        assert_eq!(16, rb.free());
        rb.extend(b"0123456789012345");
        assert_eq!(16, rb.len());
        assert_eq!(0, rb.free());
        assert_eq!(17, rb.capacity());
        assert_eq!(1, rb.as_slices().0.len());
        assert_eq!(15, rb.as_slices().1.len());

        rb.clear();

        // data in both slices and then reserve
        rb.extend(b"0123456789012345");
        rb.drop_first_n(8);
        rb.extend(b"67890123");
        assert_eq!(16, rb.len());
        assert_eq!(0, rb.free());
        assert_eq!(17, rb.capacity());
        assert_eq!(9, rb.as_slices().0.len());
        assert_eq!(7, rb.as_slices().1.len());
        rb.reserve(1);
        assert_eq!(16, rb.len());
        assert_eq!(16, rb.free());
        assert_eq!(33, rb.capacity());
        assert_eq!(16, rb.as_slices().0.len());
        assert_eq!(0, rb.as_slices().1.len());

        rb.clear();

        // fill exactly, then extend from within
        rb.extend(b"0123456789012345");
        rb.extend_from_within(0, 16);
        assert_eq!(32, rb.len());
        assert_eq!(0, rb.free());
        assert_eq!(33, rb.capacity());
        assert_eq!(32, rb.as_slices().0.len());
        assert_eq!(0, rb.as_slices().1.len());

        // extend from within cases
        let mut rb = RingBuffer::new();
        rb.reserve(8);
        rb.extend(b"01234567");
        rb.drop_first_n(5);
        rb.extend_from_within(0, 3);
        assert_eq!(4, rb.as_slices().0.len());
        assert_eq!(2, rb.as_slices().1.len());

        rb.drop_first_n(2);
        assert_eq!(2, rb.as_slices().0.len());
        assert_eq!(2, rb.as_slices().1.len());
        rb.extend_from_within(0, 4);
        assert_eq!(2, rb.as_slices().0.len());
        assert_eq!(6, rb.as_slices().1.len());

        rb.drop_first_n(2);
        assert_eq!(6, rb.as_slices().0.len());
        assert_eq!(0, rb.as_slices().1.len());
        rb.drop_first_n(2);
        assert_eq!(4, rb.as_slices().0.len());
        assert_eq!(0, rb.as_slices().1.len());
        rb.extend_from_within(0, 4);
        assert_eq!(7, rb.as_slices().0.len());
        assert_eq!(1, rb.as_slices().1.len());

        let mut rb = RingBuffer::new();
        rb.reserve(8);
        rb.extend(b"11111111");
        rb.drop_first_n(7);
        rb.extend(b"111");
        assert_eq!(2, rb.as_slices().0.len());
        assert_eq!(2, rb.as_slices().1.len());
        rb.extend_from_within(0, 4);
        assert_eq!(b"11", rb.as_slices().0);
        assert_eq!(b"111111", rb.as_slices().1);
    }
}
