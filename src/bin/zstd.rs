extern crate zstd_rs;
use std::fs::File;
use std::io::Read;
use std::io::Write;

struct StateTracker {
    bytes_used: u64,
    old_percentage: i8,
}

fn main() {
    let mut file_paths: Vec<_> = std::env::args().filter(|f| !f.starts_with("-")).collect();
    let flags: Vec<_> = std::env::args().filter(|f| f.starts_with("-")).collect();
    file_paths.remove(0);

    if !flags.contains(&"-d".to_owned()) {
        std::io::stderr().write(b"This zstd implementation only supports decompression. Please add a \"-d\" flag").unwrap();
        return;
    }

    if !flags.contains(&"-c".to_owned()) {
        std::io::stderr().write(b"This zstd implementation only supports output on the stdout. Please add a \"-c\" flag and pipe the output into a file").unwrap();
        return;
    }

    if flags.len() != 2{
        std::io::stderr().write_fmt(format_args!("No flags other than -d and -c are currently implemented. Flags used: {:?}", flags)).unwrap();
        return;
    }

    let mut frame_dec = zstd_rs::FrameDecoder::new();

    for path in file_paths {
        let mut tracker = StateTracker {
            bytes_used: 0,
            old_percentage: -1,
        };

        std::io::stderr()
            .write_fmt(format_args!("File: {}\n", path))
            .unwrap();
        let mut f = File::open(path).unwrap();

        frame_dec.reset(&mut f).unwrap();

        let batch_size = 1024 * 1024 * 10;
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
                    std::io::stderr().write_fmt(format_args!("\r")).unwrap();
                    std::io::stderr()
                        .write_fmt(format_args!("{} % done", percentage))
                        .unwrap();
                    tracker.old_percentage = percentage as i8;
                }
            }
        }

        // handle the last chunk of data
        while frame_dec.can_collect() > 0 {
            let x = frame_dec.read(result.as_mut_slice()).unwrap();

            result.resize(x, 0);
            do_something(&mut result, &mut tracker);
            result.resize(result.capacity(), 0);
        }

        std::io::stderr()
            .write_fmt(format_args!("\nDecoded bytes: {}\n", tracker.bytes_used))
            .unwrap();
    }
}

fn do_something(data: &Vec<u8>, s: &mut StateTracker) {
    //Do something. Like writing it to a file or to stdout...
    std::io::stdout().write_all(data).unwrap();
    s.bytes_used += data.len() as u64;
}
