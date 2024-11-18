//! Modules used for compressing/encoding data into the Zstd format.
// TODO: put behind a feature gate
pub(crate) mod bit_writer;
pub(crate) mod block_header;
pub(crate) mod blocks;
mod frame_encoder;
pub use frame_encoder::*;
pub(crate) mod frame_header;
pub(crate) mod match_generator;
pub(crate) mod util;
