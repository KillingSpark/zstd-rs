use std::collections::VecDeque;
use std::hash::Hasher;
use std::{io, mem};

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

        let amount = if max_amount > target.len() {
            target.len()
        } else {
            max_amount
        };

        let mut written = 0;
        self.drain_to(amount, |buf| {
            target[written..][..buf.len()].copy_from_slice(buf);
            written += buf.len();
            Ok(())
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

            // if start_idx + match_length > self.buffer.len() {
            self.buffer.reserve(match_length);
            //need to copy byte by byte. can be optimized more but for now lets leave it like this
            //TODO batch whats possible
            for x in 0..match_length {
                self.buffer.push_back(self.buffer[start_idx + x]);
            }
            // TODO: bring this back
            //} else {
            //    // can just copy parts of the existing buffer,
            //    // which is exactly what Vec::extend_from_within was create for
            //    let end_idx = start_idx + match_length;
            //    self.buffer.extend_from_within(start_idx..end_idx);
            //}
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
                let mut vec = Vec::new();
                self.drain_to(can_drain, |buf| {
                    vec.extend_from_slice(buf);
                    Ok(())
                })
                .ok()?;
                Some(vec)
            }
        }
    }

    pub fn drain_to_window_size_writer(&mut self, sink: &mut dyn io::Write) -> io::Result<usize> {
        match self.can_drain_to_window_size() {
            None => Ok(0),
            Some(can_drain) => {
                self.drain_to(can_drain, |buf| {
                    sink.write_all(buf)?;
                    Ok(())
                })?;
                Ok(can_drain)
            }
        }
    }

    //drain the buffer completely
    pub fn drain(&mut self) -> Vec<u8> {
        let (slice1, slice2) = self.buffer.as_slices();
        self.hash.write(slice1);
        self.hash.write(slice2);

        let new_buffer = VecDeque::with_capacity(self.buffer.capacity());
        mem::replace(&mut self.buffer, new_buffer).into()
    }

    pub fn drain_to_writer(&mut self, sink: &mut dyn io::Write) -> io::Result<usize> {
        let (slice1, slice2) = self.buffer.as_slices();

        self.hash.write(slice1);
        self.hash.write(slice2);
        sink.write_all(slice1)?;
        sink.write_all(slice2)?;

        let len = self.buffer.len();
        self.buffer.clear();
        Ok(len)
    }

    pub fn read_all(&mut self, target: &mut [u8]) -> io::Result<usize> {
        let amount = if self.buffer.len() > target.len() {
            target.len()
        } else {
            self.buffer.len()
        };

        let mut written = 0;
        self.drain_to(amount, |buf| {
            target[written..][..buf.len()].copy_from_slice(buf);
            written += buf.len();
            Ok(())
        })?;
        Ok(amount)
    }

    fn drain_to(
        &mut self,
        amount: usize,
        mut f: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        if amount == 0 {
            return Ok(());
        }

        let (slice1, slice2) = self.buffer.as_slices();
        let n1 = slice1.len().min(amount);
        let n2 = slice2.len().min(amount - n1);

        self.hash.write(&slice1[..n1]);
        self.hash.write(&slice2[..n2]);

        f(&slice1[..n1])?;
        f(&slice2[..n2])?;

        self.buffer.drain(..amount);
        Ok(())
    }
}
