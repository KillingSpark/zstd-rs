use super::super::block::sequence_section::Sequence;
use super::decodebuffer::Decodebuffer;
use crate::fse::FSETable;
use crate::huff0::HuffmanTable;

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

    /// parses the dictionary and set the tables
    /// it returns the dict_id for checking with the frame's dict_id 
    pub fn load_dict(&mut self, raw: &[u8]) -> Result<u32, String> {
        let magic_num = &raw[..4];
        //TODO check magic num
        let _ = magic_num;

        let dict_id = &raw[4..8];
        let dict_id = dict_id[0] as u32 + ((dict_id[1]  as u32) << 8) + ((dict_id[2]  as u32) << 16) + ((dict_id[3]  as u32) << 24);

        let raw_tables = &raw[8..];

        let huf_size = self.huf.table.build_decoder(raw_tables)?;
        let raw_tables = &raw_tables[huf_size as usize..];
        
        let of_size = self.fse.offsets.build_decoder(raw_tables, crate::decoding::sequence_section_decoder::OF_MAX_LOG)?;
        let raw_tables = &raw_tables[of_size as usize..];
        
        let ml_size = self.fse.match_lengths.build_decoder(raw_tables, crate::decoding::sequence_section_decoder::ML_MAX_LOG)?;
        let raw_tables = &raw_tables[ml_size as usize..];
        
        let ll_size = self.fse.literal_lengths.build_decoder(raw_tables, crate::decoding::sequence_section_decoder::LL_MAX_LOG)?;
        let raw_tables = &raw_tables[ll_size as usize..];
        
        let offset1 = &raw_tables[0..4];
        let offset1 = offset1[0] as u32 + (offset1[1]  as u32) << 8 + (offset1[2]  as u32) << 16 + (offset1[3]  as u32) << 24;

        let offset2 = &raw_tables[4..8];
        let offset2 = offset2[0] as u32 + (offset2[1]  as u32) << 8 + (offset2[2]  as u32) << 16 + (offset2[3]  as u32) << 24;

        let offset3 = &raw_tables[8..12];
        let offset3 = offset3[0] as u32 + (offset3[1]  as u32) << 8 + (offset3[2]  as u32) << 16 + (offset3[3]  as u32) << 24;

        self.offset_hist[0] = offset1;
        self.offset_hist[1] = offset2;
        self.offset_hist[2] = offset3;

        let raw_content = &raw_tables[12..];
        self.buffer.dict_content.clear();
        self.buffer.dict_content.extend(raw_content);
        

        Ok(dict_id)
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