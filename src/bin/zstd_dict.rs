use ruzstd::dictionary::{create_dict_from_dir, create_dict_from_source};
use std::cell::RefCell;
use std::fmt::Display;
use std::fs::File;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::{env::args, fs};

fn main() {
    //let args: Vec<String> = args().collect();
    //let input_path: &Path = args.get(1).expect("no input provided").as_ref();
    //let output_path: &Path = args.get(2).expect("no output path provided").as_ref();
    //let dict_size = args
    //    .get(3)
    //    .expect("no dict size provided (kb)")
    //    .parse::<usize>()
    //    .expect("dict size was not a valid num");
    //
    //let mut output = File::create(output_path).unwrap();
    //if input_path.is_file() {
    //    let source = File::open(input_path).expect("unable to open input path");
    //    let source_size = source.metadata().unwrap().len();
    //    create_dict_from_source(source, source_size as usize, &mut output, dict_size);
    //} else {
    //    create_dict_from_dir(input_path, &mut output, dict_size).unwrap();
    //}
    print!("{}", bench("local_corpus_files/sat-txt-files/"));
}

struct BenchmarkResults {
    uncompressed_size: usize,
    nodict_size: usize,
    reference_size: usize,
    our_size: usize,
}

impl Display for BenchmarkResults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "uncompressed: 100.00% ({})", self.uncompressed_size)?;
        writeln!(
            f,
            "no dict: {:.2}% of original size ({})",
            f64::from(self.nodict_size as u32) / f64::from(self.uncompressed_size as u32) * 100.0,
            self.nodict_size
        )?;
        writeln!(
            f,
            "reference dict: {:.2}% of no dict size ({} bytes smaller)",
            f64::from(self.reference_size as u32) / f64::from(self.nodict_size as u32) * 100.0,
            self.nodict_size - self.reference_size
        )?;
        write!(
            f,
            "our dict: {:.2}% of no dict size ({} bytes smaller)",
            f64::from(self.our_size as u32) / f64::from(self.nodict_size as u32) * 100.0,
            self.nodict_size - self.our_size
        )?;
        Ok(())
    }
}

struct Dumpster(pub usize);

impl Write for Dumpster {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn bench<P: AsRef<Path>>(input_path: P) -> BenchmarkResults {
    // At what compression level the dicts are built with
    let compression_level = 1;
    // 1. Collect a list of a path to every file in the directory into `file_paths`
    println!("[bench]: collecting list of input files");
    let mut file_paths: Vec<PathBuf> = Vec::new();
    let dir: fs::ReadDir = fs::read_dir(&input_path).expect("read input path");
    fn recurse_read(dir: fs::ReadDir, file_paths: &mut Vec<PathBuf>) -> Result<(), io::Error> {
        for entry in dir {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                recurse_read(fs::read_dir(&entry.path())?, file_paths)?;
            } else {
                file_paths.push(entry.path());
            }
        }
        Ok(())
    }
    recurse_read(dir, &mut file_paths).expect("recursing over input dir");

    // 2. Create two dictionaries, one with our strategy, and one with theirs
    println!("[bench]: creating reference dict");
    let reference_dict =
        zstd::dict::from_files(file_paths.iter(), 112640).expect("create reference dict");
    let mut our_dict = Vec::with_capacity(112640);
    println!("[bench]: creating our dict");
    create_dict_from_dir(input_path, &mut our_dict, 112640).expect("create our dict");
    // Open each file and compress it
    let mut uncompressed_size: usize = 0;
    let mut nodict_size: usize = 0;

    let mut reference_output = Dumpster(0);
    let mut reference_encoder =
        zstd::Encoder::with_dictionary(&mut reference_output, compression_level, &reference_dict)
            .unwrap();
    reference_encoder.multithread(8).unwrap();
    let mut our_output = Dumpster(0);
    let mut our_encoder =
        zstd::Encoder::with_dictionary(&mut our_output, compression_level, &our_dict).unwrap();
    our_encoder.multithread(8).unwrap();
    for (idx, path) in file_paths.iter().enumerate() {
        if idx % 10 == 0 {
            println!("[bench]: compressing file {}/{}", idx + 1, file_paths.len());
        }
        let mut handle = File::open(path).unwrap();
        let mut data = Vec::new();
        handle.read_to_end(&mut data).unwrap();
        uncompressed_size += data.len();
        // Compress with no dict
        let nodict_output = zstd::encode_all(data.as_slice(), compression_level).unwrap();
        nodict_size += nodict_output.len();
        // Compress with the reference dict
        reference_encoder
            .write_all(data.as_slice())
            .expect("reference writer writing");
        // Compress with our dict
        our_encoder
            .write_all(data.as_slice())
            .expect("our writer writing");
    }
    //println!("[bench]: reading all files");
    //let mut all_files: Vec<u8> = Vec::with_capacity(1_000_000);
    //for path in file_paths {
    //    let mut handle = File::open(path).unwrap();
    //    handle
    //        .read_to_end(&mut all_files)
    //        .expect("reading input file");
    //}
    //uncompressed_size = all_files.len();
    ////    // Compress with no dict
    //println!("[bench]: compressing using no dict");
    //let nodict_output = zstd::encode_all(all_files.as_slice(), compression_level).unwrap();
    //nodict_size = nodict_output.len();
    //drop(nodict_output);
    //println!("[bench]: compressing using reference encoder");
    //reference_encoder
    //    .write_all(&all_files)
    //    .expect("writing to reference encoder");
    //println!("[bench]: compressing using our encoder");
    //our_encoder
    //    .write_all(&all_files)
    //    .expect("writing to our encoder");
    //our_encoder.do_finish().expect("our encoder finishes");
    //reference_encoder
    //    .do_finish()
    //    .expect("reference encoder finishes");
    //drop(reference_encoder);
    //drop(our_encoder);

    BenchmarkResults {
        uncompressed_size,
        nodict_size,
        reference_size: reference_output.0,
        our_size: our_output.0,
    }
}
