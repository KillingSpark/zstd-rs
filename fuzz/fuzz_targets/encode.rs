#![no_main]
#[macro_use]
extern crate libfuzzer_sys;
extern crate ruzstd;
use ruzstd::encoding::{CompressionLevel, compress_to_vec};

fuzz_target!(|data: &[u8]| {
    let output = compress_to_vec(data, CompressionLevel::Uncompressed);

    let mut decoded = Vec::with_capacity(data.len());
    let mut decoder = ruzstd::decoding::FrameDecoder::new();
    decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
    assert_eq!(data, &decoded);

    let output = compress_to_vec(data, CompressionLevel::Fastest);

    let mut decoded = Vec::with_capacity(data.len());
    let mut decoder = ruzstd::decoding::FrameDecoder::new();
    decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
    assert_eq!(data, &decoded);
});
