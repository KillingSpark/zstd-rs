pub enum BlockType {
    Raw,
    RLE,
    Compressed,
    Reserved,
}

pub struct BlockHeader {
    pub last_block: bool,
    pub block_type: BlockType,
    pub decompressed_size: u32,
    pub content_size: u32,
}