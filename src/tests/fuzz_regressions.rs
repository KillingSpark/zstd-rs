#[test]
fn test_all_artifacts() {
    use crate::frame_decoder;
    use std::fs;
    use std::fs::File;

    for file in fs::read_dir("./fuzz/artifacts/fuzz_target_1").unwrap() {
        let file_name = file.unwrap().path();
        let mut f = File::open(file_name.clone()).unwrap();
        match frame_decoder::FrameDecoder::new(&mut f) {
            Ok(mut frame_dec) => {
                let _ = frame_dec.decode_blocks(&mut f);
                /* ignore errors. It just should never panic on invalid input */
            }
            Err(_) => {} /* ignore errors. It just should never panic on invalid input */
        }
    }
}
