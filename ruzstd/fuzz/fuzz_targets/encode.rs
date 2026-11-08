#![no_main]
#[macro_use]
extern crate libfuzzer_sys;
extern crate ruzstd;
use ruzstd::encoding::{CompressionLevel, compress_to_vec};

fuzz_target!(|data: &[u8]| {
    for level in [
        CompressionLevel::Uncompressed,
        CompressionLevel::Fastest,
        CompressionLevel::Default,
        CompressionLevel::Better,
    ] {
        let output = compress_to_vec(data, level);

        let mut decoded = Vec::with_capacity(data.len());
        let mut decoder = ruzstd::decoding::FrameDecoder::new();
        decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
        assert_eq!(data, &decoded);
    }
});
