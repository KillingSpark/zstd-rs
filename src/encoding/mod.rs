//! Modules used for compressing/encoding data into the Zstd format.
// TODO: put behind a feature gate
pub(crate) mod bit_writer;
pub(crate) mod block_header;
pub(crate) mod blocks;
pub(crate) mod frame_header;
pub(crate) mod match_generator;
pub(crate) mod util;

pub mod frame_compressor;

use crate::io::{Read, Write};
use alloc::vec::Vec;
use frame_compressor::{CompressionLevel, FrameCompressor};
use match_generator::Sequence;

/// Convenience function to compress some source into a target without reusing any resources of the compressor
/// ```rust
/// use ruzstd::encoding::{compress, frame_compressor::CompressionLevel};
/// let data: &[u8] = &[0,0,0,0,0,0,0,0,0,0,0,0];
/// let mut target = Vec::new();
/// compress(data, &mut target, CompressionLevel::Fastest);
/// ```
pub fn compress<R: Read, W: Write>(source: R, target: W, level: CompressionLevel) {
    let mut frame_enc = FrameCompressor::new(source, target, level);
    frame_enc.compress();
}

/// Convenience function to compress some source into a target without reusing any resources of the compressor into a Vec
/// ```rust
/// use ruzstd::encoding::{compress_to_vec, frame_compressor::CompressionLevel};
/// let data: &[u8] = &[0,0,0,0,0,0,0,0,0,0,0,0];
/// let compressed = compress_to_vec(data, CompressionLevel::Fastest);
/// ```
pub fn compress_to_vec<R: Read>(source: R, level: CompressionLevel) -> Vec<u8> {
    let mut vec = Vec::new();
    compress(source, &mut vec, level);
    vec
}

pub(crate) trait Matcher {
    /// Get a space where we can put data to be matched on
    fn get_next_space(&mut self) -> alloc::vec::Vec<u8>;
    /// Get a reference to the last commited space
    fn get_last_space(&mut self) -> &[u8];
    /// Commit a space to the matcher so it can be matched against
    fn commit_space(&mut self, space: alloc::vec::Vec<u8>);
    /// Just process the data in the last commited space for future matching
    fn skip_matching(&mut self);
    /// Process the data in the last commited space for future matching AND generate matches for the data
    fn start_matching(&mut self, handle_sequence: impl for<'a> FnMut(Sequence<'a>));
}
