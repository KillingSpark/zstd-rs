use super::scratch::HuffmanScratch;
use std::collections::HashMap;
use std::io::Read;

pub struct HuffmanDecoder {
    decode: HashMap<u8, u8>,
}

impl HuffmanDecoder {
    pub fn build_decoder(&mut self, source: &mut Read, scratch: &mut HuffmanScratch) -> Result<(), String> {
        scratch.decoding_map.clear();
        self.decode.clear();

        //TODO build huffman table from the source stream
        let _ = source;
        Ok(())
    }
}
