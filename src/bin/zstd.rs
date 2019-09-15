extern crate zstd_rs;
use std::fs::File;
use zstd_rs::frame_decoder;

fn main() {
    let mut file_paths: Vec<_> = std::env::args().collect();
    file_paths.remove(0);

    for path in file_paths {
        println!("File: {}", path);
        let mut f = File::open(path).unwrap();

        let mut frame_dec = frame_decoder::FrameDecoder::new(&mut f).unwrap();

        let mut result = Vec::new();

        while !frame_dec.is_finished() {
            frame_dec
                .decode_blocks(
                    &mut f,
                    frame_decoder::BlockDecodingStrategy::UptoBytes(2048),
                )
                .unwrap();
            if frame_dec.can_collect() > 0 {
                let x = frame_dec.collect_to_writer(&mut result).unwrap();
                println!("Collected bytes: {}", x);
                
                // If we collected some sensible amount of data do something with it
                if result.len() > 2048 {
                    do_something(&mut result);
                }
            }
        }
        let x = frame_dec.drain_buffer_to_writer(&mut result).unwrap();
        println!("Collected bytes in final drain: {}", x);

        // handle the last chunk of data
        do_something(&mut result);
    }
}

fn do_something(data: &mut Vec<u8>) {
    //Do something. Like writing it to a file or to stdout...
    data.clear();
}
