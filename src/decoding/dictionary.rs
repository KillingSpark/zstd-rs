use alloc::vec::Vec;
use core::convert::TryInto;

use crate::decoding::scratch::FSEScratch;
use crate::decoding::scratch::HuffmanScratch;
use crate::fse::FSETableError;
use crate::huff0::HuffmanTableError;

pub struct Dictionary {
    pub id: u32,
    pub fse: FSEScratch,
    pub huf: HuffmanScratch,
    pub dict_content: Vec<u8>,
    pub offset_hist: [u32; 3],
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DictionaryDecodeError {
    #[error(
        "Bad magic_num at start of the dictionary; Got: {got:#04X?}, Expected: {MAGIC_NUM:#04x?}"
    )]
    BadMagicNum { got: [u8; 4] },
    #[error(transparent)]
    FSETableError(#[from] FSETableError),
    #[error(transparent)]
    HuffmanTableError(#[from] HuffmanTableError),
}

pub const MAGIC_NUM: [u8; 4] = [0x37, 0xA4, 0x30, 0xEC];

impl Dictionary {
    /// parses the dictionary and set the tables
    /// it returns the dict_id for checking with the frame's dict_id
    pub fn decode_dict(raw: &[u8]) -> Result<Dictionary, DictionaryDecodeError> {
        let mut new_dict = Dictionary {
            id: 0,
            fse: FSEScratch::new(),
            huf: HuffmanScratch::new(),
            dict_content: Vec::new(),
            offset_hist: [2, 4, 8],
        };

        let magic_num: [u8; 4] = raw[..4].try_into().expect("optimized away");
        if magic_num != MAGIC_NUM {
            return Err(DictionaryDecodeError::BadMagicNum { got: magic_num });
        }

        let dict_id = raw[4..8].try_into().expect("optimized away");
        let dict_id = u32::from_le_bytes(dict_id);
        new_dict.id = dict_id;

        let raw_tables = &raw[8..];

        let huf_size = new_dict.huf.table.build_decoder(raw_tables)?;
        let raw_tables = &raw_tables[huf_size as usize..];

        let of_size = new_dict.fse.offsets.build_decoder(
            raw_tables,
            crate::decoding::sequence_section_decoder::OF_MAX_LOG,
        )?;
        let raw_tables = &raw_tables[of_size..];

        let ml_size = new_dict.fse.match_lengths.build_decoder(
            raw_tables,
            crate::decoding::sequence_section_decoder::ML_MAX_LOG,
        )?;
        let raw_tables = &raw_tables[ml_size..];

        let ll_size = new_dict.fse.literal_lengths.build_decoder(
            raw_tables,
            crate::decoding::sequence_section_decoder::LL_MAX_LOG,
        )?;
        let raw_tables = &raw_tables[ll_size..];

        let offset1 = raw_tables[0..4].try_into().expect("optimized away");
        let offset1 = u32::from_le_bytes(offset1);

        let offset2 = raw_tables[4..8].try_into().expect("optimized away");
        let offset2 = u32::from_le_bytes(offset2);

        let offset3 = raw_tables[8..12].try_into().expect("optimized away");
        let offset3 = u32::from_le_bytes(offset3);

        new_dict.offset_hist[0] = offset1;
        new_dict.offset_hist[1] = offset2;
        new_dict.offset_hist[2] = offset3;

        let raw_content = &raw_tables[12..];
        new_dict.dict_content.extend(raw_content);

        Ok(new_dict)
    }
}
