use super::scratch::FSEScratch;
use std::io::Read;
use std::collections::HashMap;

pub struct FSEDecoder {
    decode: HashMap<u8, u8>,
}

impl FSEDecoder {
    pub fn build_decoder(&mut self, source: &mut Read, scratch: &mut FSEScratch) -> Result<(), String> {
        scratch.decoding_map.clear();
        self.decode.clear();
        
        //TODO build FSE table from the source stream
        let _ = source;
        Ok(())
    }
}
