#[cfg(test)]
#[test]
fn test_frame_header_reading() {
    use crate::frame;
    use std::fs;

    let mut content = fs::File::open("./test_img.zst").unwrap();
    let frame = frame::read_frame_header(&mut content).unwrap();
    frame.check_valid().unwrap();
}

#[test]
fn test_block_header_reading() {
    use crate::block;
    use crate::frame;
    use std::fs;

    let mut content = fs::File::open("/home/moritz/rust/zstd-rs/test_img.zst").unwrap();
    let frame = frame::read_frame_header(&mut content).unwrap();
    frame.check_valid().unwrap();

    let mut block_dec = block::block_decoder::new();
    let block_header = block_dec.read_block_header(&mut content).unwrap();
    let _ = block_header; //TODO validate blockheader in a smart way
}

#[test]
fn test_frame_decoder() {
    use crate::frame_decoder;
    use std::fs;

    let mut content = fs::File::open("/home/moritz/rust/zstd-rs/test_img.zst").unwrap();

    struct NullWriter(());
    impl std::io::Write for NullWriter {
        fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> Result<(), std::io::Error> {
            Ok(())
        }
    }
    let mut _null_target = NullWriter(());

    let mut frame_dec = frame_decoder::FrameDecoder::new(&mut content);
    frame_dec.decode_blocks(&mut content).unwrap();
}

pub mod decode_corpus;