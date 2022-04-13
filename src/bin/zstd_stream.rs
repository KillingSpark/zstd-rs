extern crate ruzstd;
use std::fs::File;
use std::io::{Read, Write};

fn main() {
    let mut file_paths: Vec<_> = std::env::args().filter(|f| !f.starts_with('-')).collect();
    let flags: Vec<_> = std::env::args().filter(|f| f.starts_with('-')).collect();
    file_paths.remove(0);

    if !flags.contains(&"-d".to_owned()) {
        eprintln!("This zstd implementation only supports decompression. Please add a \"-d\" flag");
        return;
    }

    if !flags.contains(&"-c".to_owned()) {
        eprintln!("This zstd implementation only supports output on the stdout. Please add a \"-c\" flag and pipe the output into a file");
        return;
    }

    if flags.len() != 2 {
        eprintln!(
            "No flags other than -d and -c are currently implemented. Flags used: {:?}",
            flags
        );
        return;
    }

    for path in file_paths {
        eprintln!("File: {}", path);
        let f = File::open(path).unwrap();
        let mut buf_read = std::io::BufReader::new(f);

        let mut decoder = ruzstd::StreamingDecoder::new(&mut buf_read).unwrap();
        let mut buf = [0u8; 1024 * 1024];
        let mut stdout = std::io::stdout();
        while !decoder.decoder.is_finished() || decoder.decoder.can_collect() > 0 {
            let bytes = decoder.read(&mut buf[..]).unwrap();
            stdout.write_all(&buf[..bytes]).unwrap();
        }
    }
}
