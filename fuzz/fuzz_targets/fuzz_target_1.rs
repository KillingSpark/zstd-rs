#![no_main]
#[macro_use] extern crate libfuzzer_sys;
extern crate zstd_rs;
use zstd_rs::frame_decoder;

fuzz_target!(|data: &[u8]| {
    let mut content = data.clone();
    match frame_decoder::FrameDecoder::new(&mut content){
        Ok(mut frame_dec) => {
            let _ = frame_dec.decode_blocks(&mut content);
        }
        Err(_) => {/* nothing */}
    }
});
