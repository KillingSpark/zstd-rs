/// Huffman coding is a method of encoding where symbols are assigned a code,
/// and more commonly used symbols get shorter codes, and less commonly
/// used symbols get longer codes. Codes are prefix free, meaning no two codes
/// will start with the same sequence of bits.
mod huff0_decoder;
pub use huff0_decoder::*;
