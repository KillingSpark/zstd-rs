use std::collections::HashMap;


pub struct DecoderScratch {
    huf: HuffmanScratch,
    fse: FSEScratch,
}

pub struct HuffmanScratch {
   decoding_map: HashMap<u32, u32>
}

pub struct FSEScratch {
   decoding_map: HashMap<u32, u32>
}