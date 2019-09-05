use std::collections::HashMap;
use super::decodebuffer::Decodebuffer;
use super::offset_history::OffsetHist;

pub struct DecoderScratch {
   huf: HuffmanScratch,
   fse: FSEScratch,
   buffer: Decodebuffer,
   offset_hist: OffsetHist,
}

pub struct HuffmanScratch {
   pub decoding_map: HashMap<u32, u32>,
}

pub struct FSEScratch {
   pub offset_decoding_map: HashMap<u32, u32>,
   pub literal_length_decoding_map: HashMap<u32, u32>,
   pub match_length_decoding_map: HashMap<u32, u32>,
}