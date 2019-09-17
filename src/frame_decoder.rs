use super::frame;
use crate::decoding;
use crate::decoding::scratch::DecoderScratch;
use std::io::Read;

pub struct FrameDecoder {
    state: Option<FrameDecoderState>,
}

struct FrameDecoderState {
    pub frame: frame::Frame,
    decoder_scratch: DecoderScratch,
    frame_finished: bool,
    block_counter: usize,
    byte_counter: u64,
}

pub enum BlockDecodingStrategy {
    All,
    UptoBlocks(usize),
    UptoBytes(usize),
}

const MAX_WINDOW_SIZE: u64 = 1024*1024*100;

impl FrameDecoderState {
    pub fn new(source: &mut Read) -> Result<FrameDecoderState, String> {
        let (frame, header_size) = frame::read_frame_header(source)?;
        let window_size = frame.header.window_size()?;
        frame.check_valid()?;
        Ok(FrameDecoderState {
            frame: frame,
            frame_finished: false,
            block_counter: 0,
            decoder_scratch: DecoderScratch::new(window_size as usize),
            byte_counter: header_size as u64,
        })
    }

    pub fn reset(&mut self, source: &mut Read) -> Result<(), String> {
        let (frame, header_size) = frame::read_frame_header(source)?;
        let window_size = frame.header.window_size()?;
        frame.check_valid()?;

        if window_size > MAX_WINDOW_SIZE {
            return Err(format!("Dont support window_sizes (requested: {}) over: {}", window_size, MAX_WINDOW_SIZE));
        }

        self.frame = frame;
        self.frame_finished = false;
        self.block_counter = 0;
        self.decoder_scratch.reset(window_size as usize);
        self.byte_counter = header_size as u64;
        Ok(())
    }
}

impl FrameDecoder {
    pub fn new() -> FrameDecoder {
        FrameDecoder{state: None}
    }

    pub fn reset(&mut self, source: &mut Read) -> Result<(), String> {
        match &mut self.state {
            Some(s) => s.reset(source),
            None => {
                self.state = Some(FrameDecoderState::new(source)?);
                Ok(())
            }
        }
    }

    pub fn content_size(&self) -> Option<u64> {
        let state = match &self.state {
            None => return None,
            Some(s) => s,
        };

        match state.frame.header.frame_content_size() {
            Err(_) => None,
            Ok(x) => Some(x),
        }
    }

    pub fn bytes_read_from_source(&self) -> u64 {
        let state = match &self.state {
            None => return 0,
            Some(s) => s,
        };
        state.byte_counter
    }

    pub fn is_finished(&self) -> bool {
        let state = match &self.state {
            None => return true,
            Some(s) => s,
        };
        state.frame_finished
    }

    pub fn blocks_decoded(&self) -> usize {
        let state = match &self.state {
            None => return 0,
            Some(s) => s,
        };
        state.block_counter
    }

    pub fn decode_blocks(
        &mut self,
        source: &mut Read,
        strat: BlockDecodingStrategy,
    ) -> Result<bool, crate::errors::FrameDecoderError> {
        let state = match &mut self.state {
            None => return Err(crate::errors::FrameDecoderError::NotYetInitialized),
            Some(s) => s,
        };

        let mut block_dec = decoding::block_decoder::new();

        let buffer_size_before = state.decoder_scratch.buffer.len();
        let block_counter_before = state.block_counter;
        loop {
            if crate::VERBOSE {
                println!("################");
                println!("Next Block: {}", state.block_counter);
                println!("################");
            }
            let (block_header, block_header_size) = match block_dec.read_block_header(source) {
                Ok(h) => h,
                Err(m) => return Err(crate::errors::FrameDecoderError::FailedToReadBlockHeader(m)),
            };
            state.byte_counter += block_header_size as u64;

            if crate::VERBOSE {
                println!("");
                println!(
                    "Found {} block with size: {}, which will be of size: {}",
                    block_header.block_type,
                    block_header.content_size,
                    block_header.decompressed_size
                );
            }

            let bytes_read_in_block_body = match block_dec.decode_block_content(
                &block_header,
                &mut state.decoder_scratch,
                source,
            ) {
                Ok(h) => h,
                Err(m) => return Err(crate::errors::FrameDecoderError::FailedToReadBlockBody(m)),
            };
            state.byte_counter += bytes_read_in_block_body;

            state.block_counter += 1;

            if crate::VERBOSE {
                println!("Output: {}", state.decoder_scratch.buffer.len());
            }

            if block_header.last_block {
                state.frame_finished = true;
                if state.frame.header.descriptor.content_checksum_flag() {
                    let mut chksum = [0u8; 4];
                    match source.read_exact(&mut chksum[..]) {
                        Err(_) => {
                            return Err(crate::errors::FrameDecoderError::FailedToReadChecksum)
                        }
                        Ok(()) => {
                            state.byte_counter += 4;
                            //TODO checksum
                        }
                    };
                }
                break;
            }

            match strat {
                BlockDecodingStrategy::All => { /* keep going */ }
                BlockDecodingStrategy::UptoBlocks(n) => {
                    if state.block_counter - block_counter_before >= n {
                        break;
                    }
                }
                BlockDecodingStrategy::UptoBytes(n) => {
                    if state.decoder_scratch.buffer.len() - buffer_size_before >= n {
                        break;
                    }
                }
            }
        }

        Ok(state.frame_finished)
    }

    //collect is for collecting bytes and retain window_size bytes while decoding is still going on
    pub fn collect(&mut self) -> Option<Vec<u8>> {
        let state = match &mut self.state {
            None => return None,
            Some(s) => s,
        };
        state.decoder_scratch.buffer.drain_to_window_size()
    }

    //collect is for collecting bytes and retain window_size bytes while decoding is still going on
    pub fn collect_to_writer(&mut self, w: &mut std::io::Write) -> Result<usize, std::io::Error> {
        let state = match &mut self.state {
            None => return Ok(0),
            Some(s) => s,
        };
        state.decoder_scratch.buffer.drain_to_window_size_writer(w)
    }

    //drain is for collecting all bytes after decoding has been finished
    pub fn drain_buffer(&mut self) -> Vec<u8> {
        let state = match &mut self.state {
            None => return Vec::new(),
            Some(s) => s,
        };
        state.decoder_scratch.buffer.drain()
    }

    //drain is for collecting all bytes after decoding has been finished
    pub fn drain_buffer_to_writer(
        &mut self,
        w: &mut std::io::Write,
    ) -> Result<usize, std::io::Error> {
        let state = match &mut self.state {
            None => return Ok(0),
            Some(s) => s,
        };
        state.decoder_scratch.buffer.drain_to_writer(w)
    }

    pub fn can_collect(&self) -> usize {
        let state = match &self.state {
            None => return 0,
            Some(s) => s,
        };
        match state.decoder_scratch.buffer.can_drain_to_window_size() {
            Some(x) => x,
            None => 0,
        }
    }

    pub fn can_drain(&self) -> usize {
        let state = match &self.state {
            None => return 0,
            Some(s) => s,
        };
        state.decoder_scratch.buffer.can_drain()
    }
}

// Read bytes from the decode_buffer that are no longer needed. While the frame is not ye finished
// this will retain window_size bytes, else it will drain it completely
impl std::io::Read for FrameDecoder {
    fn read(&mut self, target: &mut [u8]) -> std::result::Result<usize, std::io::Error> {
        let state = match &mut self.state {
            None => return Ok(0),
            Some(s) => s,
        };
        if state.frame_finished {
            Ok(state.decoder_scratch.buffer.read_all(target))
        } else {
            state.decoder_scratch.buffer.read(target)
        }
    }
}
