#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    Raw,
    RLE,
    Compressed,
    Reserved,
}

impl std::fmt::Display for BlockType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
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
