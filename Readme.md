# Ruzstd (a pure rust zstd format implementation)

[![Released API docs](https://docs.rs/ruzstd/badge.svg)](https://docs.rs/ruzstd)
[![CI](https://github.com/killingspark/zstd-rs/workflows/CI/badge.svg)](https://github.com/killingspark/zstd-rs/actions?query=workflow%3ACI)


# What is this

A pure Rust implementation of the Zstandard compression format, as defined in [RFC8878](https://www.rfc-editor.org/rfc/rfc8878.pdf).

This crate contains a fully operational implementation of the decompression portion of the standard.
It also provides a compressor which is usable, but it does not yet reach the speed, ratio or configurability of the original zstd library.

This crate is currently actively maintained.

# Current Status

## Decompression
The `decoding` module provides a complete
implementation of a Zstandard decompressor.

In terms of speed, `ruzstd` is behind the original C implementation
which has a rust binding located [here](https://github.com/gyscos/zstd-rs).

Measuring with the 'time' utility the original zstd and my decoder both
decoding the same enwik9.zst file from a ramfs, my decoder is about 3.5
times slower. Enwik9 is highly compressible, for less compressible data
(like a ubuntu installation .iso) my decoder comes close to only being
1.4 times slower.

## Compression
On the compression side:
- Support for generating compressed blocks at any compression level
  - [x] Uncompressed
  - [x] Fastest (roughly level 1): greedy single-slot hash matcher, window of one block (128 kB)
  - [x] Default (roughly level 3): hash chains with lazy matching, 2 MiB window
  - [x] Better (roughly level 7): hash chains with lazy matching, 4 MiB window
  - [x] Best (roughly level 11): hash chains with lazy matching, 8 MiB window
- [x] Checksums
- [x] Dictionaries (raw content and the zstd dictionary format, the entropy tables of the latter are not used yet)

The encoder does not yet make use of repeat offset codes and does not try to
optimize the choice of entropy tables, so the ratio of the higher levels stays
a bit behind the reference implementation and the speed is well behind it.

Measured on three inputs (the concatenated `decodecorpus_files`, the `*.rs` files of this
repository and 48 MB of `*.rs` files from a cargo registry). Sizes are relative to the
input, speeds are single threaded with the reference implementation measured through the
`zstd` crate on the same machine:

| Level          | decodecorpus (11.5 MB) | ruzstd sources (0.5 MB) | registry sources (48 MB) |
|----------------|------------------------|-------------------------|--------------------------|
| Fastest        | 51.7 %, 54 MB/s        | 34.7 %, 67 MB/s         | 26.0 %, 74 MB/s          |
| Default        | 42.6 %, 59 MB/s        | 21.4 %, 69 MB/s         | 13.6 %, 80 MB/s          |
| Better         | 41.9 %, 30 MB/s        | 20.3 %, 30 MB/s         | 12.2 %, 35 MB/s          |
| Best           | 41.6 %, 18 MB/s        | 20.2 %, 14 MB/s         | 12.0 %, 12 MB/s          |
| zstd -1        | 46.4 %, 478 MB/s       | 24.1 %, 309 MB/s        | 16.2 %, 447 MB/s         |
| zstd -3        | 43.4 %, 218 MB/s       | 22.3 %, 228 MB/s        | 14.5 %, 305 MB/s         |
| zstd -7        | 41.8 %, 93 MB/s        | 19.7 %, 73 MB/s         | 12.0 %, 97 MB/s          |
| zstd -11       | 41.6 %, 38 MB/s        | 19.2 %, 27 MB/s         | 11.1 %, 37 MB/s          |

### Dictionaries

Dictionaries help most for many small inputs that share structure. Measured on the 207 small
systemd unit files in `dict_tests/files` (136 kB in total, 650 bytes on average) with the 52 kB
dictionary in `dict_tests/dictionary`, reusing one `FrameCompressor` for all files:

| Level          | without dictionary | with dictionary |
|----------------|--------------------|-----------------|
| Fastest        | 82.1 %, 31 MB/s    | 26.8 %, 30 MB/s |
| Default        | 80.7 %, 30 MB/s    | 18.9 %, 25 MB/s |
| Better         | 80.7 %, 19 MB/s    | 18.2 %, 16 MB/s |
| Best           | 80.7 %, 23 MB/s    | 18.1 %, 19 MB/s |
| zstd -1        | 63.2 %, 30 MB/s    | 19.9 %, 243 MB/s|
| zstd -3        | 62.0 %, 17 MB/s    | 17.2 %, 227 MB/s|
| zstd -7        | 61.7 %, 2 MB/s     | 15.0 %, 62 MB/s |

The reference implementation shrinks its tables for small inputs when it knows the input size,
which is where most of its speed advantage on tiny inputs comes from. Our compressor reads from a
`Read` and does not know the size up front, but it only touches the parts of its tables a frame
actually used, so reusing a compressor keeps the per-frame cost small. The convenience functions
create a fresh compressor per call and are a lot slower for tiny inputs.

## Dictionary Generation
When the `dict_builder` feature is enabled, the `dictionary` module
provides the ability to create new dictionaries. 

On the `github-users` sample set, our implementation benchmarks within
0.2% of the official implementation (as of commit 
`09e52d07340acdb2e13817b066e8be6e424f7258`):
```no_build
uncompressed: 100.00% (7484607 bytes)
no dict: 34.99% of original size (2618872 bytes)
reference dict: 16.16% of no dict size (2195672 bytes smaller)
our dict: 16.28% of no dict size (2192400 bytes smaller)
```

The dictionary generator only provides support for creating "raw
content" dictionaries. Tagged dictionaries are currently unsupported.

See <https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#dictionary-format>
for clarification.


# How can you use it?

## Compression

The easiest is to use the provided `compress`/`compress_to_vec` functions

```rust, no_run
use ruzstd::encoding::{compress, compress_to_vec, CompressionLevel};
let data: &[u8] = todo!();
// Either
let mut compressed = Vec::new();
compress(data, &mut compressed, CompressionLevel::Fastest);
// or
let compressed = compress_to_vec(data, CompressionLevel::Fastest);
 ```

 Or you can use the `FrameCompressor` manually to compress data. This allows you to process encoded data while it is being encoded instead of collecting into a big vector.

### Compressing with a dictionary

A dictionary acts as data that precedes the input, so inputs that share structure with it compress much better.
Both dictionaries in the zstd dictionary format (as created by `zstd --train` or the `dictionary` module) and
raw content dictionaries (arbitrary bytes) are supported. The decoder needs the same dictionary.

```rust, no_run
use ruzstd::encoding::{FrameCompressor, CompressionLevel, Dictionary};
let raw_dictionary: &[u8] = todo!("read the dictionary file");
let dictionary = Dictionary::parse(raw_dictionary).unwrap();
// Reuse one compressor for many inputs, loading the dictionary is only done once
let mut compressor: FrameCompressor<&[u8], Vec<u8>, _> = FrameCompressor::new(CompressionLevel::Default);
compressor.set_dictionary(dictionary);
for input in [b"first input".as_slice(), b"second input".as_slice()] {
    compressor.set_source(input);
    compressor.set_drain(Vec::new());
    compressor.compress();
    let compressed = compressor.take_drain().unwrap();
    // store `compressed` somewhere
}
```

Frames compressed with a dictionary in the zstd format carry its ID, so `FrameDecoder::add_dict` is enough on
the decoding side. Raw content dictionaries have no ID, register one with
`ruzstd::decoding::Dictionary::from_raw_content` and select it with `FrameDecoder::force_dict` after
initializing the decoder for a frame. The specification only allows references into the dictionary while the
frame holds at most `window_size` bytes, which is one block (128 kB) for `Fastest` and 2 MiB for `Default`.

## Decompression

Additionally to the descriptions and the docs you can have a look at the zstd / zstd_streaming binaries. They showcase how this library can be used.

### Easy

The easiest is to wrap the io::Read into a StreamingDecoder which itself implements io::Read. It will decode blocks as necessary to fulfill the read requests

```rust, no_run
use ruzstd::decoding::StreamingDecoder;
use ruzstd::io::Read;

let mut source: &[u8] = todo!("Get a reader from a File or any other source");
let mut decoder = StreamingDecoder::new(&mut source).unwrap();

let mut result = Vec::new();
decoder.read_to_end(&mut result).unwrap();
```

This might be a problem if you are accepting user provided data. Frames can be REALLY big when decoded. If this is the case you should either check how big the frame
actually is or use the memory efficient approach described below.

### Memory efficient

If memory is a concern you can decode frames partially. There are two ways to do this:

#### Streaming decoder

Use the StreamingDecoder and use a while loop to fill your buffer (see src/bin/zstd_stream.rs for an example). This is the
recommended approach.

#### Use the lower level FrameDecoder

For an example see the src/bin/zstd.rs file. Basically you can decode the frame until either a
given block count has been decoded or the decodebuffer has reached a certain size. Then you can collect no longer needed bytes from the buffer and do something with them, discard them and resume decoding the frame in a loop until the frame has been decoded completely.

## Roadmap

1. More Performance optimizations
    1. sequence_decoding and reverse_bitreader::get_bits. Those account for about 50% of the whole time used in decoding
    2. Matching suffixes. This accounts for >60% of the whole time used in encoding
2. Implement encoder features
    1. Repeat offset codes and smarter entropy table selection
    2. Use the entropy tables of dictionaries

## Testing

Tests take two forms.

1. Tests using well-formed files that have to decode correctly and are checked against their originals
1. Tests using malformed input that have been generated by the fuzzer. These don't have to decode (they are garbage) but they must not make the decoder panic

## Fuzzing

Fuzzing has been done on

1. Random input with no initial corpus
2. The \*.zst in /fuzz_decodecorpus


### You want to help fuzz?

Use `cargo +nightly fuzz run decode` or some other fuzz target to run the fuzzer. It is seeded with files created with decodecorpus.

If the fuzzer finds a crash it will be saved to the artifacts dir by the fuzzer. Run `cargo test artifacts` to run the artifacts tests.
This will tell you where the decoder panics exactly. If you are able to fix the issue please feel free to do a pull request. If not please still submit the offending input and I will see how to fix it myself.

# Contributing

Contributions will be published under the same MIT license as this project. Please make an entry in the Changelog.md file when you make a PR.

## AI Contributions
Contributions may be created by using whatever tools you like. They must be read, verified and decided to be a value added to the project by a human (you! not a maintainer) before submitting a PR and the communication about the PR must be handled by a human and not an agent. This is meant to be more or less in the spirit of the [LLVM policy](https://llvm.org/docs/AIToolPolicy.html) adapted to a much smaller project.
