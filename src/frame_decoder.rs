use super::frame;
use crate::decoding::dictionary::Dictionary;
use crate::decoding::scratch::DecoderScratch;
use crate::decoding::{self, dictionary};
use crate::io::{Error, Read, Write};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::convert::TryInto;
use core::hash::Hasher;

/// This implements a decoder for zstd frames. This decoder is able to decode frames only partially and gives control
/// over how many bytes/blocks will be decoded at a time (so you don't have to decode a 10GB file into memory all at once).
/// It reads bytes as needed from a provided source and can be read from to collect partial results.
///
/// If you want to just read the whole frame with an io::Read without having to deal with manually calling decode_blocks
/// you can use the provided StreamingDecoder with wraps this FrameDecoder
///
/// Workflow is as follows:
/// ```
/// use ruzstd::frame_decoder::BlockDecodingStrategy;
///
/// # #[cfg(feature = "std")]
/// use std::io::{Read, Write};
///
/// // no_std environments can use the crate's own Read traits
/// # #[cfg(not(feature = "std"))]
/// use ruzstd::io::{Read, Write};
///
/// fn decode_this(mut file: impl Read) {
///     //Create a new decoder
///     let mut frame_dec = ruzstd::FrameDecoder::new();
///     let mut result = Vec::new();
///
///     // Use reset or init to make the decoder ready to decode the frame from the io::Read
///     frame_dec.reset(&mut file).unwrap();
///
///     // Loop until the frame has been decoded completely
///     while !frame_dec.is_finished() {
///         // decode (roughly) batch_size many bytes
///         frame_dec.decode_blocks(&mut file, BlockDecodingStrategy::UptoBytes(1024)).unwrap();
///
///         // read from the decoder to collect bytes from the internal buffer
///         let bytes_read = frame_dec.read(result.as_mut_slice()).unwrap();
///
///         // then do something with it
///         do_something(&result[0..bytes_read]);
///     }
///
///     // handle the last chunk of data
///     while frame_dec.can_collect() > 0 {
///         let x = frame_dec.read(result.as_mut_slice()).unwrap();
///
///         do_something(&result[0..x]);
///     }
/// }
///
/// fn do_something(data: &[u8]) {
/// # #[cfg(feature = "std")]
///     std::io::stdout().write_all(data).unwrap();
/// }
/// ```
pub struct FrameDecoder {
    state: Option<FrameDecoderState>,
    dicts: BTreeMap<u32, Dictionary>,
}

struct FrameDecoderState {
    pub frame: frame::Frame,
    decoder_scratch: DecoderScratch,
    frame_finished: bool,
    block_counter: usize,
    bytes_read_counter: u64,
    check_sum: Option<u32>,
    using_dict: Option<u32>,
}

pub enum BlockDecodingStrategy {
    All,
    UptoBlocks(usize),
    UptoBytes(usize),
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FrameDecoderError {
    #[error(transparent)]
    ReadFrameHeaderError(#[from] frame::ReadFrameHeaderError),
    #[error(transparent)]
    FrameHeaderError(#[from] frame::FrameHeaderError),
    #[error("Specified window_size is too big; Requested: {requested}, Max: {MAX_WINDOW_SIZE}")]
    WindowSizeTooBig { requested: u64 },
    #[error(transparent)]
    DictionaryDecodeError(#[from] dictionary::DictionaryDecodeError),
    #[error("Failed to parse/decode block body: {0}")]
    FailedToReadBlockHeader(#[from] decoding::block_decoder::BlockHeaderReadError),
    #[error("Failed to parse block header: {0}")]
    FailedToReadBlockBody(decoding::block_decoder::DecodeBlockContentError),
    #[error("Failed to read checksum: {0}")]
    FailedToReadChecksum(#[source] Error),
    #[error("Decoder must initialized or reset before using it")]
    NotYetInitialized,
    #[error("Decoder encountered error while initializing: {0}")]
    FailedToInitialize(frame::FrameHeaderError),
    #[error("Decoder encountered error while draining the decodebuffer: {0}")]
    FailedToDrainDecodebuffer(#[source] Error),
    #[error("Target must have at least as many bytes as the contentsize of the frame reports")]
    TargetTooSmall,
    #[error("Frame header specified dictionary id 0x{dict_id:X} that wasnt provided by add_dict() or reset_with_dict()")]
    DictNotProvided { dict_id: u32 },
}

const MAX_WINDOW_SIZE: u64 = 1024 * 1024 * 100;

impl FrameDecoderState {
    pub fn new(source: impl Read) -> Result<FrameDecoderState, FrameDecoderError> {
        let (frame, header_size) = frame::read_frame_header(source)?;
        let window_size = frame.header.window_size()?;
        Ok(FrameDecoderState {
            frame,
            frame_finished: false,
            block_counter: 0,
            decoder_scratch: DecoderScratch::new(window_size as usize),
            bytes_read_counter: u64::from(header_size),
            check_sum: None,
            using_dict: None,
        })
    }

    pub fn reset(&mut self, source: impl Read) -> Result<(), FrameDecoderError> {
        let (frame, header_size) = frame::read_frame_header(source)?;
        let window_size = frame.header.window_size()?;

        if window_size > MAX_WINDOW_SIZE {
            return Err(FrameDecoderError::WindowSizeTooBig {
                requested: window_size,
            });
        }

        self.frame = frame;
        self.frame_finished = false;
        self.block_counter = 0;
        self.decoder_scratch.reset(window_size as usize);
        self.bytes_read_counter = u64::from(header_size);
        self.check_sum = None;
        self.using_dict = None;
        Ok(())
    }
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDecoder {
    /// This will create a new decoder without allocating anything yet.
    /// init()/reset() will allocate all needed buffers if it is the first time this decoder is used
    /// else they just reset these buffers with not further allocations
    pub fn new() -> FrameDecoder {
        FrameDecoder {
            state: None,
            dicts: BTreeMap::new(),
        }
    }

    /// init() will allocate all needed buffers if it is the first time this decoder is used
    /// else they just reset these buffers with not further allocations
    ///
    /// Note that all bytes currently in the decodebuffer from any previous frame will be lost. Collect them with collect()/collect_to_writer()
    ///
    /// equivalent to reset()
    pub fn init(&mut self, source: impl Read) -> Result<(), FrameDecoderError> {
        self.reset(source)
    }

    /// reset() will allocate all needed buffers if it is the first time this decoder is used
    /// else they just reset these buffers with not further allocations
    ///
    /// Note that all bytes currently in the decodebuffer from any previous frame will be lost. Collect them with collect()/collect_to_writer()
    ///
    /// equivalent to init()
    pub fn reset(&mut self, source: impl Read) -> Result<(), FrameDecoderError> {
        use FrameDecoderError as err;
        let state = match &mut self.state {
            Some(s) => {
                s.reset(source)?;
                s
            }
            None => {
                self.state = Some(FrameDecoderState::new(source)?);
                self.state.as_mut().unwrap()
            }
        };
        if let Some(dict_id) = state.frame.header.dictionary_id() {
            let dict = self
                .dicts
                .get(&dict_id)
                .ok_or(err::DictNotProvided { dict_id })?;
            state.decoder_scratch.init_from_dict(dict);
            state.using_dict = Some(dict_id);
        }
        Ok(())
    }

    /// Add a dict to the FrameDecoder that can be used when needed. The FrameDecoder uses the appropriate one dynamically
    pub fn add_dict(&mut self, dict: Dictionary) -> Result<(), FrameDecoderError> {
        self.dicts.insert(dict.id, dict);
        Ok(())
    }

    pub fn force_dict(&mut self, dict_id: u32) -> Result<(), FrameDecoderError> {
        use FrameDecoderError as err;
        let Some(state) = self.state.as_mut() else {
            return Err(err::NotYetInitialized);
        };

        let dict = self
            .dicts
            .get(&dict_id)
            .ok_or(err::DictNotProvided { dict_id })?;
        state.decoder_scratch.init_from_dict(dict);
        state.using_dict = Some(dict_id);

        Ok(())
    }

    /// Returns how many bytes the frame contains after decompression
    pub fn content_size(&self) -> u64 {
        match &self.state {
            None => 0,
            Some(s) => s.frame.header.frame_content_size(),
        }
    }

    /// Returns the checksum that was read from the data. Only available after all bytes have been read. It is the last 4 bytes of a zstd-frame
    pub fn get_checksum_from_data(&self) -> Option<u32> {
        let state = match &self.state {
            None => return None,
            Some(s) => s,
        };

        state.check_sum
    }

    /// Returns the checksum that was calculated while decoding.
    /// Only a sensible value after all decoded bytes have been collected/read from the FrameDecoder
    pub fn get_calculated_checksum(&self) -> Option<u32> {
        let state = match &self.state {
            None => return None,
            Some(s) => s,
        };
        let cksum_64bit = state.decoder_scratch.buffer.hash.finish();
        //truncate to lower 32bit because reasons...
        Some(cksum_64bit as u32)
    }

    /// Counter for how many bytes have been consumed while decoding the frame
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
        if state.frame.header.descriptor.content_checksum_flag() {
            state.frame_finished && state.check_sum.is_some()
        } else {
            state.frame_finished
        }
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
    /// This is important if you want to manage memory consumption carefully. If you don't care
    /// about that you can just choose the strategy "All" and have all blocks of the frame decoded into the buffer
    pub fn decode_blocks(
        &mut self,
        mut source: impl Read,
        strat: BlockDecodingStrategy,
    ) -> Result<bool, FrameDecoderError> {
        use FrameDecoderError as err;
        let state = self.state.as_mut().ok_or(err::NotYetInitialized)?;

        let mut block_dec = decoding::block_decoder::new();

        let buffer_size_before = state.decoder_scratch.buffer.len();
        let block_counter_before = state.block_counter;
        loop {
            vprintln!("################");
            vprintln!("Next Block: {}", state.block_counter);
            vprintln!("################");
            let (block_header, block_header_size) = block_dec
                .read_block_header(&mut source)
                .map_err(err::FailedToReadBlockHeader)?;
            state.bytes_read_counter += u64::from(block_header_size);

            vprintln!();
            vprintln!(
                "Found {} block with size: {}, which will be of size: {}",
                block_header.block_type,
                block_header.content_size,
                block_header.decompressed_size
            );

            let bytes_read_in_block_body = block_dec
                .decode_block_content(&block_header, &mut state.decoder_scratch, &mut source)
                .map_err(err::FailedToReadBlockBody)?;
            state.bytes_read_counter += bytes_read_in_block_body;

            state.block_counter += 1;

            vprintln!("Output: {}", state.decoder_scratch.buffer.len());

            if block_header.last_block {
                state.frame_finished = true;
                if state.frame.header.descriptor.content_checksum_flag() {
                    let mut chksum = [0u8; 4];
                    source
                        .read_exact(&mut chksum)
                        .map_err(err::FailedToReadChecksum)?;
                    state.bytes_read_counter += 4;
                    let chksum = u32::from_le_bytes(chksum);
                    state.check_sum = Some(chksum);
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

    /// Collect bytes and retain window_size bytes while decoding is still going on.
    /// After decoding of the frame (is_finished() == true) has finished it will collect all remaining bytes
    pub fn collect(&mut self) -> Option<Vec<u8>> {
        let finished = self.is_finished();
        let state = self.state.as_mut()?;
        if finished {
            Some(state.decoder_scratch.buffer.drain())
        } else {
            state.decoder_scratch.buffer.drain_to_window_size()
        }
    }

    /// Collect bytes and retain window_size bytes while decoding is still going on.
    /// After decoding of the frame (is_finished() == true) has finished it will collect all remaining bytes
    pub fn collect_to_writer(&mut self, w: impl Write) -> Result<usize, Error> {
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

    /// How many bytes can currently be collected from the decodebuffer, while decoding is going on this will be lower than the actual decodbuffer size
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
            state
                .decoder_scratch
                .buffer
                .can_drain_to_window_size()
                .unwrap_or(0)
        }
    }

    /// Decodes as many blocks as possible from the source slice and reads from the decodebuffer into the target slice
    /// The source slice may contain only parts of a frame but must contain at least one full block to make progress
    ///
    /// By all means use decode_blocks if you have a io.Reader available. This is just for compatibility with other decompressors
    /// which try to serve an old-style c api
    ///
    /// Returns (read, written), if read == 0 then the source did not contain a full block and further calls with the same
    /// input will not make any progress!
    ///
    /// Note that no kind of block can be bigger than 128kb.
    /// So to be safe use at least 128*1024 (max block content size) + 3 (block_header size) + 18 (max frame_header size) bytes as your source buffer
    ///
    /// You may call this function with an empty source after all bytes have been decoded. This is equivalent to just call decoder.read(&mut target)
    pub fn decode_from_to(
        &mut self,
        source: &[u8],
        target: &mut [u8],
    ) -> Result<(usize, usize), FrameDecoderError> {
        use FrameDecoderError as err;
        let bytes_read_at_start = match &self.state {
            Some(s) => s.bytes_read_counter,
            None => 0,
        };

        if !self.is_finished() || self.state.is_none() {
            let mut mt_source = source;

            if self.state.is_none() {
                self.init(&mut mt_source)?;
            }

            //pseudo block to scope "state" so we can borrow self again after the block
            {
                let mut state = match &mut self.state {
                    Some(s) => s,
                    None => panic!("Bug in library"),
                };
                let mut block_dec = decoding::block_decoder::new();

                if state.frame.header.descriptor.content_checksum_flag()
                    && state.frame_finished
                    && state.check_sum.is_none()
                {
                    //this block is needed if the checksum were the only 4 bytes that were not included in the last decode_from_to call for a frame
                    if mt_source.len() >= 4 {
                        let chksum = mt_source[..4].try_into().expect("optimized away");
                        state.bytes_read_counter += 4;
                        let chksum = u32::from_le_bytes(chksum);
                        state.check_sum = Some(chksum);
                    }
                    return Ok((4, 0));
                }

                loop {
                    //check if there are enough bytes for the next header
                    if mt_source.len() < 3 {
                        break;
                    }
                    let (block_header, block_header_size) = block_dec
                        .read_block_header(&mut mt_source)
                        .map_err(err::FailedToReadBlockHeader)?;

                    // check the needed size for the block before updating counters.
                    // If not enough bytes are in the source, the header will have to be read again, so act like we never read it in the first place
                    if mt_source.len() < block_header.content_size as usize {
                        break;
                    }
                    state.bytes_read_counter += u64::from(block_header_size);

                    let bytes_read_in_block_body = block_dec
                        .decode_block_content(
                            &block_header,
                            &mut state.decoder_scratch,
                            &mut mt_source,
                        )
                        .map_err(err::FailedToReadBlockBody)?;
                    state.bytes_read_counter += bytes_read_in_block_body;
                    state.block_counter += 1;

                    if block_header.last_block {
                        state.frame_finished = true;
                        if state.frame.header.descriptor.content_checksum_flag() {
                            //if there are enough bytes handle this here. Else the block at the start of this function will handle it at the next call
                            if mt_source.len() >= 4 {
                                let chksum = mt_source[..4].try_into().expect("optimized away");
                                state.bytes_read_counter += 4;
                                let chksum = u32::from_le_bytes(chksum);
                                state.check_sum = Some(chksum);
                            }
                        }
                        break;
                    }
                }
            }
        }

        let result_len = self.read(target).map_err(err::FailedToDrainDecodebuffer)?;
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
impl Read for FrameDecoder {
    fn read(&mut self, target: &mut [u8]) -> Result<usize, Error> {
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
