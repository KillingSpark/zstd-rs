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
    bytes_read_counter: u64,
}

pub enum BlockDecodingStrategy {
    All,
    UptoBlocks(usize),
    UptoBytes(usize),
}

const MAX_WINDOW_SIZE: u64 = 1024 * 1024 * 100;

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
            bytes_read_counter: header_size as u64,
        })
    }

    pub fn reset(&mut self, source: &mut Read) -> Result<(), String> {
        let (frame, header_size) = frame::read_frame_header(source)?;
        let window_size = frame.header.window_size()?;
        frame.check_valid()?;

        if window_size > MAX_WINDOW_SIZE {
            return Err(format!(
                "Dont support window_sizes (requested: {}) over: {}",
                window_size, MAX_WINDOW_SIZE
            ));
        }

        self.frame = frame;
        self.frame_finished = false;
        self.block_counter = 0;
        self.decoder_scratch.reset(window_size as usize);
        self.bytes_read_counter = header_size as u64;
        Ok(())
    }
}

impl FrameDecoder {
    pub fn new() -> FrameDecoder {
        FrameDecoder { state: None }
    }

    pub fn init(&mut self, source: &mut Read) -> Result<(), String> {
        self.reset(source)
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

    /// Returns how many bytes the frame contains after decompression
    pub fn content_size(&self) -> Option<u64> {
        let state = match &self.state {
            None => return Some(0),
            Some(s) => s,
        };

        match state.frame.header.frame_content_size() {
            Err(_) => None,
            Ok(x) => Some(x),
        }
    }

    /// Counter for how many bytes have been consumed while deocidng the frame
    pub fn bytes_read_from_source(&self) -> u64 {
        let state = match &self.state {
            None => return 0,
            Some(s) => s,
        };
        state.bytes_read_counter
    }

    /// Whether the current frames last block has been decoded yet
    /// If this returns true you can call the drain* functions to get all content
    /// (the read() function will drain automatically if this returns true)
    pub fn is_finished(&self) -> bool {
        let state = match &self.state {
            None => return true,
            Some(s) => s,
        };
        state.frame_finished
    }

    /// Counter for how many blocks have already been decoded
    pub fn blocks_decoded(&self) -> usize {
        let state = match &self.state {
            None => return 0,
            Some(s) => s,
        };
        state.block_counter
    }

    /// Decodes blocks from a reader. It requires that the framedecoder has been initialized first.
    /// The Strategy influences how many blocks will be decoded before the function returns
    /// This is important if you want to manage memory consumption carefully. If you dont care
    /// about that you can just choose the strategy "All" and have all blocks of the frame decoded into the buffer
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
            state.bytes_read_counter += block_header_size as u64;

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
            state.bytes_read_counter += bytes_read_in_block_body;

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
                            state.bytes_read_counter += 4;
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

    /// Collect is for collecting bytes and retain window_size bytes while decoding is still going on
    /// After decoding of the frame (is_finished() == true) has finished it will collect all remaining bytes
    pub fn collect(&mut self) -> Option<Vec<u8>> {
        let finished = self.is_finished();
        let state = match &mut self.state {
            None => return None,
            Some(s) => s,
        };
        if finished {
            Some(state.decoder_scratch.buffer.drain())
        } else {
            state.decoder_scratch.buffer.drain_to_window_size()
        }
    }

    /// Collect is for collecting bytes and retain window_size bytes while decoding is still going on
    /// After decoding of the frame (is_finished() == true) has finished it will collect all remaining bytes
    pub fn collect_to_writer(&mut self, w: &mut std::io::Write) -> Result<usize, std::io::Error> {
        let finished = self.is_finished();
        let state = match &mut self.state {
            None => return Ok(0),
            Some(s) => s,
        };
        if finished {
            state.decoder_scratch.buffer.drain_to_writer(w)
        } else {
            state.decoder_scratch.buffer.drain_to_window_size_writer(w)
        }
    }

    /// How many bytes can currently be collected from the decodebuffer, while decoding is going on this will be lower than the ectual decodbuffer size
    /// because window_size bytes need to be retained for decoding.
    /// After decoding of the frame (is_finished() == true) has finished it will report all remaining bytes
    pub fn can_collect(&self) -> usize {
        let finished = self.is_finished();
        let state = match &self.state {
            None => return 0,
            Some(s) => s,
        };
        if finished {
            state.decoder_scratch.buffer.can_drain()
        } else {
            match state.decoder_scratch.buffer.can_drain_to_window_size() {
                Some(x) => x,
                None => 0,
            }
        }
    }

    /// Decodes as many blocks as possible from the source slice and reads from the decodebuffer into the target slice
    /// The source slice may contain only parts of a frame but must contain at least one full block to make progress
    /// Returns (read, written), if read == 0 then the source did not contain a full block and further calls with the same
    /// input will not make any progress!
    ///
    /// Note that no kind of block can be bigger than 128kb.
    /// So to be safe use at least 128*1024 (max block content size) + 3 (block_header size) + 18 (max frame_header size) bytes as your source buffer
    ///
    /// You may call this function with an empty source after all bytes have been decoded. This is equivalent to just call decoder.read(&mut traget)
    pub fn decode_from_to(
        &mut self,
        source: &[u8],
        target: &mut [u8],
    ) -> Result<(usize, usize), crate::errors::FrameDecoderError> {
        let bytes_read_at_start = match &mut self.state {
            Some(s) => s.bytes_read_counter,
            None => 0,
        };

        if !self.is_finished() || self.state.is_none() {
            let mut mt_source = &source[..];

            if self.state.is_none() {
                match self.init(&mut mt_source) {
                    Ok(()) => {}
                    Err(m) => return Err(crate::errors::FrameDecoderError::FailedToInitialize(m)),
                }
            }

            //pseudo block to scope "state" so we can borrow self again after the block
            {
                let mut state = match &mut self.state {
                    Some(s) => s,
                    None => panic!("Bug in library"),
                };
                let mut block_dec = decoding::block_decoder::new();

                loop {
                    //check if there are enough bytes for the next header
                    if mt_source.len() < 3 {
                        break;
                    }
                    let (block_header, block_header_size) =
                        match block_dec.read_block_header(&mut mt_source) {
                            Ok(h) => h,
                            Err(m) => {
                                return Err(
                                    crate::errors::FrameDecoderError::FailedToReadBlockHeader(m),
                                )
                            }
                        };

                    // check the needed size for the block before updating counters.
                    // If not enough bytes are in the source, the header will have to be read again, so act like we never read it in the first place
                    if mt_source.len() < block_header.content_size as usize {
                        break;
                    }
                    state.bytes_read_counter += block_header_size as u64;

                    let bytes_read_in_block_body = match block_dec.decode_block_content(
                        &block_header,
                        &mut state.decoder_scratch,
                        &mut mt_source,
                    ) {
                        Ok(h) => h,
                        Err(m) => {
                            return Err(crate::errors::FrameDecoderError::FailedToReadBlockBody(m))
                        }
                    };
                    state.bytes_read_counter += bytes_read_in_block_body;
                    state.block_counter += 1;

                    if block_header.last_block {
                        state.frame_finished = true;
                        if state.frame.header.descriptor.content_checksum_flag() {
                            let chksum = &mt_source[..3];
                            state.bytes_read_counter += 4;
                            let _ = chksum;
                            //TODO checksum
                        }
                        break;
                    }
                }
            }
        }

        let result_len = match self.read(target) {
            Ok(x) => x,
            Err(_) => return Err(crate::errors::FrameDecoderError::FailedToDrainDecodebuffer),
        };
        let bytes_read_at_end = match &mut self.state {
            Some(s) => s.bytes_read_counter,
            None => panic!("Bug in library"),
        };
        let read_len = bytes_read_at_end - bytes_read_at_start;
        Ok((read_len as usize, result_len))
    }
}

/// Read bytes from the decode_buffer that are no longer needed. While the frame is not yet finished
/// this will retain window_size bytes, else it will drain it completely
impl std::io::Read for FrameDecoder {
    fn read(&mut self, target: &mut [u8]) -> std::result::Result<usize, std::io::Error> {
        let state = match &mut self.state {
            None => return Ok(0),
            Some(s) => s,
        };
        if state.frame_finished {
            state.decoder_scratch.buffer.read_all(target)
        } else {
            state.decoder_scratch.buffer.read(target)
        }
    }
}
