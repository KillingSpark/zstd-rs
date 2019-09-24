pub struct Decodebuffer {
    buffer: Vec<u8>,

    window_size: usize,
}

impl std::io::Read for Decodebuffer {
    fn read(&mut self, target: &mut [u8]) -> std::result::Result<usize, std::io::Error> {
        let max_amount = match self.can_drain_to_window_size() {
            Some(x) => x,
            None => 0,
        };

        let amount = if max_amount > target.len() {
            target.len()
        } else {
            max_amount
        };

        if amount == 0 {
            return Ok(0);
        }

        let mut buffer_slice = self.buffer.as_slice();
        buffer_slice.read_exact(&mut target[..amount])?;
        self.buffer.drain(0..amount);

        Ok(amount)
    }
}

impl Decodebuffer {
    pub fn new(window_size: usize) -> Decodebuffer {
        Decodebuffer {
            buffer: Vec::new(),
            window_size: window_size,
        }
    }

    pub fn reset(&mut self, window_size: usize) {
        self.window_size = window_size;
        self.buffer.clear();
        self.buffer.reserve(self.window_size);
    } 

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    pub fn repeat(&mut self, offset: usize, match_length: usize) -> Result<(), String> {
        if offset > self.window_size {
            return Err(format!(
                "offset: {} bigger than windowsize: {}",
                offset, self.window_size
            ));
        }
        if offset > self.buffer.len() {
            return Err(format!(
                "offset: {} bigger than buffer: {}",
                offset,
                self.buffer.len()
            ));
        }

        let start_idx = self.buffer.len() - offset;
        self.buffer.reserve(match_length);
        for x in 0..match_length {
            self.buffer.push(self.buffer[start_idx + x]);
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
            Some(can_drain) => Some(self.buffer.drain(0..can_drain).collect()),
        }
    }

    pub fn drain_to_window_size_writer(
        &mut self,
        sink: &mut std::io::Write,
    ) -> Result<usize, std::io::Error> {
        match self.can_drain_to_window_size() {
            None => Ok(0),
            Some(can_drain) => {
                let mut buf = [0u8; 1]; //TODO batch to reasonable size
                for x in self.buffer.drain(0..can_drain) {
                    buf[0] = x;
                    sink.write_all(&buf[..])?;
                }
                Ok(can_drain)
            }
        }
    }

    //drain the buffer completely
    pub fn drain(&mut self) -> Vec<u8> {
        let r = self.buffer.clone();
        self.buffer.clear();
        r
    }

    pub fn drain_to_writer(&mut self, sink: &mut std::io::Write) -> Result<usize, std::io::Error> {
        let mut buf = [0u8; 1]; //TODO batch to reasonable size
        for x in &self.buffer {
            buf[0] = *x;
            sink.write_all(&buf[..])?;
        }
        let len = self.buffer.len();
        self.buffer.clear();
        Ok(len)
    }

    pub fn read_all(&mut self, target: &mut [u8]) -> Result<usize, std::io::Error> {
        let amount = if self.buffer.len() > target.len() {
            target.len()
        } else {
            self.buffer.len()
        };

        if amount == 0 {
            return Ok(0);
        }

        use std::io::Read;
        let mut buffer_slice = self.buffer.as_slice();
        buffer_slice.read_exact(&mut target[..amount])?;
        self.buffer.drain(0..amount);

        Ok(amount)
    }
}
