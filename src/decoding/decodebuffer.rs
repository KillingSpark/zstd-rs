use std::collections::VecDeque;
use std::hash::Hasher;
use std::mem::{self, MaybeUninit};
use std::{io, ptr, slice};

use twox_hash::XxHash64;

pub struct Decodebuffer {
    buffer: VecDeque<u8>,
    pub dict_content: Vec<u8>,

    pub window_size: usize,
    total_output_counter: u64,
    pub hash: XxHash64,
}

impl io::Read for Decodebuffer {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        let max_amount = self.can_drain_to_window_size().unwrap_or(0);
        let amount = max_amount.min(target.len());

        let mut written = 0;
        self.drain_to(amount, |buf| {
            target[written..][..buf.len()].copy_from_slice(buf);
            written += buf.len();
            (buf.len(), Ok(()))
        })?;
        Ok(amount)
    }
}

impl Decodebuffer {
    pub fn new(window_size: usize) -> Decodebuffer {
        Decodebuffer {
            buffer: VecDeque::new(),
            dict_content: Vec::new(),
            window_size,
            total_output_counter: 0,
            hash: XxHash64::with_seed(0),
        }
    }

    pub fn reset(&mut self, window_size: usize) {
        self.window_size = window_size;
        self.buffer.clear();
        self.buffer.reserve(self.window_size);
        self.dict_content.clear();
        self.total_output_counter = 0;
        self.hash = XxHash64::with_seed(0);
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buffer.extend(data);
        self.total_output_counter += data.len() as u64;
    }

    pub fn repeat(&mut self, offset: usize, match_length: usize) -> Result<(), String> {
        if offset > self.buffer.len() {
            if self.total_output_counter <= self.window_size as u64 {
                // at least part of that repeat is from the dictionary content
                let bytes_from_dict = offset - self.buffer.len();

                if bytes_from_dict > self.dict_content.len() {
                    return Err(format!(
                        "Need {} bytes from the dictionary but it is only {} bytes long",
                        bytes_from_dict,
                        self.dict_content.len()
                    ));
                }

                if bytes_from_dict < match_length {
                    let dict_slice =
                        &self.dict_content[self.dict_content.len() - bytes_from_dict..];
                    self.buffer.extend(dict_slice);

                    self.total_output_counter += bytes_from_dict as u64;
                    return self.repeat(self.buffer.len(), match_length - bytes_from_dict);
                } else {
                    let low = self.dict_content.len() - bytes_from_dict;
                    let high = low + match_length;
                    let dict_slice = &self.dict_content[low..high];
                    self.buffer.extend(dict_slice);
                }
            } else {
                return Err(format!(
                    "offset: {} bigger than buffer: {}",
                    offset,
                    self.buffer.len()
                ));
            }
        } else {
            let start_idx = self.buffer.len() - offset;

            self.buffer.reserve(match_length);

            if start_idx + match_length > self.buffer.len() {
                //need to copy byte by byte. can be optimized more but for now lets leave it like this
                //TODO batch whats possible
                for x in 0..match_length {
                    self.buffer.push_back(self.buffer[start_idx + x]);
                }
            } else {
                let mut buf = [MaybeUninit::<u8>::uninit(); 4096];

                // can just copy parts of the existing buffer

                let mut start_idx = start_idx;
                let mut match_length = match_length;
                while match_length > 0 {
                    let filled = {
                        let (slice1, slice2) = self.buffer.as_slices();

                        let slice = if slice1.len() > start_idx {
                            &slice1[start_idx..]
                        } else {
                            &slice2[start_idx - slice1.len()..]
                        };
                        let slice = &slice[..match_length.min(slice.len()).min(buf.len())];

                        // TODO: replace with MaybeUninit::write_slice once it's stable.
                        // SAFETY: we initialize a portion of `buf` and then we return a slice
                        // of the initialized portion of `buf`.
                        unsafe {
                            debug_assert!(slice.len() <= buf.len());

                            ptr::copy_nonoverlapping(
                                slice.as_ptr().cast::<MaybeUninit<u8>>(),
                                buf.as_mut_ptr(),
                                slice.len(),
                            );
                            slice::from_raw_parts(buf.as_ptr().cast::<u8>(), slice.len())
                        }
                    };

                    self.buffer.extend(filled);
                    start_idx += filled.len();
                    match_length -= filled.len();
                }
            }

            self.total_output_counter += match_length as u64;
        }

        Ok(())
    }

    // Check if and how many bytes can currently be drawn from the buffer
    pub fn can_drain_to_window_size(&self) -> Option<usize> {
        if self.buffer.len() > self.window_size {
            Some(self.buffer.len() - self.window_size)
        } else {
            None
        }
    }

    //How many bytes can be drained if the windowsize does not have to be maintained
    pub fn can_drain(&self) -> usize {
        self.buffer.len()
    }

    //drain as much as possible while retaining enough so that decoding si still possible with the requeired windowsize
    //At best call only if can_drain_to_window_size reports a 'high' number of bytes to reduce allocations
    pub fn drain_to_window_size(&mut self) -> Option<Vec<u8>> {
        //TODO investigate if it is possible to return the std::vec::Drain iterator directly without collecting here
        match self.can_drain_to_window_size() {
            None => None,
            Some(can_drain) => {
                let mut vec = Vec::with_capacity(can_drain);
                self.drain_to(can_drain, |buf| {
                    vec.extend_from_slice(buf);
                    (buf.len(), Ok(()))
                })
                .ok()?;
                Some(vec)
            }
        }
    }

    pub fn drain_to_window_size_writer(&mut self, mut sink: impl io::Write) -> io::Result<usize> {
        match self.can_drain_to_window_size() {
            None => Ok(0),
            Some(can_drain) => {
                self.drain_to(can_drain, |buf| write_all_bytes(&mut sink, buf))?;
                Ok(can_drain)
            }
        }
    }

    //drain the buffer completely
    pub fn drain(&mut self) -> Vec<u8> {
        let new_buffer = VecDeque::with_capacity(self.buffer.capacity());

        let (slice1, slice2) = self.buffer.as_slices();
        self.hash.write(slice1);
        self.hash.write(slice2);

        mem::replace(&mut self.buffer, new_buffer).into()
    }

    pub fn drain_to_writer(&mut self, mut sink: impl io::Write) -> io::Result<usize> {
        let len = self.buffer.len();
        self.drain_to(len, |buf| write_all_bytes(&mut sink, buf))?;

        Ok(len)
    }

    pub fn read_all(&mut self, target: &mut [u8]) -> io::Result<usize> {
        let amount = self.buffer.len().min(target.len());

        let mut written = 0;
        self.drain_to(amount, |buf| {
            target[written..][..buf.len()].copy_from_slice(buf);
            written += buf.len();
            (buf.len(), Ok(()))
        })?;
        Ok(amount)
    }

    /// Semantics of write_bytes:
    /// Should dump as many of the provided bytes as possible to whatever sink until no bytes are left or an error is encountered
    /// Return how many bytes have actually been dumped to the sink.
    fn drain_to(
        &mut self,
        amount: usize,
        mut write_bytes: impl FnMut(&[u8]) -> (usize, io::Result<()>),
    ) -> io::Result<()> {
        if amount == 0 {
            return Ok(());
        }

        struct DrainGuard<'a> {
            buffer: &'a mut VecDeque<u8>,
            amount: usize,
        }

        impl<'a> Drop for DrainGuard<'a> {
            fn drop(&mut self) {
                if self.amount != 0 {
                    self.buffer.drain(..self.amount);
                }
            }
        }

        let mut drain_guard = DrainGuard {
            buffer: &mut self.buffer,
            amount: 0,
        };

        let (slice1, slice2) = drain_guard.buffer.as_slices();
        let n1 = slice1.len().min(amount);
        let n2 = slice2.len().min(amount - n1);

        if n1 != 0 {
            let (written1, res1) = write_bytes(&slice1[..n1]);
            self.hash.write(&slice1[..written1]);
            drain_guard.amount += written1;

            if res1.is_err() {
                return res1;
            }

            // Only if the first call to write_bytes was not a partial write we can continue with slice2
            // Partial writes SHOULD never happen without res1 being an error, but lets just protect against it anyways.
            if written1 == n1 {
                if n2 != 0 {
                    let (written2, res2) = write_bytes(&slice2[..n2]);
                    self.hash.write(&slice2[..written2]);
                    drain_guard.amount += written2;
                    if res2.is_err() {
                        return res2;
                    }
                }
            }
        }

        // Make sure we don't accidentally drop `DrainGuard` earlier.
        drop(drain_guard);

        Ok(())
    }
}

/// Like Write::write_all but returns partial write length even on error
fn write_all_bytes(mut sink: impl io::Write, buf: &[u8]) -> (usize, io::Result<()>) {
    let mut written = 0;
    while written < buf.len() {
        match sink.write(buf) {
            Ok(w) => written += w,
            Err(e) => return (written, Err(e)),
        }
    }
    (written, Ok(()))
}
