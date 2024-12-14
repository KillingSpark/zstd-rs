//! A pure Rust implementation of the [Zstandard compression format](https://facebook.github.io/zstd/).
//!
//! # Getting Started
//! ## Decompression
//! The [decoding] module contains the internals for decompression.
//! Decompression can be achieved by using the [`decoding::streaming_decoder::StreamingDecoder`] interface
//! or the more low-level [`decoding::frame_decoder::FrameDecoder`]
//!
//! ## Compression
//! The [encoding] module contains the internals for compression.
//! Decompression can be achieved by using the [`encoding::compress`]/[`encoding::compress_to_vec`] functions or the [`encoding::frame_compressor::FrameCompressor`] interface.
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
pub(crate) const VERBOSE: bool = false;

macro_rules! vprintln {
    ($($x:expr),*) => {
        #[cfg(feature = "std")]
        if crate::VERBOSE {
            std::println!($($x),*);
        }
    }
}

pub mod decoding;
pub mod encoding;

pub(crate) mod blocks;

#[cfg(feature = "fuzz_exports")]
pub mod fse;
#[cfg(feature = "fuzz_exports")]
pub mod huff0;

#[cfg(not(feature = "fuzz_exports"))]
pub(crate) mod fse;
#[cfg(not(feature = "fuzz_exports"))]
pub(crate) mod huff0;

mod tests;

#[cfg(feature = "std")]
pub mod io;

#[cfg(not(feature = "std"))]
pub mod io_nostd;

#[cfg(not(feature = "std"))]
pub use io_nostd as io;
