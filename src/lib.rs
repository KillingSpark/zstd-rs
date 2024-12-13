//! A pure Rust implementation of the [Zstandard compression algorithm](https://facebook.github.io/zstd/).
//!
//! # Getting Started
//! ## Decompression
//! The [decoding] module contains the internals for decompression.
//! Decompression can be achieved by using the [`StreamingDecoder`] interface.
//!
//! ## Compression
//! The [encoding] module contains the internals for compression.
//! Decompression can be achieved by using the [`encoding::compress`]/[`encoding::compress_to_vec`] functions or the [`FrameCompressor`] interface.
//!
//! # Speed
//! The decoder has been measured to be roughly between 3.5 to 1.4 times slower
//! than the original implementation depending on the compressed data.
#![no_std]
#![deny(trivial_casts, trivial_numeric_casts, rust_2018_idioms)]

#[cfg(feature = "std")]
extern crate std;

#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

#[cfg(feature = "std")]
pub const VERBOSE: bool = false;

macro_rules! vprintln {
    ($($x:expr),*) => {
        #[cfg(feature = "std")]
        if crate::VERBOSE {
            std::println!($($x),*);
        }
    }
}

pub mod blocks;
pub mod decoding;
pub mod encoding;
pub mod frame;
pub mod frame_decoder;
pub mod fse;
pub mod huff0;
pub mod streaming_decoder;
mod tests;

#[cfg(feature = "std")]
pub mod io;

#[cfg(not(feature = "std"))]
pub mod io_nostd;

#[cfg(not(feature = "std"))]
pub use io_nostd as io;

pub use frame_decoder::BlockDecodingStrategy;
pub use frame_decoder::FrameDecoder;
pub use streaming_decoder::StreamingDecoder;

pub use encoding::FrameCompressor;
