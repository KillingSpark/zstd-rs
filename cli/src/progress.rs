// ! Utilities for displaying a progress monitor to track compression/decompression/whatever else
//!
//! This implementation relies heavily on the `indicatif` crate, see <https://docs.rs/indicatif>cargo hack check --feature-powerset --exclude-features rustc-dep-of-std

use std::{fmt::Write, io::Read, time::Duration};

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
                fmt_size(self.total as f64),
                fmt_duration(self.progress_bar.elapsed()),
                fmt_size(self.total as f64 / self.progress_bar.elapsed().as_secs_f64())
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

/// Converts a quantity in bytes to a human readable size, "GiB, MiB, KiB, etc"
pub fn fmt_size(size_in_bytes: f64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let order_of_magnitude = (size_in_bytes).log10() as usize;
    // Overflow to the next order of magnitude if there are more than `upper_bound` figures
    // before the decimal
    let upper_bound = 3;
    let unit_index = (order_of_magnitude / upper_bound).clamp(0, units.len() - 1);
    let decimal = size_in_bytes / 2_f64.powi((unit_index * 10) as i32);
    // Only use a decimal if displaying a unit larger than a byte
    if unit_index > 0 {
        format!("{:.2}{}", decimal, units[unit_index])
    } else {
        format!("{:.0}{}", decimal, units[unit_index])
    }
}

/// Converts a [`std::time::Duration`] to a human readable format
fn fmt_duration(duration: Duration) -> String {
    let as_secs = duration.as_secs_f64();
    let as_min = (as_secs / 60.0).floor() as usize;
    // When displayed in long form, the value shown
    let secs_portion: f64 = as_secs % 60.0;
    let min_portion: usize = ((as_secs - secs_portion) as usize / 60) % 60;
    let hr_portion: usize = ((as_min - min_portion) / 60) % 60;

    let mut output = String::with_capacity(8);
    if hr_portion > 0 {
        write!(&mut output, "{hr_portion}h ").unwrap();
    }
    if min_portion > 0 {
        write!(&mut output, "{min_portion}m ").unwrap();
    }
    // Formatting for seconds is fairly manual
    // to provide a "useful" level of precision
    if as_secs > 60.0 && secs_portion != 0.0 {
        // Zero points of precision
        write!(&mut output, "{:.0}s", secs_portion.round()).unwrap();
    } else if secs_portion > 4.0 {
        // One point of precision
        write!(&mut output, "{secs_portion:.1}s").unwrap();
    } else if secs_portion > 1.0 {
        // Two points of precision
        write!(&mut output, "{secs_portion:.2}s").unwrap();
    } else if secs_portion > 0.0 {
        // Display as ms with two units of precision
        write!(&mut output, "{:.2}ms", secs_portion * 1000.0).unwrap();
    }
    output.trim().to_string()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{fmt_duration, fmt_size};

    #[test]
    fn human_readable_filesize() {
        // Bytes
        assert_eq!(&fmt_size(100.0), "100B");
        // Kibibytes
        assert_eq!(&fmt_size(12.0 * 2.0_f64.powi(10)), "12.00KiB");
        // Mebibytes
        assert_eq!(&fmt_size(7.0 * 2.0_f64.powi(20)), "7.00MiB");
        // Gibibytes
        assert_eq!(&fmt_size(123.0 * 2.0_f64.powi(30)), "123.00GiB");
    }

    #[test]
    fn human_readable_duration() {
        assert_eq!(&fmt_duration(Duration::from_millis(7)), "7.00ms");
        assert_eq!(&fmt_duration(Duration::from_millis(1500)), "1.50s");
        assert_eq!(&fmt_duration(Duration::from_secs(30)), "30.0s");
        assert_eq!(&fmt_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(&fmt_duration(Duration::from_secs(5 * 60)), "5m");
        assert_eq!(&fmt_duration(Duration::from_secs(3 * 60 * 60)), "3h");
        assert_eq!(
            &fmt_duration(Duration::from_secs(1 * 60 * 60 + 20 * 60 + 30)),
            "1h 20m 30s"
        );
    }
}
