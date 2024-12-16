//! A pure Rust implementation of the [Zstandard compression format](https://www.rfc-editor.org/rfc/rfc8878.pdf).
//!
//! ## Decompression
//! The [decoding] module contains the code for decompression.
//! Decompression can be achieved by using the [`decoding::streaming_decoder::StreamingDecoder`]
//! or the more low-level [`decoding::frame_decoder::FrameDecoder`]
//!
//! ## Compression
//! The [encoding] module contains the code for compression.
//! Decompression can be achieved by using the [`encoding::compress`]/[`encoding::compress_to_vec`]
//! functions or the [`encoding::frame_compressor::FrameCompressor`]
//!
#![doc = include_str!("../Readme.md")]
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
