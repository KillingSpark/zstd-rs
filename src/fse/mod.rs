//! FSE, short for Finite State Entropy, is an encoding technique
//! that assigns shorter codes to symbols that appear more frequently in data,
//! and longer codes to less frequent symbols.
//!
//! FSE works by mutating a state and using that state to index into a table.
//!
//! Zstandard uses two different kinds of entropy encoding: FSE, and Huffman coding.
//! Huffman is used to compress literals,
//! while FSE is used for all other symbols (literal length code, match length code, offset code).
//!
//! https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#fse
//!
//! <https://arxiv.org/pdf/1311.2540>

mod fse_decoder;
pub use fse_decoder::*;
