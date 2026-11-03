//! Per-level block compression logic
mod matched;
#[cfg(test)]
mod tests;
pub use matched::compress_matched_block;
