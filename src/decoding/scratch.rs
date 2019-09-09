use super::decodebuffer::Decodebuffer;
use super::offset_history::OffsetHist;
use super::fse::FSETable;
use super::huff0::HuffmanDecoder;

pub struct DecoderScratch {
   huf: HuffmanScratch,
   fse: FSEScratch,
   buffer: Decodebuffer,
   offset_hist: OffsetHist,
}

pub struct HuffmanScratch {
   pub decoder: HuffmanDecoder,
}

pub struct FSEScratch {
   pub offset_decoder: FSETable,
   pub literal_length_decoder: FSETable,
   pub match_length_decoder: FSETable,
}