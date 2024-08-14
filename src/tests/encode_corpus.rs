#[test]
fn test_encode_corpus_files_uncompressed() {
    extern crate std;
    use crate::encoding::frame_encoder::FrameCompressor;
    use alloc::borrow::ToOwned;
    use alloc::vec::Vec;
    use std::ffi::OsStr;
    use std::fs;
    use std::io::Read;
    use std::println;

    let mut files: Vec<_> = fs::read_dir("./decodecorpus_files").unwrap().collect();
    if fs::read_dir("./local_corpus_files").is_ok() {
        files.extend(fs::read_dir("./local_corpus_files").unwrap());
    }

    files.sort_by_key(|x| match x {
        Err(_) => "".to_owned(),
        Ok(entry) => entry.path().to_str().unwrap().to_owned(),
    });

    for entry in files.iter().map(|f| f.as_ref().unwrap()) {
        let path = entry.path();
        if path.extension() == Some(OsStr::new("zst")) {
            continue;
        }
        println!("Trying file: {:?}", path);
        let input = fs::read(entry.path()).unwrap();

        let compressor = FrameCompressor::new(
            &input,
            crate::encoding::frame_encoder::CompressionLevel::Uncompressed,
        );
        let mut compressed_file: Vec<u8> = Vec::new();
        compressor.compress(&mut compressed_file);
        let mut decompressed_output = Vec::new();
        // zstd::stream::copy_decode(compressed_file.as_slice(), &mut decompressed_output).unwrap();
        let mut decoder =
            crate::streaming_decoder::StreamingDecoder::new(compressed_file.as_slice()).unwrap();
        decoder.read_to_end(&mut decompressed_output).unwrap();

        assert_eq!(input, decompressed_output);
    }
}
