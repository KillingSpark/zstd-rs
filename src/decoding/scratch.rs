use super::decodebuffer::Decodebuffer;
use crate::fse::FSETable;
use crate::huff0::HuffmanTable;
use super::super::block::sequence_section::Sequence;

pub struct DecoderScratch {
   pub huf: HuffmanScratch,
   pub fse: FSEScratch,
   pub buffer: Decodebuffer,
   pub offset_hist: [u32;3],

   pub literals_buffer: Vec<u8>,
   pub sequences: Vec<Sequence>,
   pub block_content_buffer: Vec<u8>,
}

impl DecoderScratch {
   pub fn new(window_size: usize) -> DecoderScratch {
      DecoderScratch {
         huf: HuffmanScratch {
            table: HuffmanTable::new(),
         },
         fse: FSEScratch {
            offsets: FSETable::new(),
            of_rle: None,
            literal_lengths: FSETable::new(),
            ll_rle: None,
            match_lengths: FSETable::new(),
            ml_rle: None,
         },
         buffer: Decodebuffer::new(window_size),
         offset_hist: [1,4,8],

         block_content_buffer: Vec::new(),
         literals_buffer: Vec::new(),
         sequences: Vec::new(),
      }
   }
}

pub struct HuffmanScratch {
   pub table: HuffmanTable,
}

pub struct FSEScratch {
   pub offsets: FSETable,
   pub of_rle: Option<u8>,
   pub literal_lengths: FSETable,
   pub ll_rle: Option<u8>,
   pub match_lengths: FSETable,
   pub ml_rle: Option<u8>,
}
