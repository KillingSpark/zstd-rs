extern crate zstd_rs;
use std::fs::File;
use std::io::Read;

struct StateTracker {
    bytes_used: u64,
    old_percentage: i8,
}

fn main() {
    let mut file_paths: Vec<_> = std::env::args().collect();
    file_paths.remove(0);
    let mut tracker = StateTracker {
        bytes_used: 0,
        old_percentage: -1,
    };

    let mut frame_dec = zstd_rs::FrameDecoder::new();

    for path in file_paths {
        println!("File: {}", path);
        let mut f = File::open(path).unwrap();

        frame_dec.reset(&mut f).unwrap();

        let batch_size = 1024 * 1024;
        let mut result = Vec::with_capacity(batch_size);
        result.resize(batch_size, 0);

        while !frame_dec.is_finished() {
            frame_dec
                .decode_blocks(
                    &mut f,
                    zstd_rs::BlockDecodingStrategy::UptoBytes(batch_size),
                )
                .unwrap();

            if frame_dec.can_collect() > batch_size {
                let x = frame_dec.read(result.as_mut_slice()).unwrap();

                result.resize(x, 0);
                do_something(&mut result, &mut tracker);
                result.resize(result.capacity(), 0);

                let percentage = (tracker.bytes_used * 100) / frame_dec.content_size().unwrap();
                if percentage as i8 != tracker.old_percentage {
                    //println!("{}", percentage);
                    tracker.old_percentage = percentage as i8;
                }
            }
        }
        println!("");
        while frame_dec.can_collect() > 0 {
            let x = frame_dec.read(result.as_mut_slice()).unwrap();

            result.resize(x, 0);
            do_something(&mut result, &mut tracker);
            result.resize(result.capacity(), 0);
        }

        // handle the last chunk of data
        do_something(&mut result, &mut tracker);
        println!("Decoded bytes: {}", tracker.bytes_used);
    }
}

fn do_something(data: &Vec<u8>, s: &mut StateTracker) {
    //Do something. Like writing it to a file or to stdout...
    use std::io::Write;
    std::io::stdout().write_all(data).unwrap();
    s.bytes_used += data.len() as u64;
}
