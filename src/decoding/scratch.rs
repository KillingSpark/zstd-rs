use super::super::blocks::sequence_section::Sequence;
use super::decodebuffer::Decodebuffer;
use crate::decoding::dictionary::Dictionary;
use crate::fse::FSETable;
use crate::huff0::HuffmanTable;
use alloc::vec::Vec;

pub struct DecoderScratch {
    pub huf: HuffmanScratch,
    pub fse: FSEScratch,
    pub buffer: Decodebuffer,
    pub offset_hist: [u32; 3],

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
            offset_hist: [1, 4, 8],

            block_content_buffer: Vec::new(),
            literals_buffer: Vec::new(),
            sequences: Vec::new(),
        }
    }

    pub fn reset(&mut self, window_size: usize) {
        self.offset_hist = [1, 4, 8];
        self.literals_buffer.clear();
        self.sequences.clear();
        self.block_content_buffer.clear();

        self.buffer.reset(window_size);

        self.fse.literal_lengths.reset();
        self.fse.match_lengths.reset();
        self.fse.offsets.reset();
        self.fse.ll_rle = None;
        self.fse.ml_rle = None;
        self.fse.of_rle = None;

        self.huf.table.reset();
    }

    pub fn init_from_dict(&mut self, dict: &Dictionary) {
        self.fse.reinit_from(&dict.fse);
        self.huf.table.reinit_from(&dict.huf.table);
        self.offset_hist = dict.offset_hist;
        self.buffer.dict_content.clear();
        self.buffer
            .dict_content
            .extend_from_slice(&dict.dict_content);
    }
}

pub struct HuffmanScratch {
    pub table: HuffmanTable,
}

impl HuffmanScratch {
    pub fn new() -> HuffmanScratch {
        HuffmanScratch {
            table: HuffmanTable::new(),
        }
    }
}

impl Default for HuffmanScratch {
    fn default() -> Self {
        Self::new()
    }
}

pub struct FSEScratch {
    pub offsets: FSETable,
    pub of_rle: Option<u8>,
    pub literal_lengths: FSETable,
    pub ll_rle: Option<u8>,
    pub match_lengths: FSETable,
    pub ml_rle: Option<u8>,
}

impl FSEScratch {
    pub fn new() -> FSEScratch {
        FSEScratch {
            offsets: FSETable::new(),
            of_rle: None,
            literal_lengths: FSETable::new(),
            ll_rle: None,
            match_lengths: FSETable::new(),
            ml_rle: None,
        }
    }

    pub fn reinit_from(&mut self, other: &Self) {
        self.offsets.reinit_from(&other.offsets);
        self.literal_lengths.reinit_from(&other.literal_lengths);
        self.match_lengths.reinit_from(&other.match_lengths);
        self.of_rle = other.of_rle;
        self.ll_rle = other.ll_rle;
        self.ml_rle = other.ml_rle;
    }
}

impl Default for FSEScratch {
    fn default() -> Self {
        Self::new()
    }
}
