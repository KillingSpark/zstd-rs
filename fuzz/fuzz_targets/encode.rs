#![no_main]
#[macro_use]
extern crate libfuzzer_sys;
extern crate ruzstd;
use ruzstd::encoding::{CompressionLevel, FrameCompressor};

fuzz_target!(|data: &[u8]| {
    let mut output = Vec::new();
    let mut compressor = FrameCompressor::new(data, &mut output, CompressionLevel::Uncompressed);
    compressor.compress();

    let mut decoded = Vec::with_capacity(data.len());
    let mut decoder = ruzstd::decoding::FrameDecoder::new();
    decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
    assert_eq!(data, &decoded);

    let mut output = Vec::new();
    let mut compressor = FrameCompressor::new(data, &mut output, CompressionLevel::Fastest);
    compressor.compress();

    let mut decoded = Vec::with_capacity(data.len());
    let mut decoder = ruzstd::decoding::FrameDecoder::new();
    decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
    assert_eq!(data, &decoded);
});
