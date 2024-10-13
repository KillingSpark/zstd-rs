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
//! <https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#fse>
//!
//! <https://arxiv.org/pdf/1311.2540>

mod fse_decoder;

pub use fse_decoder::*;
pub mod fse_encoder;

#[test]
fn tables_equal() {
    let probs = &[0, 0, -1, 3, 2, 2, (1 << 6) - 8];
    let mut dec_table = FSETable::new(255);
    dec_table.build_from_probabilities(6, probs).unwrap();
    let enc_table = fse_encoder::build_table_from_probabilities(probs, 6);

    for (idx, dec_state) in dec_table.decode.iter().enumerate() {
        let enc_states = &enc_table.states[dec_state.symbol as usize];
        let enc_state = enc_states.states.iter().find(| state| state.index == idx).unwrap();
        assert_eq!(enc_state.baseline, dec_state.base_line as usize);
        assert_eq!(enc_state.num_bits, dec_state.num_bits);
    }
}
