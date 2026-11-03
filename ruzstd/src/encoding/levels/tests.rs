use alloc::vec::Vec;

use crate::decoding::FrameDecoder;
use crate::encoding::{compress_to_vec, CompressionLevel};

const BLOCK: usize = 128 * 1024;

const COMPRESSING_LEVELS: [CompressionLevel; 4] = [
    CompressionLevel::Fastest,
    CompressionLevel::Default,
    CompressionLevel::Better,
    CompressionLevel::Best,
];

const LAZY_LEVELS: [CompressionLevel; 3] = [
    CompressionLevel::Default,
    CompressionLevel::Better,
    CompressionLevel::Best,
];

#[test]
fn fastest_does_not_expand_incompressible_blocks_past_raw_size() {
    assert_does_not_exceed_raw(CompressionLevel::Fastest, 8 * 1024);
}

#[test]
fn fastest_does_not_expand_incompressible_max_size_blocks() {
    assert_does_not_exceed_raw(CompressionLevel::Fastest, BLOCK);
}

#[test]
fn default_does_not_expand_incompressible_blocks_past_raw_size() {
    for level in LAZY_LEVELS {
        assert_does_not_exceed_raw(level, 8 * 1024);
        assert_does_not_exceed_raw(level, BLOCK);
        assert_does_not_exceed_raw(level, 3 * BLOCK + 17);
    }
}

fn assert_does_not_exceed_raw(level: CompressionLevel, len: usize) {
    let data = xorshift(len, 0x1234_5678_9ABC_DEF0);
    let raw = compress_to_vec(data.as_slice(), CompressionLevel::Uncompressed);
    let compressed = compress_to_vec(data.as_slice(), level);

    assert!(
        compressed.len() <= raw.len(),
        "{level:?} output should not exceed raw frame size for {len} bytes: {} > {}",
        compressed.len(),
        raw.len()
    );
}

fn xorshift(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    let mut data = Vec::with_capacity(len);
    while data.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        data.extend_from_slice(&state.to_le_bytes());
    }
    data.truncate(len);
    data
}

/// Text-like data: words drawn from a small vocabulary, so there are many short and medium matches
fn text_like(len: usize, seed: u64) -> Vec<u8> {
    const WORDS: &[&str] = &[
        "the",
        "quick",
        "brown",
        "fox",
        "jumps",
        "over",
        "lazy",
        "dog",
        "zstandard",
        "compression",
        "sequence",
        "literal",
        "offset",
        "match",
        "length",
        "window",
        "block",
        "frame",
        "header",
        "entropy",
        "huffman",
        "finite",
        "state",
        "ruzstd",
        "decoder",
        "encoder",
        "rust",
        "memory",
        "safety",
        "performance",
        "\n",
    ];
    let mut state = seed;
    let mut data = Vec::with_capacity(len + 16);
    while data.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let word = WORDS[(state % WORDS.len() as u64) as usize];
        data.extend_from_slice(word.as_bytes());
        data.push(b' ');
    }
    data.truncate(len);
    data
}

fn decode_ruzstd(compressed: &[u8], expected_len: usize) -> Vec<u8> {
    let mut decoder = FrameDecoder::new();
    // decode_all_to_vec decodes into the spare capacity of the vector
    let mut decoded = Vec::with_capacity(expected_len);
    decoder
        .decode_all_to_vec(compressed, &mut decoded)
        .expect("ruzstd failed to decode its own output");
    decoded
}

fn decode_zstd(compressed: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::new();
    zstd::stream::copy_decode(compressed, &mut decoded)
        .expect("reference zstd failed to decode ruzstd output");
    decoded
}

/// Compress at every level, check both decoders reproduce the input and return the sizes per level
fn roundtrip_all_levels(data: &[u8]) -> Vec<(CompressionLevel, usize)> {
    let mut sizes = Vec::new();
    for level in COMPRESSING_LEVELS {
        let compressed = compress_to_vec(data, level);
        assert_eq!(
            decode_ruzstd(&compressed, data.len()),
            data,
            "ruzstd roundtrip failed at {level:?} for {} bytes",
            data.len()
        );
        assert_eq!(
            decode_zstd(&compressed),
            data,
            "zstd roundtrip failed at {level:?} for {} bytes",
            data.len()
        );
        sizes.push((level, compressed.len()));
    }
    sizes
}

fn size_at(sizes: &[(CompressionLevel, usize)], wanted: CompressionLevel) -> usize {
    sizes
        .iter()
        .find(|(level, _)| core::mem::discriminant(level) == core::mem::discriminant(&wanted))
        .unwrap()
        .1
}

#[test]
fn roundtrip_empty_input() {
    roundtrip_all_levels(&[]);
}

#[test]
fn roundtrip_tiny_inputs() {
    roundtrip_all_levels(&[42]);
    roundtrip_all_levels(b"ab");
    roundtrip_all_levels(b"abcd");
    roundtrip_all_levels(b"abcde");
    roundtrip_all_levels(b"abcabcabcabc");
    roundtrip_all_levels(b"hello hello hello hello hello world");
}

#[test]
fn roundtrip_all_same_bytes() {
    let sizes = roundtrip_all_levels(&alloc::vec![7u8; 1000]);
    // Encoded as a single RLE block
    assert!(size_at(&sizes, CompressionLevel::Default) < 32);
    let sizes = roundtrip_all_levels(&alloc::vec![0u8; 3 * BLOCK + 5]);
    assert!(size_at(&sizes, CompressionLevel::Default) < 64);
}

#[test]
fn roundtrip_incompressible() {
    let data = xorshift(2 * BLOCK + 1234, 99);
    let sizes = roundtrip_all_levels(&data);
    let raw = compress_to_vec(data.as_slice(), CompressionLevel::Uncompressed).len();
    for (level, size) in sizes {
        assert!(size <= raw, "{:?} expanded incompressible data", level);
    }
}

#[test]
fn roundtrip_text_like_multiple_blocks() {
    let data = text_like(5 * BLOCK + 321, 5);
    let sizes = roundtrip_all_levels(&data);
    let raw = compress_to_vec(data.as_slice(), CompressionLevel::Uncompressed).len();
    let fastest = size_at(&sizes, CompressionLevel::Fastest);
    let default = size_at(&sizes, CompressionLevel::Default);
    let better = size_at(&sizes, CompressionLevel::Better);
    let best = size_at(&sizes, CompressionLevel::Best);
    assert!(default < raw);
    assert!(
        default <= fastest,
        "Default ({}) > Fastest ({})",
        default,
        fastest
    );
    assert!(
        better <= default,
        "Better ({}) > Default ({})",
        better,
        default
    );
    assert!(best <= better, "Best ({}) > Better ({})", best, better);
}

#[test]
fn roundtrip_cross_block_matches() {
    // A random chunk that is repeated with a distance larger than one block.
    // Fastest can not find these matches, Default can.
    let chunk = xorshift(100 * 1024, 11);
    let filler = xorshift(BLOCK + 40 * 1024, 12);
    let mut data = Vec::new();
    data.extend_from_slice(&chunk);
    data.extend_from_slice(&filler);
    data.extend_from_slice(&chunk);
    data.extend_from_slice(&filler[..7000]);
    data.extend_from_slice(&chunk);

    let sizes = roundtrip_all_levels(&data);
    let raw = compress_to_vec(data.as_slice(), CompressionLevel::Uncompressed).len();
    let fastest = size_at(&sizes, CompressionLevel::Fastest);
    let default = size_at(&sizes, CompressionLevel::Default);
    assert!(default <= fastest);
    // The two repetitions of the 100 kB chunk should (almost) vanish
    assert!(
        default < raw - 190 * 1024,
        "Default did not find the cross-block matches: {} vs raw {}",
        default,
        raw
    );
}

#[test]
fn roundtrip_matches_at_window_edge() {
    // Repetition distance just below and just above the 2 MiB window of the Default level.
    // Both must roundtrip (the reference decoder rejects offsets larger than the window);
    // only the former needs to be found. The filler is all zeros so it does not evict the
    // hash table entries of the first chunk.
    let chunk = xorshift(64 * 1024, 21);
    let window = 2 * 1024 * 1024;
    for distance in [window - chunk.len() - 100, window, window + 100] {
        let mut data = Vec::new();
        data.extend_from_slice(&chunk);
        data.extend_from_slice(&alloc::vec![0u8; distance - chunk.len()]);
        data.extend_from_slice(&chunk);
        let sizes = roundtrip_all_levels(&data);
        let raw = compress_to_vec(data.as_slice(), CompressionLevel::Uncompressed).len();
        let default = size_at(&sizes, CompressionLevel::Default);
        if distance <= window {
            assert!(
                default < raw - 60 * 1024,
                "match at distance {} not found",
                distance
            );
        }
    }
}

#[test]
fn roundtrip_mixed_content_long_input() {
    // Several MiB mixing text, zero runs and random data, so the lazy matcher has to rebase its window
    let mut data = Vec::new();
    for i in 0..12u64 {
        data.extend_from_slice(&text_like(300 * 1024, i));
        data.extend_from_slice(&alloc::vec![i as u8; 50 * 1024]);
        data.extend_from_slice(&xorshift(20 * 1024 + i as usize, 100 + i));
        data.extend_from_slice(&text_like(100 * 1024, 3));
    }
    let sizes = roundtrip_all_levels(&data);
    let fastest = size_at(&sizes, CompressionLevel::Fastest);
    let default = size_at(&sizes, CompressionLevel::Default);
    assert!(default <= fastest);
}

#[test]
fn reusing_a_compressor_across_levels() {
    use crate::encoding::FrameCompressor;
    let data = text_like(300 * 1024, 77);
    let mut compressor: FrameCompressor<&[u8], Vec<u8>, _> =
        FrameCompressor::new(CompressionLevel::Fastest);
    for level in [
        CompressionLevel::Fastest,
        CompressionLevel::Default,
        CompressionLevel::Fastest,
        CompressionLevel::Best,
        CompressionLevel::Default,
        CompressionLevel::Uncompressed,
        CompressionLevel::Default,
    ] {
        compressor.set_compression_level(level);
        compressor.set_source(data.as_slice());
        compressor.set_drain(Vec::new());
        compressor.compress();
        let output = compressor.take_drain().unwrap();
        assert_eq!(
            decode_ruzstd(&output, data.len()),
            data,
            "reuse failed at {:?}",
            level
        );
        assert_eq!(decode_zstd(&output), data, "reuse failed at {:?}", level);
    }
}
