#[test]
fn test_decode_corpus_files() {
    use crate::frame_decoder;
    use std::fs;
    use std::io::Read;

    let mut success_counter = 0;
    let mut fail_counter_diff = 0;
    let mut fail_counter_size = 0;
    let mut total_counter = 0;
    let mut failed: Vec<String> = Vec::new();

    for file in fs::read_dir("./decodecorpus_files").unwrap() {
        let f = file.unwrap();

        let p = String::from(f.path().to_str().unwrap());
        if !p.ends_with(".zst") {
            continue;
        }
        println!("Trying file: {}", p);

        let mut content = fs::File::open(f.path()).unwrap();

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

        let result = frame_dec.drain_buffer_completely();

        let mut original_p = p.clone();
        original_p.truncate(original_p.len() - 4);
        let original_f = fs::File::open(original_p).unwrap();
        let original: Vec<u8> = original_f.bytes().map(|x| x.unwrap()).collect();

        println!("Results for file: {}", p.clone());
        let mut success = true;

        if !(original.len() == result.len()) {
            println!(
                "Result has wrong length: {}, should be: {}",
                result.len(),
                original.len()
            );
            success = false;
            fail_counter_size += 1;
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
        }else{
            failed.push(p.clone().to_string());
        }
        total_counter += 1;
    }

    println!("###################");
    println!("Summary:");
    println!("###################");
    println!("Total: {}, Success: {}, WrongSize: {}, Diffs: {}", total_counter, success_counter, fail_counter_size, fail_counter_diff);
    println!("Failed files: ");
    for f in failed {
        println!("{}", f);
    }

}
