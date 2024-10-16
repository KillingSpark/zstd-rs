#![no_main]
#[macro_use] extern crate libfuzzer_sys;
extern crate ruzstd;
use ruzstd::encoding::{FrameCompressor, CompressionLevel};

fuzz_target!(|data: &[u8]| {
    let compressor = FrameCompressor::new(data, CompressionLevel::Uncompressed);
    let mut output = Vec::new();
    compressor.compress(&mut output);

    let compressor = FrameCompressor::new(data, CompressionLevel::Fastest);
    let mut output = Vec::new();
    compressor.compress(&mut output);

    let mut decoded = Vec::with_capacity(data.len());
    let mut decoder = ruzstd::FrameDecoder::new();
    decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
    assert_eq!(data, &decoded);
});