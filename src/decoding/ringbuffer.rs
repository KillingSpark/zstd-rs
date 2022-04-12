use std::{alloc::Layout, ptr::slice_from_raw_parts};

pub struct RingBuffer {
    buf: *mut u8,
    layout: Layout,
    cap: usize,
    head: usize,
    tail: usize,
}

impl RingBuffer {
    pub fn new() -> Self {
        RingBuffer {
            buf: std::ptr::null_mut(),
            layout: Layout::new::<u8>(),
            cap: 0,
            head: 0,
            tail: 0,
        }
    }

    pub fn len(&self) -> usize {
        let (x, y) = self.data_slice_lengths();
        x + y
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    pub fn reserve(&mut self, amount: usize) {
        if self.cap - self.len() > amount {
            return;
        }

        // TODO make this the next biggest 2^x?
        let new_cap = usize::max(self.cap * 2, self.cap + amount + 1);
        let new_layout = Layout::array::<u8>(new_cap).unwrap();
        let new_buf = unsafe { std::alloc::alloc(new_layout) };

        if new_buf != std::ptr::null_mut() {
            if self.cap > 0 {
                let ((s1_ptr, s1_len), (s2_ptr, s2_len)) = self.data_slice_parts();
                unsafe {
                    new_buf.copy_from_nonoverlapping(s1_ptr, s1_len);
                    new_buf.add(s1_len).copy_from_nonoverlapping(s2_ptr, s2_len);
                    std::alloc::dealloc(self.buf, self.layout);
                }
                self.tail = s1_len + s2_len;
                self.head = 0;
            }
            self.buf = new_buf;
            self.layout = new_layout;
            self.cap = new_cap;
        }
    }

    pub fn push_back(&mut self, byte: u8) {
        self.reserve(1);
        unsafe { self.buf.add(self.tail).write(byte) };
        self.tail = (self.tail + 1) % self.cap;
    }

    pub fn get(&self, idx: usize) -> Option<u8> {
        if idx < self.len() {
            let idx = (self.head + idx) % self.cap;
            Some(unsafe { self.buf.add(idx).read() })
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn extend(&mut self, data: &[u8]) {
        let len = data.len();
        let ptr = data.as_ptr();

        self.reserve(len);
        let ((f1_ptr, f1_len), (f2_ptr, f2_len)) = self.free_slice_parts();
        debug_assert!(f1_len + f2_len >= len, "{} + {} < {}", f1_len, f2_len, len);

        let in_f1 = usize::min(len, f1_len);
        let in_f2 = len - in_f1;

        debug_assert!(in_f1 + in_f2 == len);

        unsafe {
            f1_ptr.copy_from_nonoverlapping(ptr, in_f1);
            f2_ptr.copy_from_nonoverlapping(ptr.add(in_f1), in_f2);
        }
        self.tail = (self.tail + len) % self.cap;
    }

    pub fn drain(&mut self, amount: usize) {
        if amount > self.len() {
            panic!("Thats illegal");
        }
        self.head = (self.head + amount) % self.cap;
    }

    fn data_slice_lengths(&self) -> (usize, usize) {
        let len_after_head;
        let len_to_tail;

        // TODO can we do this branchless?
        if self.tail >= self.head {
            len_after_head = self.tail - self.head;
            len_to_tail = 0;
        } else {
            len_after_head = self.cap - self.head;
            len_to_tail = self.tail;
        }
        (len_after_head, len_to_tail)
    }

    fn data_slice_parts(&self) -> ((*const u8, usize), (*const u8, usize)) {
        let (len_after_head, len_to_tail) = self.data_slice_lengths();

        (
            (unsafe { self.buf.add(self.head) }, len_after_head),
            (self.buf, len_to_tail),
        )
    }
    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        let (s1, s2) = self.data_slice_parts();
        unsafe {
            let s1 = &*slice_from_raw_parts(s1.0, s1.1);
            let s2 = &*slice_from_raw_parts(s2.0, s2.1);
            (s1, s2)
        }
    }

    fn free_slice_lengths(&self) -> (usize, usize) {
        let len_to_head;
        let len_after_tail;

        // TODO can we do this branchless?
        if self.tail < self.head {
            len_after_tail = self.head - self.tail;
            len_to_head = 0;
        } else {
            len_after_tail = self.cap - self.tail;
            len_to_head = self.head;
        }
        (len_to_head, len_after_tail)
    }

    fn free_slice_parts(&self) -> ((*mut u8, usize), (*mut u8, usize)) {
        let (len_to_head, len_after_tail) = self.free_slice_lengths();

        (
            (unsafe { self.buf.add(self.tail) }, len_after_tail),
            (self.buf, len_to_head),
        )
    }

    #[inline(always)]
    pub fn extend_from_within(&mut self, start: usize, len: usize) {
        if start > self.len() || start + len > self.len() {
            panic!("This is illegal!");
        }

        self.reserve(len);

        // data slices in raw parts
        let ((s1_ptr, s1_len), (s2_ptr, s2_len)) = self.data_slice_parts();

        debug_assert!(len <= s1_len + s2_len, "{} > {} + {}", len, s1_len, s2_len);

        // calc the actually wanted slices in raw parts
        let start_in_s1 = usize::min(s1_len, start);
        let end_in_s1 = usize::min(s1_len, start + len);
        let m1_ptr = unsafe { s1_ptr.add(start_in_s1) };
        let m1_len = end_in_s1 - start_in_s1;

        debug_assert!(end_in_s1 <= s1_len);
        debug_assert!(start_in_s1 <= s1_len);

        let start_in_s2 = start.saturating_sub(s1_len);
        let end_in_s2 = start_in_s2 + (len - m1_len);
        let m2_ptr = unsafe { s2_ptr.add(start_in_s2) };
        let m2_len = end_in_s2 - start_in_s2;

        debug_assert!(start_in_s2 <= s2_len);
        debug_assert!(end_in_s2 <= s2_len);

        debug_assert_eq!(len, m1_len + m2_len);

        // the free slices, must hold: f1_len + f2_len >= m1_len + m2_len
        let ((f1_ptr, f1_len), (f2_ptr, f2_len)) = self.free_slice_parts();

        debug_assert!(f1_len + f2_len >= m1_len + m2_len);

        // calc how many from where bytes go where
        let m1_in_f1 = usize::min(m1_len, f1_len);
        let m1_in_f2 = m1_len - m1_in_f1;
        let m2_in_f1 = usize::min(f1_len - m1_in_f1, m2_len);
        let m2_in_f2 = m2_len - m2_in_f1;

        debug_assert_eq!(m1_len, m1_in_f1 + m1_in_f2);
        debug_assert_eq!(m2_len, m2_in_f1 + m2_in_f2);
        debug_assert!(f1_len >= m1_in_f1 + m2_in_f1);
        debug_assert!(f2_len >= m1_in_f2 + m2_in_f2);
        debug_assert_eq!(len, m1_in_f1 + m2_in_f1 + m1_in_f2 + m2_in_f2);

        debug_assert!((m1_in_f2 > 0) ^ (m2_in_f1 > 0) || (m1_in_f2 == 0 && m2_in_f1 == 0));

        unsafe {
            copy_with_checks(
                m1_ptr, m2_ptr, f1_ptr, f2_ptr, m1_in_f1, m2_in_f1, m1_in_f2, m2_in_f2,
            );
        }

        self.tail = (self.tail + len) % self.cap;
    }
}

#[allow(dead_code)]
#[inline(always)]
unsafe fn copy_without_checks(
    m1_ptr: *const u8,
    m2_ptr: *const u8,
    f1_ptr: *mut u8,
    f2_ptr: *mut u8,
    m1_in_f1: usize,
    m2_in_f1: usize,
    m1_in_f2: usize,
    m2_in_f2: usize,
) {
    f1_ptr.copy_from_nonoverlapping(m1_ptr, m1_in_f1);
    f1_ptr
        .add(m1_in_f1)
        .copy_from_nonoverlapping(m2_ptr, m2_in_f1);

    f2_ptr.copy_from_nonoverlapping(m1_ptr.add(m1_in_f1), m1_in_f2);
    f2_ptr
        .add(m1_in_f2)
        .copy_from_nonoverlapping(m2_ptr.add(m2_in_f1), m2_in_f2);
}

#[allow(dead_code)]
#[inline(always)]
unsafe fn copy_with_checks(
    m1_ptr: *const u8,
    m2_ptr: *const u8,
    f1_ptr: *mut u8,
    f2_ptr: *mut u8,
    m1_in_f1: usize,
    m2_in_f1: usize,
    m1_in_f2: usize,
    m2_in_f2: usize,
) {
    if m1_in_f1 != 0 {
        f1_ptr.copy_from_nonoverlapping(m1_ptr, m1_in_f1);
    }
    if m2_in_f1 != 0 {
        f1_ptr
            .add(m1_in_f1)
            .copy_from_nonoverlapping(m2_ptr, m2_in_f1);
    }

    if m1_in_f2 != 0 {
        f2_ptr.copy_from_nonoverlapping(m1_ptr.add(m1_in_f1), m1_in_f2);
    }
    if m2_in_f2 != 0 {
        f2_ptr
            .add(m1_in_f2)
            .copy_from_nonoverlapping(m2_ptr.add(m2_in_f1), m2_in_f2);
    }
}

#[allow(dead_code)]
#[inline(always)]
unsafe fn copy_with_nobranch_check(
    m1_ptr: *const u8,
    m2_ptr: *const u8,
    f1_ptr: *mut u8,
    f2_ptr: *mut u8,
    m1_in_f1: usize,
    m2_in_f1: usize,
    m1_in_f2: usize,
    m2_in_f2: usize,
) {
    let case = (m1_in_f1 > 0) as usize
        | (((m2_in_f1 > 0) as usize) << 1)
        | (((m1_in_f2 > 0) as usize) << 2)
        | (((m2_in_f2 > 0) as usize) << 3);

    match case {
        0 => {}

        // one bit set
        1 => {
            f1_ptr.copy_from_nonoverlapping(m1_ptr, m1_in_f1);
        }
        2 => {
            f1_ptr.copy_from_nonoverlapping(m2_ptr, m2_in_f1);
        }
        4 => {
            f2_ptr.copy_from_nonoverlapping(m1_ptr, m1_in_f2);
        }
        8 => {
            f2_ptr.copy_from_nonoverlapping(m2_ptr, m2_in_f2);
        }

        // two bit set
        3 => {
            f1_ptr.copy_from_nonoverlapping(m1_ptr, m1_in_f1);
            f1_ptr
                .add(m1_in_f1)
                .copy_from_nonoverlapping(m2_ptr, m2_in_f1);
        }
        5 => {
            f1_ptr.copy_from_nonoverlapping(m1_ptr, m1_in_f1);
            f2_ptr.copy_from_nonoverlapping(m1_ptr.add(m1_in_f1), m1_in_f2);
        }
        6 => std::hint::unreachable_unchecked(),
        7 => std::hint::unreachable_unchecked(),
        9 => {
            f1_ptr.copy_from_nonoverlapping(m1_ptr, m1_in_f1);
            f2_ptr.copy_from_nonoverlapping(m2_ptr, m2_in_f2);
        }
        10 => {
            f1_ptr.copy_from_nonoverlapping(m2_ptr, m2_in_f1);
            f2_ptr.copy_from_nonoverlapping(m2_ptr.add(m2_in_f1), m2_in_f2);
        }
        12 => {
            f2_ptr.copy_from_nonoverlapping(m1_ptr, m1_in_f2);
            f2_ptr
                .add(m1_in_f2)
                .copy_from_nonoverlapping(m2_ptr, m2_in_f2);
        }

        // three bit set
        11 => {
            f1_ptr.copy_from_nonoverlapping(m1_ptr, m1_in_f1);
            f1_ptr
                .add(m1_in_f1)
                .copy_from_nonoverlapping(m2_ptr, m2_in_f1);
            f2_ptr.copy_from_nonoverlapping(m2_ptr.add(m2_in_f1), m2_in_f2);
        }
        13 => {
            f1_ptr.copy_from_nonoverlapping(m1_ptr, m1_in_f1);
            f2_ptr.copy_from_nonoverlapping(m1_ptr.add(m1_in_f1), m1_in_f2);
            f2_ptr
                .add(m1_in_f2)
                .copy_from_nonoverlapping(m2_ptr, m2_in_f2);
        }
        14 => std::hint::unreachable_unchecked(),
        15 => std::hint::unreachable_unchecked(),
        _ => std::hint::unreachable_unchecked(),
    }
}

#[test]
fn smoke() {
    let mut rb = RingBuffer::new();

    rb.extend(b"abcdefghijklmnop");
    assert_eq!(16, rb.len());
    assert_eq!(rb.as_slices().0, b"abcdefghijklmnop");
    assert_eq!(rb.as_slices().1, b"");

    rb.extend_from_within(4, 6);
    assert_eq!(22, rb.len());
    assert_eq!(rb.as_slices().0, b"abcdefghijklmnopefghij");
    assert_eq!(rb.as_slices().1, b"");

    rb.drain(6);
    assert_eq!(16, rb.len());
    assert_eq!(rb.as_slices().0, b"ghijklmnopefghij");
    assert_eq!(rb.as_slices().1, b"");

    rb.extend_from_within(4, 6);
    assert_eq!(22, rb.len());
    assert_eq!(rb.as_slices().0, b"ghijklmnopefghijklmnop");
    assert_eq!(rb.as_slices().1, b"");

    rb.extend_from_within(4, 10);
    assert_eq!(32, rb.len());
    assert_eq!(rb.as_slices().0, b"ghijklmnopefghijklmnopklmnop");
    assert_eq!(rb.as_slices().1, b"efgh");

    rb.extend(b"1");
    assert_eq!(33, rb.len());
    assert_eq!(rb.as_slices().0, b"ghijklmnopefghijklmnopklmnop");
    assert_eq!(rb.as_slices().1, b"efgh1");

    rb.drain(9);
    assert_eq!(24, rb.len());
    assert_eq!(rb.as_slices().0, b"pefghijklmnopklmnop");
    assert_eq!(rb.as_slices().1, b"efgh1");

    rb.extend(b"234567890");
    assert_eq!(33, rb.len());
    assert_eq!(rb.as_slices().0, b"pefghijklmnopklmnop");
    assert_eq!(rb.as_slices().1, b"efgh1234567890");

    rb.drain(11);
    assert_eq!(22, rb.len());
    assert_eq!(rb.as_slices().0, b"opklmnop");
    assert_eq!(rb.as_slices().1, b"efgh1234567890");

    rb.extend_from_within(12, 10);
    assert_eq!(32, rb.len());
    assert_eq!(rb.as_slices().0, b"opklmnop");
    assert_eq!(rb.as_slices().1, b"efgh12345678901234567890");

    rb.drain(10);
    assert_eq!(22, rb.len());
    assert_eq!(rb.as_slices().0, b"gh12345678901234567890");
    assert_eq!(rb.as_slices().1, b"");
}
