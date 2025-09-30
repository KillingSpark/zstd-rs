extern crate ruzstd;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use color_eyre::eyre::{ContextCompat, WrapErr};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use ruzstd::encoding::CompressionLevel;
use tracing::info;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

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
    Decompress {
        /// .zst archive to decompress
        input_file: PathBuf,
        /// Where the compressed file is written
        /// [default: <ARCHIVE_NAME>]
        output_file: Option<PathBuf>,
    },
    GenDict {},
    Bench {},
}

fn main() -> color_eyre::Result<()> {
    // Process CLI arguments
    let cli = Cli::parse();
    // Initialize logging (with indicatif integration)
    let indicatif_layer = IndicatifLayer::new();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(indicatif_layer.get_stderr_writer())
                .without_time(),
        )
        .with(indicatif_layer)
        .init();

    let command: Commands = cli.command.wrap_err("no subcommand provided").unwrap();
    match command {
        Commands::Compress {
            input_file,
            output_file,
            level,
        } => {
            let output_file = output_file.unwrap_or_else(|| add_extension(&input_file, ".zst"));
            compress(input_file, output_file, level)?;
        }
        Commands::Decompress {
            input_file,
            output_file,
        } => {
            let output_file = output_file.unwrap_or(
                input_file
                    .file_stem()
                    .expect("input has a file name")
                    .into(),
            );
            decompress(input_file, output_file)?;
        }
        _ => unimplemented!(),
    }
    Ok(())
}

/// A generic wrapper around a reader that keeps track of how many bytes have been read
/// from the total.
///
/// This wrapper has a lock on standard out for the lifetime of the monitor
pub struct ProgressMonitor<R: Read> {
    /// The total amount that the reader will read
    pub total: usize,
    /// Amount read so far
    pub read: usize,
    /// The internal reader
    reader: R,
    progress_bar: ProgressBar,
}

impl<R: Read> ProgressMonitor<R> {
    /// Create a new progress monitor, initialized with zero bytes read
    fn new(reader: R, size: usize) -> Self {
        // https://docs.rs/indicatif/latest/indicatif/index.html#templates
        let style = ProgressStyle::with_template(
            "{wide_bar} {binary_bytes}/{binary_total_bytes}  \n[est. {eta} remaining]",
        )
        .unwrap();
        let progress_bar = ProgressBar::new(size as u64).with_style(style);
        // The default is 20hz, this reduces rendering overhead
        progress_bar.set_draw_target(ProgressDrawTarget::stderr_with_hz(8));
        Self {
            reader,
            total: size,
            read: 0,
            progress_bar,
        }
    }

    /// This function is called whenever a new read is made, and is responsible for updating the UI
    fn update(&mut self, delta: u64) {
        self.progress_bar.inc(delta);
        if self.total == self.read && !self.progress_bar.is_finished() {
            self.progress_bar.finish_and_clear();
            info!(
                "processed {} in {} ({}/s avg)",
                fmt_size(self.total),
                indicatif::HumanDuration(self.progress_bar.elapsed()),
                indicatif::HumanBytes(self.total as u64 / self.progress_bar.elapsed().as_secs())
            );
        }
    }
}

impl<R: Read> Read for ProgressMonitor<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Fall back on the internally stored reader, but filch the number of bytes read
        // along the way
        let out = self.reader.read(buf)?;
        self.read += out;
        self.update(out as u64);
        Ok(out)
    }
}

fn compress(input: PathBuf, output: PathBuf, level: u8) -> color_eyre::Result<()> {
    info!("compressing {input:?} to {output:?}");
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
    let source_file = File::open(input).wrap_err("failed to open input file")?;
    let source_size = source_file.metadata()?.size() as usize;
    let buffered_source = BufReader::new(source_file);
    let encoder_input = ProgressMonitor::new(buffered_source, source_size);
    let output: File = File::create(output).wrap_err("failed to open output file for writing")?;

    ruzstd::encoding::compress(encoder_input, &output, compression_level);
    let compressed_size = output.metadata()?.len();
    let compression_ratio = source_size as f64 / compressed_size as f64;
    info!(
        "{} ——> {} ({compression_ratio:.2}x)",
        fmt_size(source_size),
        fmt_size(compressed_size as usize)
    );
    Ok(())
}

fn decompress(input: PathBuf, output: PathBuf) -> color_eyre::Result<()> {
    info!("extracting {input:?} to {output:?}");
    let source_file = File::open(input).wrap_err("failed to open input file")?;
    let source_size = source_file.metadata()?.size() as usize;
    let buffered_source = BufReader::new(source_file);
    let decoder_input = ProgressMonitor::new(buffered_source, source_size);
    let mut output: File =
        File::create(output).wrap_err("failed to open output file for writing")?;

    let mut decoder = ruzstd::decoding::StreamingDecoder::new(decoder_input)?;

    std::io::copy(&mut decoder, &mut output)?;

    info!(
        "inflated {} ——> {}",
        fmt_size(source_size),
        fmt_size(output.metadata()?.len() as usize),
    );
    Ok(())
}

/// Converts a quantity in bytes to a human readable size, "GiB, MiB, KiB, etc"
fn fmt_size(size_in_bytes: usize) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let order_of_magnitude = (size_in_bytes as f64).log10() as usize;
    // Overflow to the next order of magnitude if there are more than `upper_bound` figures
    // before the decimal
    let upper_bound = 3;
    let unit_index = (order_of_magnitude / upper_bound).clamp(0, units.len() - 1);
    let size_in_bytes = size_in_bytes as f64;
    let decimal = size_in_bytes / 2_f64.powi((unit_index * 10) as i32);
    // Only use a decimal if division takes place
    if unit_index > 0 {
        format!("{:.2}{}", decimal, units[unit_index])
    } else {
        format!("{:.0}{}", decimal, units[unit_index])
    }
}

/// A temporary utility function that appends a file extension
/// to the provided path buf.
///
/// Pending removal when our MSRV reaches 1.91 so we can use
///
/// <https://doc.rust-lang.org/std/path/struct.PathBuf.html#method.add_extension>
fn add_extension<P: AsRef<Path>>(path: &Path, extension: P) -> PathBuf {
    let mut output = path.to_path_buf().into_os_string();
    output.push(extension.as_ref().as_os_str());

    output.into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{add_extension, fmt_size};

    #[test]
    fn extension_added() {
        let filename = PathBuf::from("README.md");
        assert_eq!(
            add_extension(&filename, ".zst"),
            PathBuf::from("README.md.zst")
        );
    }

    #[test]
    fn human_readable_filesize() {
        // Bytes
        assert_eq!(&fmt_size(100), "100B");
        // Kibibytes
        assert_eq!(&fmt_size(12 * 2_usize.pow(10)), "12.00KiB");
        // Mebibytes
        assert_eq!(&fmt_size(7 * 2_usize.pow(20)), "7.00MiB");
        // Gibibytes
        assert_eq!(&fmt_size(123 * 2_usize.pow(30)), "123.00GiB");
    }
}
