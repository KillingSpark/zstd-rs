#![no_main]
#[macro_use] extern crate libfuzzer_sys;
extern crate ruzstd;
use ruzstd::encoding::{FrameCompressor, CompressionLevel};

fuzz_target!(|data: &[u8]| {
    let mut content = data;
    let mut compressor = FrameCompressor::new(data, CompressionLevel::Uncompressed);
    let mut output = Vec::new();
    compressor.compress(&mut output);
});