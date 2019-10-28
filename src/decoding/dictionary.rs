use crate::decoding::scratch::FSEScratch;
use crate::decoding::scratch::HuffmanScratch;

pub struct Dictionary {
    pub id: u32,
    pub fse: FSEScratch,
    pub huf: HuffmanScratch,
    pub dict_content: Vec<u8>,
    pub offset_hist: [u32; 3],
}

impl Dictionary {
    /// parses the dictionary and set the tables
    /// it returns the dict_id for checking with the frame's dict_id
    pub fn decode_dict(raw: &[u8]) -> Result<Dictionary, String> {
        let mut new_dict = Dictionary {
            id: 0,
            fse: FSEScratch::new(),
            huf: HuffmanScratch::new(),
            dict_content: Vec::new(),
            offset_hist: [2, 4, 8],
        };
        let magic_num = &raw[..4];
        //TODO check magic num
        let _ = magic_num;

        let dict_id = &raw[4..8];
        let dict_id = dict_id[0] as u32
            + ((dict_id[1] as u32) << 8)
            + ((dict_id[2] as u32) << 16)
            + ((dict_id[3] as u32) << 24);
        new_dict.id = dict_id;

        let raw_tables = &raw[8..];

        let huf_size = new_dict.huf.table.build_decoder(raw_tables)?;
        let raw_tables = &raw_tables[huf_size as usize..];

        let of_size = new_dict.fse.offsets.build_decoder(
            raw_tables,
            crate::decoding::sequence_section_decoder::OF_MAX_LOG,
        )?;
        let raw_tables = &raw_tables[of_size as usize..];

        let ml_size = new_dict.fse.match_lengths.build_decoder(
            raw_tables,
            crate::decoding::sequence_section_decoder::ML_MAX_LOG,
        )?;
        let raw_tables = &raw_tables[ml_size as usize..];

        let ll_size = new_dict.fse.literal_lengths.build_decoder(
            raw_tables,
            crate::decoding::sequence_section_decoder::LL_MAX_LOG,
        )?;
        let raw_tables = &raw_tables[ll_size as usize..];

        let offset1 = &raw_tables[0..4];
        let offset1 = (offset1[0] as u32)
            + ((offset1[1] as u32) << 8)
            + ((offset1[2] as u32) << 16)
            + ((offset1[3] as u32) << 24);

        let offset2 = &raw_tables[4..8];
        let offset2 = (offset2[0] as u32)
            + ((offset2[1] as u32) << 8)
            + ((offset2[2] as u32) << 16)
            + ((offset2[3] as u32) << 24);

        let offset3 = &raw_tables[8..12];
        let offset3 = (offset3[0] as u32)
            + ((offset3[1] as u32) << 8)
            + ((offset3[2] as u32) << 16)
            + ((offset3[3] as u32) << 24);

        new_dict.offset_hist[0] = offset1;
        new_dict.offset_hist[1] = offset2;
        new_dict.offset_hist[2] = offset3;

        let raw_content = &raw_tables[12..];
        new_dict.dict_content.extend(raw_content);

        Ok(new_dict)
    }
}
