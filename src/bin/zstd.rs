extern crate ruzstd;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};
use color_eyre::eyre::{ContextCompat, WrapErr};
use ruzstd::encoding::CompressionLevel;

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
    let command: Commands = cli.command.wrap_err("no subcommand provided").unwrap();
    match command {
        Commands::Compress {
            input_file,
            output_file,
            level,
        } => {
            let output_file = output_file.unwrap_or(Path::join(&input_file, ".zst"));
            compress(input_file, output_file, level)?;
        }
        _ => unimplemented!(),
    }
    Ok(())
}

/// A generic wrapper around a reader that keeps track of how many bytes have been read
/// from the total.
///
/// This wrapper has a lock on standard out for the lifetime of the monitor,
/// any logging (TODO) should be done via methods on this struct
pub struct ProgressMonitor<R: Read> {
    /// The total amount that the reader will read
    pub total: usize,
    /// Amount read so far
    pub read: usize,
    /// The internal reader
    reader: R,
}

impl<R: Read> ProgressMonitor<R> {
    /// Create a new progress monitor, initialized with zero bytes read
    fn new(reader: R, size: usize) -> Self {
        Self {
            reader,
            total: size,
            read: 0,
        }
    }

    // This function is called whenever a new read is made, and is responsible for updating the UI
    fn update() {}
}

impl<R: Read> Read for ProgressMonitor<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Fall back on the internally stored reader, but filch the number of bytes read
        // along the way
        let out = self.reader.read(buf)?;
        self.read += out;
        Ok(out)
    }
}

fn compress(input: PathBuf, output: PathBuf, level: u8) -> color_eyre::Result<()> {
    let source = BufReader::new(File::open(input).wrap_err("failed to open input file")?);
    let output = File::create(output).wrap_err("failed to open output file for writing")?;
    let compression_level: ruzstd::encoding::CompressionLevel = match level {
        0 => CompressionLevel::Uncompressed,
        1 => CompressionLevel::Fastest,
        2 => CompressionLevel::Default,
        3 => CompressionLevel::Better,
        4 => CompressionLevel::Best,
        _ => {
            unimplemented!("unsupported compression level: {}", level);
        }
    };

    ruzstd::encoding::compress(source, output, compression_level);
    Ok(())
}
