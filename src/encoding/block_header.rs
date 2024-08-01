use super::bit_writer::BitWriter;
use crate::blocks::block::BlockType;
use std::vec::Vec;

// /// The type of a single Zstandard block
// ///
// /// https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#blocks
// pub enum BlockType {
//     /// This is an uncompressed block.
//     Raw,
//     /// This is a single byte, repeated `block_size`
//     /// times.
//     RLE,
//     /// This is a Zstandard compressed block.
//     Compressed,
//     /// This is not a block, and this value
//     /// cannot be used in the current version of the spec.
//     Reserved,
// }

pub struct BlockHeader {
    /// Signals if this block is the last one.
    /// The frame will end after this block.
    last_block: bool,
    /// Influences the meaning of `block_size`.
    block_type: BlockType,
    /// - For `Raw` blocks, this is the size of the block's
    /// content in bytes.
    /// - For `RLE` blocks, there will be a single byte follwing
    /// the header, repeated `block_size` times.
    /// - For `Compressed` blocks, this is the length of
    /// the compressed data.
    ///
    /// **This value must not be greater than 21 bits in length.**
    block_size: u32,
}

#[derive(Debug)]
pub enum BlockHeaderError {
    AboveMaxBlockSize,
}

impl BlockHeader {
    /// Returns the encoded binary representation of this header.
    pub fn serialize(self) -> Result<Vec<u8>, BlockHeaderError> {
        let mut bw = BitWriter::new();
        // A block header uses 3 bytes,
        // with the first bit representing `last_block`,
        // the next two representing `block_type`, and the
        // last 21 bits representing `block_size`
        if self.block_size >> 21 != 0 {
            return Err(BlockHeaderError::AboveMaxBlockSize);
        }
        bw.write_bits(&[self.last_block as u8], 1);
        let encoded_block_type = match self.block_type {
            BlockType::Raw => 0,
            BlockType::RLE => 1,
            BlockType::Compressed => 2,
            BlockType::Reserved => panic!("You cannot use a reserved block type"),
        };
        bw.write_bits(&[encoded_block_type], 2);
        bw.write_bits(&self.block_size.to_le_bytes(), 21);
        Ok(bw.dump().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::BlockHeader;
    use crate::{blocks::block::BlockType, decoding::block_decoder};
    use std::string::String;
    use std::{format, println};

    #[test]
    fn block_header_serialize() {
        let header = BlockHeader {
            last_block: true,
            block_type: super::BlockType::Compressed,
            block_size: 0,
        };
        let serialized_header = header.serialize().unwrap();
        let mut decoder = block_decoder::new();
        let parsed_header = decoder
            .read_block_header(serialized_header.as_slice())
            .unwrap()
            .0;
        let mut display_str = String::new();
        for byte in serialized_header {
            display_str += &format!(" {byte:08b}");
        }

        println!("BE: {display_str}");
        println!(
            "LE: {}",
            display_str
                .split(" ")
                .collect::<std::vec::Vec<&str>>()
                .into_iter()
                .rev()
                .collect::<std::vec::Vec<&str>>()
                .join(" ")
        );
        assert!(parsed_header.last_block);
        assert_eq!(parsed_header.block_type, BlockType::Compressed);
        assert_eq!(parsed_header.content_size, 69);
    }
}
