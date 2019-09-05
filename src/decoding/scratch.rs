use std::collections::HashMap;
use super::decodebuffer::Decodebuffer;

pub struct DecoderScratch {
   huf: HuffmanScratch,
   fse: FSEScratch,
   buffer: Decodebuffer,
}

pub struct HuffmanScratch {
   pub decoding_map: HashMap<u32, u32>
}

pub struct FSEScratch {
   pub decoding_map: HashMap<u32, u32>
}