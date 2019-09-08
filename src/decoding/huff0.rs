use std::collections::HashMap;

pub struct HuffmanDecoder {
    decode: HashMap<u8, u8>,
}

impl HuffmanDecoder {
    pub fn build_decoder(&mut self, source: &[u8]) -> Result<u32, String> {
        self.decode.clear();

        //TODO build huffman table from the source stream
        let _ = source;
        Ok(100)
    }
}
