//! Modules used for compressing/encoding data into the Zstd format.
// TODO: put behind a feature gate
pub mod bit_writer;
pub mod frame_encoder;
pub mod frame_header;
