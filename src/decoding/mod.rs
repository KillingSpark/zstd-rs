//! Structures and utilities used for reading from data, decoding that data
//! and storing the output.

pub mod bit_reader;
pub mod bit_reader_reverse;
pub mod block_decoder;
pub mod decodebuffer;
pub mod dictionary;
pub mod frame;
pub mod frame_decoder;
pub mod literals_section_decoder;
mod ringbuffer;
#[allow(dead_code)]
pub mod scratch;
pub mod sequence_execution;
pub mod sequence_section_decoder;
pub mod streaming_decoder;

#[derive(Debug)]
#[non_exhaustive]
pub enum GetBitsError {
    TooManyBits {
        num_requested_bits: usize,
        limit: u8,
    },
    NotEnoughRemainingBits {
        requested: usize,
        remaining: usize,
    },
}
