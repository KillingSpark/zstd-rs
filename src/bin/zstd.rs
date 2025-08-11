extern crate ruzstd;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::time::Instant;

use ruzstd::decoding::errors::FrameDecoderError;
use ruzstd::decoding::errors::ReadFrameHeaderError;
use ruzstd::dictionary::create_dict_from_source;
use ruzstd::encoding::CompressionLevel;
use ruzstd::encoding::FrameCompressor;

struct StateTracker {
    bytes_used: u64,
    frames_used: usize,
    valid_checksums: usize,
    invalid_checksums: usize,
    file_pos: u64,
    file_size: u64,
    old_percentage: i8,
}

fn decompress(flags: &[String], file_paths: &[String]) {
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

    let mut frame_dec = ruzstd::decoding::FrameDecoder::new();

    for path in file_paths {
        eprintln!("File: {}", path);
        let mut f = File::open(path).unwrap();

        let mut tracker = StateTracker {
            bytes_used: 0,
            frames_used: 0,
            valid_checksums: 0,
            invalid_checksums: 0,
            file_size: f.metadata().unwrap().len(),
            file_pos: 0,
            old_percentage: -1,
        };

        let batch_size = 1024 * 1024 * 10;
        let mut result = vec![0; batch_size];

        while tracker.file_pos < tracker.file_size {
            match frame_dec.reset(&mut f) {
                Err(FrameDecoderError::ReadFrameHeaderError(ReadFrameHeaderError::SkipFrame {
                    magic_number: magic_num,
                    length: skip_size,
                })) => {
                    eprintln!("Found a skippable frame with magic number: {magic_num} and size: {skip_size}");
                    tracker.file_pos = f.stream_position().unwrap();
                    tracker.file_pos += skip_size as u64;
                    f.seek(SeekFrom::Current(skip_size as i64)).unwrap();
                    continue;
                }
                other => other.unwrap(),
            }

            tracker.frames_used += 1;

            while !frame_dec.is_finished() {
                frame_dec
                    .decode_blocks(
                        &mut f,
                        ruzstd::decoding::BlockDecodingStrategy::UptoBytes(batch_size),
                    )
                    .unwrap();

                if frame_dec.can_collect() > batch_size {
                    let x = frame_dec.read(result.as_mut_slice()).unwrap();
                    tracker.file_pos = f.stream_position().unwrap();
                    do_something(&result[..x], &mut tracker);
                }
            }

            // handle the last chunk of data
            while frame_dec.can_collect() > 0 {
                let x = frame_dec.read(result.as_mut_slice()).unwrap();
                tracker.file_pos = f.stream_position().unwrap();
                do_something(&result[..x], &mut tracker);
            }

            #[cfg(feature = "hash")]
            if let Some(chksum) = frame_dec.get_checksum_from_data() {
                if frame_dec.get_calculated_checksum().unwrap() != chksum {
                    tracker.invalid_checksums += 1;
                    eprintln!(
                        "Checksum did not match in frame {}! From data: {}, calculated while decoding: {}",
                        tracker.frames_used,
                        chksum,
                        frame_dec.get_calculated_checksum().unwrap()
                    );
                } else {
                    tracker.valid_checksums += 1;
                }
            }
        }

        eprintln!(
            "\nDecoded frames: {}  bytes: {}",
            tracker.frames_used, tracker.bytes_used
        );
        if tracker.valid_checksums == 0 && tracker.invalid_checksums == 0 {
            eprintln!("No checksums to test");
        } else {
            eprintln!(
                "{} of {} checksums are ok!",
                tracker.valid_checksums,
                tracker.valid_checksums + tracker.invalid_checksums,
            );
        }
    }
}

struct PercentPrintReader<R: Read> {
    total: usize,
    counter: usize,
    last_percent: usize,
    reader: R,
}

impl<R: Read> Read for PercentPrintReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let new_bytes = self.reader.read(buf)?;
        self.counter += new_bytes;
        let progress = self.counter * 100 / self.total;
        if progress > self.last_percent {
            self.last_percent = progress;
            eprint!("\r");
            eprint!("{} % done", progress);
        }
        Ok(new_bytes)
    }
}

fn main() {
    let input = File::open("ik9").expect("open input file");
    //let input = File::open("local_corpus_files/enwik9").expect("open input file");
    let input_len = input.metadata().unwrap().len() as usize;
    let mut output = File::create("output.dict").expect("create output file");
    create_dict_from_source(input, input_len, &mut output, 5_000_000);
    //let mut file_paths: Vec<_> = std::env::args().filter(|f| !f.starts_with('-')).collect();
    //let flags: Vec<_> = std::env::args().filter(|f| f.starts_with('-')).collect();
    //file_paths.remove(0);
    //
    //if flags.is_empty() {
    //    let mut encoder = FrameCompressor::new(CompressionLevel::Fastest);
    //    encoder.set_drain(Vec::new());
    //
    //    for path in file_paths {
    //        let start_instant = Instant::now();
    //        let file = std::fs::File::open(&path).unwrap();
    //        let input_len = file.metadata().unwrap().len() as usize;
    //        let file = PercentPrintReader {
    //            reader: BufReader::new(file),
    //            total: input_len,
    //            counter: 0,
    //            last_percent: 0,
    //        };
    //        encoder.set_source(file);
    //        encoder.compress();
    //        let mut output: Vec<_> = encoder.take_drain().unwrap();
    //        println!(
    //            "Compressed {path:} from {} to {} ({}%) took {}ms",
    //            input_len,
    //            output.len(),
    //            if input_len == 0 {
    //                0
    //            } else {
    //                output.len() * 100 / input_len
    //            },
    //            start_instant.elapsed().as_millis()
    //        );
    //        output.clear();
    //        encoder.set_drain(output);
    //    }
    //} else {
    //    decompress(&flags, &file_paths);
    //}
}

fn do_something(data: &[u8], s: &mut StateTracker) {
    //Do something. Like writing it to a file or to stdout...
    std::io::stdout().write_all(data).unwrap();
    s.bytes_used += data.len() as u64;

    let percentage = (s.file_pos * 100) / s.file_size;
    if percentage as i8 != s.old_percentage {
        eprint!("\r");
        eprint!("{} % done", percentage);
        s.old_percentage = percentage as i8;
    }
}
