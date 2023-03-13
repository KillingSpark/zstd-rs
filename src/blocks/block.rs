#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    Raw,
    RLE,
    Compressed,
    Reserved,
}

impl core::fmt::Display for BlockType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        match self {
            BlockType::Compressed => write!(f, "Compressed"),
            BlockType::Raw => write!(f, "Raw"),
            BlockType::RLE => write!(f, "RLE"),
            BlockType::Reserved => write!(f, "Reserverd"),
        }
    }
}

pub struct BlockHeader {
    pub last_block: bool,
    pub block_type: BlockType,
    pub decompressed_size: u32,
    pub content_size: u32,
}
