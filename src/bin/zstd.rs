extern crate ruzstd;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Compress a single file. If no output file is specified,
    /// output will be written to <INPUT_FILE>.zst
    Compress {
        /// File to compress
        input_file: PathBuf,
        /// Where the compressed file is written
        /// [default: <INPUT_FILE>.zst]
        output_file: Option<PathBuf>,
        /// How thoroughly the file should be compressed. A higher level will take
        /// more time to compress but result in a smaller file, and vice versa.
        ///
        /// - 0: Uncompressed
        /// - 1: Fastest
        /// - 2: Default
        /// - 3: Better
        /// - 4: Best
        #[arg(
            short,
            long,
            value_name = "COMPRESSION_LEVEL",
            default_value_t = 2,
            verbatim_doc_comment
        )]
        level: u8,
    },
    Decompress {},
    GenDict {},
}

fn main() -> color_eyre::Result<()> {
    let cli = Cli::parse();
    let command: Commands = &cli.command.wrap_err("no subcommand provided").unwrap();
    match command {
        Commands::Compress {
            input_file,
            output_file,
            level,
        } => {
            todo!();
        }
        _ => unimplemented!(),
    }
    Ok(())
}

fn compress(input: PathBuf, output: PathBuf, level: u8) -> color_eyre::Result<()> {
    let source = BufReader::new(File::open(input));
}
