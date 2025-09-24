use ruzstd::dictionary::{create_raw_dict_from_dir, create_raw_dict_from_source};
use std::env::args;
use std::fs::File;
use std::path::Path;

fn main() {
    let args: Vec<String> = args().collect();
    let input_path: &Path = args.get(1).expect("no input provided").as_ref();
    let output_path: &Path = args.get(2).expect("no output path provided").as_ref();
    let dict_size = args
        .get(3)
        .expect("no dict size provided (kb)")
        .parse::<usize>()
        .expect("dict size was not a valid num");

    let mut output = File::create(output_path).unwrap();
    if input_path.is_file() {
        let source = File::open(input_path).expect("unable to open input path");
        let source_size = source.metadata().unwrap().len();
        create_raw_dict_from_source(source, source_size as usize, &mut output, dict_size);
    } else {
        create_raw_dict_from_dir(input_path, &mut output, dict_size).unwrap();
    }
}
