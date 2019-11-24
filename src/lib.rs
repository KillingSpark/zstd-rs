pub mod block;
pub mod decoding;
pub mod errors;
pub mod frame;
pub mod frame_decoder;
pub mod streaming_decoder;
pub mod fse;
pub mod huff0;
mod tests;

pub const VERBOSE: bool = false;
pub use frame_decoder::BlockDecodingStrategy;
pub use frame_decoder::FrameDecoder;
pub use streaming_decoder::StreamingDecoder;
