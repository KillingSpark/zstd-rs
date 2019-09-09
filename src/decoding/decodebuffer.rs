pub struct Decodebuffer {
    buffer: Vec<u8>,

    window_size: usize,
}

impl Decodebuffer {
    pub fn new(window_size: usize) -> Decodebuffer {
        Decodebuffer {
            buffer: Vec::new(),
            window_size: window_size,
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
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

    //drain the buffer completely
    pub fn drain(&mut self) -> Vec<u8> {
        let r = self.buffer.clone();
        self.buffer.clear();
        r
    }
}
