#[test]
fn test_decode_corpus_files() {
    use crate::frame_decoder;
    use std::fs;

    for file in fs::read_dir("./decodecorpus_files").unwrap() {
        let f = file.unwrap();

        let p = String::from(f.path().to_str().unwrap());
        if !p.ends_with(".zst"){
            continue;
        }
        println!("Trying file: {}", p);

        let mut content = fs::File::open(f.path()).unwrap();

        struct NullWriter(());
        impl std::io::Write for NullWriter {
            fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
                Ok(buf.len())
            }
            fn flush(&mut self) -> Result<(), std::io::Error> {
                Ok(())
            }
        }
        let mut null_target = NullWriter(());

        let mut frame_dec = frame_decoder::FrameDecoder::new(&mut content);
        frame_dec
            .decode_blocks(&mut content, &mut null_target)
            .unwrap();
    }
}
