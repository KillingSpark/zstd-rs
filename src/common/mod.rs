//! Values and interfaces shared between the encoding side
//! and the decoding side.

// --- FRAMES ---
/// This magic number is included at the start of a single Zstandard frame
pub const MAGIC_NUM: u32 = 0xFD2F_B528;
/// Window size refers to the minimum amount of memory needed to decode any given frame.
///
/// The minimum window size is defined as 1 KB
pub const MIN_WINDOW_SIZE: u64 = 1024;
/// Window size refers to the minimum amount of memory needed to decode any given frame.
///
/// The maximum window size is 3.75TB
pub const MAX_WINDOW_SIZE: u64 = (1 << 41) + 7 * (1 << 38);

// --- BLOCKS ---
/// Blocks cannot be larger than 128KB in size.
pub const MAX_BLOCK_SIZE: u32 = 128 * 1024;
