#![no_main]
#[macro_use]
extern crate libfuzzer_sys;
extern crate ruzstd;
use ruzstd::encoding::{compress_to_vec, CompressionLevel};

fuzz_target!(|data: &[u8]| {
    let output = compress_to_vec(data, CompressionLevel::Uncompressed);

    let mut decoded = Vec::with_capacity(data.len());
    let mut decoder = ruzstd::decoding::FrameDecoder::new();
    decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
    assert_eq!(data, &decoded);

    for level in [
        CompressionLevel::Fastest,
        CompressionLevel::Default,
        CompressionLevel::Better,
        CompressionLevel::Best,
    ] {
        let output = compress_to_vec(data, level);

        let mut decoded = Vec::with_capacity(data.len());
        let mut decoder = ruzstd::decoding::FrameDecoder::new();
        decoder.decode_all_to_vec(&output, &mut decoded).unwrap();
        assert_eq!(data, &decoded);

        // The first half of the input doubles as a raw content dictionary for the second half
        let (dict, rest) = data.split_at(data.len() / 2);
        let dictionary = ruzstd::encoding::Dictionary::from_raw_content(dict.to_vec());
        let output = ruzstd::encoding::compress_to_vec_with_dictionary(rest, level, &dictionary);
        let mut decoder = ruzstd::decoding::FrameDecoder::new();
        decoder
            .add_dict(ruzstd::decoding::Dictionary::from_raw_content(1, dict.to_vec()))
            .unwrap();
        let mut input = output.as_slice();
        decoder.reset(&mut input).unwrap();
        decoder.force_dict(1).unwrap();
        decoder
            .decode_blocks(&mut input, ruzstd::decoding::BlockDecodingStrategy::All)
            .unwrap();
        let mut decoded = Vec::new();
        decoder.collect_to_writer(&mut decoded).unwrap();
        assert_eq!(rest, decoded.as_slice());
    }
});
