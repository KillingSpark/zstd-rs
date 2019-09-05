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
