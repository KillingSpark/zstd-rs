#![deny(trivial_casts, trivial_numeric_casts, rust_2018_idioms)]

pub mod blocks;
pub mod decoding;
pub mod frame;
pub mod frame_decoder;
pub mod fse;
pub mod huff0;
pub mod streaming_decoder;
mod tests;

pub const VERBOSE: bool = false;
pub use frame_decoder::BlockDecodingStrategy;
pub use frame_decoder::FrameDecoder;
pub use streaming_decoder::StreamingDecoder;
