use alloc::collections::VecDeque;
use core::{cmp, hint::unreachable_unchecked, mem::MaybeUninit, slice};

pub struct RingBuffer {
    buf: VecDeque<MaybeUninit<u8>>,
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
    pub fn free(&self) -> usize {
        let len = self.buf.len();
        let capacity = self.buf.capacity();
        if len > capacity {
            unsafe { unreachable_unchecked() }
        }

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
        if self.free() < additional {
            self.reserve_amortized(additional);
        }

        if self.free() < additional {
            unsafe { unreachable_unchecked() }
        }
    }

    #[inline(never)]
    #[cold]
    fn reserve_amortized(&mut self, additional: usize) {
        self.buf.reserve(additional);
    }

    #[allow(dead_code)]
    pub fn push_back(&mut self, byte: u8) {
        self.reserve(1);
        self.buf.push_back(MaybeUninit::new(byte));
    }

    /// Fetch the byte stored at the selected index from the buffer, returning it, or
    /// `None` if the index is out of bounds.
    #[allow(dead_code)]
    pub fn get(&self, idx: usize) -> Option<u8> {
        self.buf
            .get(idx)
            .map(|&byte| unsafe { MaybeUninit::assume_init(byte) })
    }

    /// Append the provided data to the end of `self`.
    pub fn extend(&mut self, data: &[u8]) {
        let len = data.len();
        let data = data.as_ptr().cast::<MaybeUninit<u8>>();
        let data = unsafe { slice::from_raw_parts(data, len) };
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
        let (a, b) = self.buf.as_slices();

        (unsafe { slice_assume_init_ref_polyfill(a) }, unsafe {
            slice_assume_init_ref_polyfill(b)
        })
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
    pub unsafe fn extend_from_within_unchecked(&mut self, mut start: usize, len: usize) {
        debug_assert!(start + len <= self.len());
        debug_assert!(self.free() >= len);

        if self.free() < len {
            unsafe { unreachable_unchecked() }
        }

        let original_len = self.len();
        let mut intermediate = {
            IntermediateRingBuffer {
                this: self,
                original_len,
                disarmed: false,
            }
        };

        intermediate
            .this
            .buf
            .extend((0..len).map(|_| MaybeUninit::uninit()));
        debug_assert_eq!(intermediate.this.buf.len(), original_len + len);

        let (a, b, a_spare, b_spare) = intermediate.as_slices_spare_mut();
        debug_assert_eq!(a_spare.len() + b_spare.len(), len);

        let skip = cmp::min(a.len(), start);
        start -= skip;
        let a = &a[skip..];
        let b = unsafe { b.get_unchecked(start..) };

        let mut remaining_copy_len = len;

        // A -> A Spare
        let copy_at_least = cmp::min(cmp::min(a.len(), a_spare.len()), remaining_copy_len);
        copy_bytes_overshooting(a, a_spare, copy_at_least);
        remaining_copy_len -= copy_at_least;

        if remaining_copy_len == 0 {
            intermediate.disarmed = true;
            return;
        }

        let a = &a[copy_at_least..];
        let a_spare = &mut a_spare[copy_at_least..];

        // A -> B Spare
        let copy_at_least = cmp::min(cmp::min(a.len(), b_spare.len()), remaining_copy_len);
        copy_bytes_overshooting(a, b_spare, copy_at_least);
        remaining_copy_len -= copy_at_least;

        if remaining_copy_len == 0 {
            intermediate.disarmed = true;
            return;
        }

        let b_spare = &mut b_spare[copy_at_least..];

        // B -> A Spare
        let copy_at_least = cmp::min(cmp::min(b.len(), a_spare.len()), remaining_copy_len);
        copy_bytes_overshooting(b, a_spare, copy_at_least);
        remaining_copy_len -= copy_at_least;

        if remaining_copy_len == 0 {
            intermediate.disarmed = true;
            return;
        }

        let b = &b[copy_at_least..];

        // B -> B Spare
        let copy_at_least = cmp::min(cmp::min(b.len(), b_spare.len()), remaining_copy_len);
        copy_bytes_overshooting(b, b_spare, copy_at_least);
        remaining_copy_len -= copy_at_least;

        debug_assert_eq!(remaining_copy_len, 0);

        intermediate.disarmed = true;
    }
}

struct IntermediateRingBuffer<'a> {
    this: &'a mut RingBuffer,
    original_len: usize,
    disarmed: bool,
}

impl<'a> IntermediateRingBuffer<'a> {
    // inspired by `Vec::split_at_spare_mut`
    fn as_slices_spare_mut(
        &mut self,
    ) -> (&[u8], &[u8], &mut [MaybeUninit<u8>], &mut [MaybeUninit<u8>]) {
        let (a, b) = self.this.buf.as_mut_slices();
        debug_assert!(a.len() + b.len() >= self.original_len);

        let mut remaining_init_len = self.original_len;
        let a_mid = cmp::min(a.len(), remaining_init_len);
        remaining_init_len -= a_mid;
        let b_mid = remaining_init_len;
        debug_assert!(b.len() >= b_mid);

        let (a, a_spare) = unsafe { a.split_at_mut_unchecked(a_mid) };
        let (b, b_spare) = unsafe { b.split_at_mut_unchecked(b_mid) };
        debug_assert!(a_spare.is_empty() || b.is_empty());

        (
            unsafe { slice_assume_init_ref_polyfill(a) },
            unsafe { slice_assume_init_ref_polyfill(b) },
            a_spare,
            b_spare,
        )
    }
}

impl<'a> Drop for IntermediateRingBuffer<'a> {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }

        self.this.buf.truncate(self.original_len);
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
fn copy_bytes_overshooting(src: &[u8], dst: &mut [MaybeUninit<u8>], copy_at_least: usize) {
    // this assert is required for this function to be safe
    // the optimizer should be able to remove it given how the caller
    // has somehow to figure out `copy_at_least <= src.len() && copy_at_least <= dst.len()`
    assert!(src.len() >= copy_at_least && dst.len() >= copy_at_least);

    type CopyType = usize;

    const COPY_AT_ONCE_SIZE: usize = core::mem::size_of::<CopyType>();
    let min_buffer_size = usize::min(src.len(), dst.len());

    // this check should be removed by the optimizer thanks to the above assert
    // if `src.len() >= copy_at_least && dst.len() >= copy_at_least` then `min_buffer_size >= copy_at_least`
    assert!(min_buffer_size >= copy_at_least);

    // these bounds checks are removed because this is guaranteed:
    // `min_buffer_size <= src.len() && min_buffer_size <= dst.len()`
    let src = &src[..min_buffer_size];
    let dst = &mut dst[..min_buffer_size];

    // Can copy in just one read+write, very common case
    if min_buffer_size >= COPY_AT_ONCE_SIZE && copy_at_least <= COPY_AT_ONCE_SIZE {
        let chunk = unsafe { src.as_ptr().cast::<CopyType>().read_unaligned() };
        unsafe { dst.as_mut_ptr().cast::<CopyType>().write_unaligned(chunk) };
    } else {
        unsafe {
            dst.as_mut_ptr()
                .cast::<u8>()
                .copy_from_nonoverlapping(src.as_ptr(), copy_at_least)
        };
    }

    debug_assert_eq!(&src[..copy_at_least], unsafe {
        slice_assume_init_ref_polyfill(&dst[..copy_at_least])
    });
}

#[inline(always)]
unsafe fn slice_assume_init_ref_polyfill(slice: &[MaybeUninit<u8>]) -> &[u8] {
    let len = slice.len();
    let data = slice.as_ptr().cast::<u8>();
    slice::from_raw_parts(data, len)
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
