use super::decodebuffer::Decodebuffer;
use super::fse::FSETable;
use super::huff0::HuffmanDecoder;
use super::offset_history::OffsetHist;

pub struct DecoderScratch {
   pub huf: HuffmanScratch,
   pub fse: FSEScratch,
   pub buffer: Decodebuffer,
   pub offset_hist: OffsetHist,

   pub literals_buffer: Vec<u8>,
   pub block_content_buffer: Vec<u8>,
}

impl DecoderScratch {
   pub fn new(window_size: usize) -> DecoderScratch {
      DecoderScratch {
         huf: HuffmanScratch {
            decoder: HuffmanDecoder::new(),
         },
         fse: FSEScratch {
            offsets: FSETable::new(),
            literal_lengths: FSETable::new(),
            match_lengths: FSETable::new(),
         },
         buffer: Decodebuffer::new(window_size),
         offset_hist: OffsetHist::new(),

         literals_buffer: Vec::new(),
         block_content_buffer: Vec::new(),
      }
   }
}

pub struct HuffmanScratch {
   pub decoder: HuffmanDecoder,
}

pub struct FSEScratch {
   pub offsets: FSETable,
   pub literal_lengths: FSETable,
   pub match_lengths: FSETable,
}
