#![no_std]
#![deny(trivial_casts, trivial_numeric_casts, rust_2018_idioms)]
#![cfg_attr(not(feature = "std"), feature(error_in_core))]

#[cfg(feature = "std")]
extern crate std;

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
