//! Dictionaries for the compressor

use alloc::vec::Vec;

use crate::decoding::errors::DictionaryDecodeError;

/// A dictionary that is used to compress data. The decoder needs the same dictionary to decompress the data again.
///
/// The content of a dictionary acts as data that precedes the data of a frame, so the first part of the frame can
/// already reference typical patterns. This is most useful for many small inputs which share a lot of structure,
/// like log lines or serialized records. See the `dictionary` module (feature `dict_builder`) for creating them.
///
/// Two kinds of dictionaries are supported:
/// * Dictionaries in the Zstd dictionary format (as created by the reference `zstd --train`): the ID
///   is taken from the dictionary and written into the frame header. The entropy tables such a dictionary
///   contains are not used, the compressor always transmits its own tables, which is valid for any decoder.
/// * Raw content dictionaries: arbitrary bytes. They have no ID, the frame header does not reference them,
///   and the decoder needs to be told explicitly to use the dictionary.
///
/// The compressor only references the dictionary while the frame holds at most `window_size` bytes, this is the
/// limit the Zstd specification places on dictionary references. For [`super::CompressionLevel::Fastest`] that
/// is a single block, for [`super::CompressionLevel::Default`] 2 MiB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dictionary {
    id: u32,
    content: Vec<u8>,
}

impl Dictionary {
    /// Parse a dictionary like the reference implementation does: if `raw` starts with the dictionary magic number it
    /// has to be a complete, valid dictionary in the Zstd dictionary format. Otherwise all of `raw` is treated as
    /// a raw content dictionary.
    pub fn parse(raw: &[u8]) -> Result<Dictionary, DictionaryDecodeError> {
        if raw.starts_with(&crate::decoding::dictionary::MAGIC_NUM) {
            let decoded = crate::decoding::Dictionary::decode_dict(raw)?;
            Ok(Self::from_decoding_dictionary(&decoded))
        } else {
            Ok(Self::from_raw_content(raw.to_vec()))
        }
    }

    /// Create a raw content dictionary from arbitrary bytes. It has the ID 0, so no dictionary ID is written into
    /// the frame header.
    pub fn from_raw_content(content: Vec<u8>) -> Dictionary {
        Dictionary { id: 0, content }
    }

    /// Create a dictionary from a parsed decoder dictionary, using its ID and content
    pub fn from_decoding_dictionary(dictionary: &crate::decoding::Dictionary) -> Dictionary {
        Dictionary {
            id: dictionary.id,
            content: dictionary.dict_content.clone(),
        }
    }

    /// The ID that is written into the frame header, 0 if the dictionary has none
    pub fn id(&self) -> u32 {
        self.id
    }

    /// The content that can be referenced by the compressed data
    pub fn content(&self) -> &[u8] {
        &self.content
    }

    /// Consume the dictionary and return its content
    pub fn into_content(self) -> Vec<u8> {
        self.content
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::Dictionary;
    use crate::decoding::{BlockDecodingStrategy, FrameDecoder};
    use crate::encoding::{compress_to_vec, compress_to_vec_with_dictionary, CompressionLevel};
    use alloc::vec::Vec;

    const BLOCK: usize = 128 * 1024;

    const LEVELS: [CompressionLevel; 4] = [
        CompressionLevel::Fastest,
        CompressionLevel::Default,
        CompressionLevel::Better,
        CompressionLevel::Best,
    ];

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

    /// Records that share a lot of structure, like log lines
    fn records(count: usize, seed: u64) -> Vec<u8> {
        let mut state = seed;
        let mut data = Vec::new();
        for i in 0..count {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let level = ["INFO", "WARN", "ERROR", "DEBUG"][(state % 4) as usize];
            let module = [
                "ruzstd::encoding",
                "ruzstd::decoding",
                "ruzstd::fse",
                "ruzstd::huff0",
            ][((state >> 8) % 4) as usize];
            let line = alloc::format!(
                "2026-11-03T14:{:02}:{:02}.{:03}Z {level:<5} [{module}] request_id={:016x} user={} status=processed bytes={}\n",
                (i / 60) % 60,
                i % 60,
                state % 1000,
                state,
                (state >> 16) % 5000,
                (state >> 32) % 100_000
            );
            data.extend_from_slice(line.as_bytes());
        }
        data
    }

    fn decode_ruzstd(compressed: &[u8], dict: Option<&crate::decoding::Dictionary>) -> Vec<u8> {
        let mut decoder = FrameDecoder::new();
        let force = dict.map(|dict| dict.id);
        if let Some(dict) = dict {
            let dict =
                crate::decoding::Dictionary::from_raw_content(dict.id, dict.dict_content.clone());
            decoder.add_dict(dict).unwrap();
        }
        let mut input = compressed;
        decoder.reset(&mut input).unwrap();
        if let Some(id) = force {
            decoder.force_dict(id).unwrap();
        }
        let mut out = Vec::new();
        while !decoder.is_finished() || decoder.can_collect() > 0 {
            decoder
                .decode_blocks(&mut input, BlockDecodingStrategy::UptoBytes(1024 * 1024))
                .unwrap();
            decoder.collect_to_writer(&mut out).unwrap();
        }
        out
    }

    fn decode_zstd(compressed: &[u8], dict: &[u8]) -> Vec<u8> {
        use std::io::Read as _;
        // The streaming decoder of the reference implementation enforces the window rules strictly
        let mut decoder = zstd::stream::read::Decoder::with_dictionary(compressed, dict).unwrap();
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).unwrap();
        out
    }

    fn roundtrip(data: &[u8], dict: &Dictionary, level: CompressionLevel) -> usize {
        let compressed = compress_to_vec_with_dictionary(data, level, dict);
        let decoding_dict = crate::decoding::Dictionary::from_raw_content(
            dict.id().max(1),
            dict.content().to_vec(),
        );
        assert_eq!(
            decode_ruzstd(&compressed, Some(&decoding_dict)),
            data,
            "ruzstd roundtrip with dictionary failed at {:?}",
            level
        );
        assert_eq!(
            decode_zstd(&compressed, dict.content()),
            data,
            "zstd roundtrip with dictionary failed at {:?}",
            level
        );
        compressed.len()
    }

    #[test]
    fn raw_dictionary_improves_ratio_on_all_levels() {
        let dict = Dictionary::from_raw_content(records(400, 1));
        assert_eq!(dict.id(), 0);
        let data = records(300, 2);
        for level in LEVELS {
            let with_dict = roundtrip(&data, &dict, level);
            let without = compress_to_vec(data.as_slice(), level).len();
            assert!(
                with_dict < without,
                "{:?}: dictionary did not help: {} >= {}",
                level,
                with_dict,
                without
            );
        }
    }

    #[test]
    fn frames_with_raw_dictionary_carry_no_id() {
        let dict = Dictionary::from_raw_content(records(100, 3));
        let data = records(50, 4);
        let compressed =
            compress_to_vec_with_dictionary(data.as_slice(), CompressionLevel::Default, &dict);
        // Decodable without any dictionary registered as far as the header is concerned, but the content
        // references the dictionary so decoding must fail somewhere later
        let mut decoder = FrameDecoder::new();
        let mut input = compressed.as_slice();
        decoder.reset(&mut input).unwrap();
        assert!(decoder
            .decode_blocks(&mut input, BlockDecodingStrategy::All)
            .is_err());
    }

    #[test]
    fn tiny_and_empty_inputs_with_dictionary() {
        let dict = Dictionary::from_raw_content(records(50, 5));
        for level in LEVELS {
            roundtrip(&[], &dict, level);
            roundtrip(b"2", &dict, level);
            roundtrip(b"2026-11-03T14:00:01.000Z INFO", &dict, level);
            roundtrip(&records(1, 6), &dict, level);
        }
    }

    #[test]
    fn empty_dictionary_is_a_noop() {
        let dict = Dictionary::from_raw_content(Vec::new());
        let data = records(100, 7);
        for level in LEVELS {
            let with_dict = compress_to_vec_with_dictionary(data.as_slice(), level, &dict);
            let without = compress_to_vec(data.as_slice(), level);
            assert_eq!(with_dict, without);
        }
    }

    #[test]
    fn dictionary_references_stop_once_the_window_is_full() {
        // Fastest has a window of one block, Default of 2 MiB. Inputs larger than that must still roundtrip through
        // the strict streaming decoder of the reference implementation, which means no dictionary references after
        // the window is full.
        let dict = Dictionary::from_raw_content(records(2000, 8));
        let mut data = Vec::new();
        for i in 0..9u64 {
            data.extend_from_slice(&records(2500, 100 + i));
            data.extend_from_slice(&xorshift(10_000, 200 + i));
        }
        assert!(data.len() > 2 * 1024 * 1024 + 3 * BLOCK);
        for level in [CompressionLevel::Fastest, CompressionLevel::Default] {
            let with_dict = roundtrip(&data, &dict, level);
            let without = compress_to_vec(data.as_slice(), level).len();
            assert!(with_dict < without);
        }
    }

    #[test]
    fn large_dictionary() {
        // Larger than a block and larger than the lazy matcher's slack, so it is referenced across many blocks
        let dict = Dictionary::from_raw_content(records(6000, 9));
        assert!(dict.content().len() > 4 * BLOCK);
        let data = records(4000, 9);
        for level in [CompressionLevel::Fastest, CompressionLevel::Default] {
            let with_dict = roundtrip(&data, &dict, level);
            let without = compress_to_vec(data.as_slice(), level).len();
            assert!(with_dict < without);
        }
        // Default sees the same records again, the frame should almost vanish
        let with_dict = roundtrip(&data, &dict, CompressionLevel::Default);
        assert!(
            with_dict < data.len() / 20,
            "{} vs {}",
            with_dict,
            data.len()
        );
    }

    #[test]
    fn compressor_reuse_with_and_without_dictionary() {
        use crate::encoding::FrameCompressor;
        let dict = Dictionary::from_raw_content(records(300, 10));
        let decoding_dict =
            crate::decoding::Dictionary::from_raw_content(1, dict.content().to_vec());
        let data = records(200, 11);
        let mut compressor: FrameCompressor<&[u8], Vec<u8>, _> =
            FrameCompressor::new(CompressionLevel::Default);
        for (use_dict, level) in [
            (true, CompressionLevel::Default),
            (false, CompressionLevel::Default),
            (true, CompressionLevel::Fastest),
            (true, CompressionLevel::Default),
            (false, CompressionLevel::Fastest),
            (true, CompressionLevel::Best),
        ] {
            compressor.set_compression_level(level);
            if use_dict {
                compressor.set_dictionary(dict.clone());
                assert_eq!(compressor.dictionary(), Some(&dict));
            } else {
                compressor.take_dictionary();
                assert!(compressor.dictionary().is_none());
            }
            compressor.set_source(data.as_slice());
            compressor.set_drain(Vec::new());
            compressor.compress();
            let output = compressor.take_drain().unwrap();
            let dict_for_decoding = if use_dict { Some(&decoding_dict) } else { None };
            assert_eq!(decode_ruzstd(&output, dict_for_decoding), data);
            if use_dict {
                assert_eq!(decode_zstd(&output, dict.content()), data);
            } else {
                assert_eq!(decode_zstd(&output, &[]), data);
            }
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn zstd_format_dictionary_from_the_test_corpus() {
        use std::io::Read;
        let raw = std::fs::read("./dict_tests/dictionary").unwrap();
        let dict = Dictionary::parse(&raw).unwrap();
        let decoding_dict = crate::decoding::Dictionary::decode_dict(&raw).unwrap();
        assert_eq!(dict.id(), decoding_dict.id);
        assert_ne!(dict.id(), 0);
        assert_eq!(dict.content(), decoding_dict.dict_content.as_slice());
        assert_eq!(dict, Dictionary::from_decoding_dictionary(&decoding_dict));

        let mut files: Vec<_> = std::fs::read_dir("./dict_tests/files")
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_none_or(|ext| ext != "zst"))
            .collect();
        files.sort();
        assert!(!files.is_empty());

        let mut total_with = 0;
        let mut total_without = 0;
        for path in files {
            let data = std::fs::read(&path).unwrap();
            for level in LEVELS {
                let compressed = compress_to_vec_with_dictionary(data.as_slice(), level, &dict);
                // The frame header carries the ID, so the decoder picks the dictionary up by itself
                let mut decoder = FrameDecoder::new();
                decoder
                    .add_dict(crate::decoding::Dictionary::decode_dict(&raw).unwrap())
                    .unwrap();
                let mut streaming = crate::decoding::StreamingDecoder::new_with_decoder(
                    compressed.as_slice(),
                    decoder,
                )
                .unwrap();
                let mut decoded = Vec::new();
                streaming.read_to_end(&mut decoded).unwrap();
                assert_eq!(decoded, data, "{:?} at {:?}", path, level);
                assert_eq!(
                    decode_zstd(&compressed, &raw),
                    data,
                    "{:?} at {:?}",
                    path,
                    level
                );

                // Without the dictionary registered the decoder has to refuse the frame
                let mut decoder = FrameDecoder::new();
                assert!(decoder.reset(compressed.as_slice()).is_err());

                if matches!(level, CompressionLevel::Default) {
                    total_with += compressed.len();
                    total_without += compress_to_vec(data.as_slice(), level).len();
                }
            }
        }
        assert!(
            total_with < total_without / 2,
            "dictionary should help a lot on these small files: {} vs {}",
            total_with,
            total_without
        );
    }

    #[test]
    fn parse_rejects_truncated_zstd_format_dictionaries() {
        let mut raw = crate::decoding::dictionary::MAGIC_NUM.to_vec();
        raw.extend_from_slice(&[1, 0, 0, 0, 5, 6]);
        assert!(Dictionary::parse(&raw).is_err());
        // Anything without the magic number is raw content
        let dict = Dictionary::parse(b"just some bytes").unwrap();
        assert_eq!(dict.id(), 0);
        assert_eq!(dict.content(), b"just some bytes");
        assert_eq!(dict.clone().into_content(), b"just some bytes".to_vec());
    }
}
