use std::collections::HashMap;
use std::io::Read;

pub struct HuffmanDecoder {
    decode: HashMap<u8, u8>,
}

impl HuffmanDecoder {
    pub fn build_decoder(&mut self, source: &mut Read) -> Result<(), String> {
        self.decode.clear();

        //TODO build huffman table from the source stream
        let _ = source;
        Ok(())
    }
}
