#![no_main]
#[macro_use] extern crate libfuzzer_sys;
extern crate ruzstd;
use ruzstd::frame_decoder;

fuzz_target!(|data: &[u8]| {
    let mut content = data.clone();
    let mut frame_dec = frame_decoder::FrameDecoder::new();

    match frame_dec.reset(&mut content){
        Ok(_) => {
            let _ = frame_dec.decode_blocks(&mut content,frame_decoder::BlockDecodingStrategy::All);
        }
        Err(_) => {/* nothing */}
    }
});
