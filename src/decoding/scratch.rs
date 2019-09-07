use super::decodebuffer::Decodebuffer;
use super::offset_history::OffsetHist;
use super::fse::FSEDecoder;
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
   pub offset_decoder: FSEDecoder,
   pub literal_length_decoder: FSEDecoder,
   pub match_length_decoder: FSEDecoder,
}