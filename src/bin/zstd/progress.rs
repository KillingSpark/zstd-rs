//! Utilities for displaying a progress monitor to track compression/decompression/whatever else
//! 
//! This implementation relies heavily on the `indicatif` crate, see <https://docs.rs/indicatif>

use std::io::Read;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use tracing::info;

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
    pub fn new(reader: R, size: usize) -> Self {
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