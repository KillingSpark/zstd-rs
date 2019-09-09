use super::decodebuffer::Decodebuffer;
use super::offset_history::OffsetHist;
use super::fse::FSETable;
use super::huff0::HuffmanDecoder;

pub struct DecoderScratch {
   pub huf: HuffmanScratch,
   pub fse: FSEScratch,
   pub buffer: Decodebuffer,
   pub offset_hist: OffsetHist,

   pub literals_buffer: Vec<u8>,
   pub block_content_buffer: Vec<u8>,
}

pub struct HuffmanScratch {
   pub decoder: HuffmanDecoder,
}

pub struct FSEScratch {
   pub offset_decoder: FSETable,
   pub literal_length_decoder: FSETable,
   pub match_length_decoder: FSETable,
}