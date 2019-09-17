#[test]
fn test_decode_corpus_files() {
    use crate::frame_decoder;
    use std::fs;
    use std::io::Read;

    let mut success_counter = 0;
    let mut fail_counter_diff = 0;
    let mut fail_counter_size = 0;
    let mut fail_counter_bytes_read = 0;
    let mut total_counter = 0;
    let mut failed: Vec<String> = Vec::new();

    let mut speeds = Vec::new();

    let mut files: Vec<_> = fs::read_dir("./decodecorpus_files").unwrap().collect();
    if fs::read_dir("./local_corpus_files").is_ok() {
        files.extend(fs::read_dir("./local_corpus_files").unwrap());
    }

    files.sort_by_key(|x| {
        match x {
            Err(_) => "".to_owned(),
            Ok(entry) => {
                entry.path().to_str().unwrap().to_owned()
            }
        }
    });

    let mut frame_dec = frame_decoder::FrameDecoder::new();

    for file in files {
        let f = file.unwrap();
        let metadata = f.metadata().unwrap();
        let file_size = metadata.len();

        let p = String::from(f.path().to_str().unwrap());
        if !p.ends_with(".zst") {
            continue;
        }
        println!("Trying file: {}", p);

        let mut content = fs::File::open(f.path()).unwrap();

        frame_dec.reset(&mut content).unwrap();

        let start_time = std::time::Instant::now();
        /////DECODING
        frame_dec.decode_blocks(&mut content, frame_decoder::BlockDecodingStrategy::All).unwrap();
        let result = frame_dec.drain_buffer();
        let end_time = start_time.elapsed();

        let mut original_p = p.clone();
        original_p.truncate(original_p.len() - 4);
        let original_f = fs::File::open(original_p).unwrap();
        let original: Vec<u8> = original_f.bytes().map(|x| x.unwrap()).collect();

        println!("Results for file: {}", p.clone());
        let mut success = true;

        if original.len() != result.len() {
            println!(
                "Result has wrong length: {}, should be: {}",
                result.len(),
                original.len()
            );
            success = false;
            fail_counter_size += 1;
        }

        if frame_dec.bytes_read_from_source() != file_size as u64 {
            println!(
                "Framedecoder counted wrong amount of bytes: {}, should be: {}",
                frame_dec.bytes_read_from_source(),
                file_size
            );
            success = false;
            fail_counter_bytes_read += 1;
        }

        let mut counter = 0;
        let min = if original.len() < result.len() {
            original.len()
        } else {
            result.len()
        };
        for idx in 0..min {
            if !(original[idx] == result[idx]) {
                counter += 1;
                //println!(
                //    "Original {} not equal to result {} at byte: {}",
                //    original[idx], result[idx], idx,
                //);
            }
        }

        if counter > 0 {
            println!("Result differs in at least {} bytes from original", counter);
            success = false;
            fail_counter_diff += 1;
        }

        if success {
            success_counter += 1;
        } else {
            failed.push(p.clone().to_string());
        }
        total_counter += 1;

        let dur = end_time.as_micros() as usize;
        let speed = result.len() / if dur == 0 { 1 } else { dur };
        println!("SPEED: {}", speed);
        speeds.push(speed);
    }

    println!("###################");
    println!("Summary:");
    println!("###################");
    println!(
        "Total: {}, Success: {}, WrongSize: {}, WrongBytecount: {}, Diffs: {}",
        total_counter, success_counter, fail_counter_size, fail_counter_bytes_read, fail_counter_diff
    );
    println!("Failed files: ");
    for f in &failed {
        println!("{}", f);
    }
    let speed_len = speeds.len();
    let sum_speed: usize = speeds.into_iter().sum();
    let avg_speed = sum_speed / speed_len;
    let avg_speed_bps = avg_speed * 1000000;
    if avg_speed_bps < 1000 {
        println!("Average speed: {} B/s", avg_speed_bps);
    } else {
        if avg_speed_bps < 1000000 {
            println!("Average speed: {} KB/s", avg_speed_bps / 1000);
        } else {
            println!("Average speed: {} MB/s", avg_speed_bps / 1000000);
        }
    }

    assert!(failed.len() == 0);
}
