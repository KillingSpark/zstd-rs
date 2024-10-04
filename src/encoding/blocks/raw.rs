use std::vec::Vec;

/// Write the data from input into output. The data is not compressed.
pub(crate) fn compress_raw_block(input: &[u8], output: &mut Vec<u8>) {
    output.extend_from_slice(input);
}

#[cfg(test)]
mod tests {
    use super::compress_raw_block;
    use std::{vec, vec::Vec};
    #[test]
    fn raw_block_compressed() {
        let mut output: Vec<u8> = Vec::new();
        compress_raw_block(&[1, 2, 3], &mut output);
        assert_eq!(vec![1_u8, 2, 3], output);
    }
}
