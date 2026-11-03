use alloc::vec::Vec;
use core::convert::TryInto;

use crate::decoding::errors::DictionaryDecodeError;
use crate::decoding::scratch::FSEScratch;
use crate::decoding::scratch::HuffmanScratch;

/// Zstandard includes support for "raw content" dictionaries, that store bytes optionally used
/// during sequence execution.
///
/// <https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#dictionary-format>
pub struct Dictionary {
    /// A 4 byte value used by decoders to check if they can use
    /// the correct dictionary. This value must not be zero.
    pub id: u32,
    /// A dictionary can contain an entropy table, either FSE or
    /// Huffman.
    pub fse: FSEScratch,
    /// A dictionary can contain an entropy table, either FSE or
    /// Huffman.
    pub huf: HuffmanScratch,
    /// The content of a dictionary acts as a "past" in front of data
    /// to compress or decompress,
    /// so it can be referenced in sequence commands.
    /// As long as the amount of data decoded from this frame is less than or
    /// equal to Window_Size, sequence commands may specify offsets longer than
    /// the total length of decoded output so far to reference back to the
    /// dictionary, even parts of the dictionary with offsets larger than Window_Size.
    /// After the total output has surpassed Window_Size however,
    /// this is no longer allowed and the dictionary is no longer accessible
    pub dict_content: Vec<u8>,
    /// The 3 most recent offsets are stored so that they can be used
    /// during sequence execution, see
    /// <https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#repeat-offsets>
    /// for more.
    pub offset_hist: [u32; 3],
}

/// This 4 byte (little endian) magic number refers to the start of a dictionary
pub const MAGIC_NUM: [u8; 4] = [0x37, 0xA4, 0x30, 0xEC];

impl Dictionary {
    /// Create a "raw content" dictionary that only consists of content and has no entropy tables.
    ///
    /// Raw content dictionaries have no ID of their own. Frames compressed with one do not carry a dictionary ID
    /// in their header, so a [`crate::decoding::FrameDecoder`] does not pick them up automatically. Use
    /// [`crate::decoding::FrameDecoder::force_dict`] with the `id` chosen here after initializing the decoder for
    /// a frame.
    pub fn from_raw_content(id: u32, content: Vec<u8>) -> Dictionary {
        Dictionary {
            id,
            fse: FSEScratch::new(),
            huf: HuffmanScratch::new(),
            dict_content: content,
            offset_hist: [1, 4, 8],
        }
    }

    /// Parses the dictionary from `raw` and set the tables
    /// it returns the dict_id for checking with the frame's `dict_id``
    pub fn decode_dict(raw: &[u8]) -> Result<Dictionary, DictionaryDecodeError> {
        if raw.len() < 8 {
            return Err(DictionaryDecodeError::NotEnoughBytes);
        }

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

        if raw_tables.len() < huf_size as usize {
            return Err(DictionaryDecodeError::NotEnoughBytes);
        }
        let raw_tables = &raw_tables[huf_size as usize..];

        let of_size = new_dict.fse.offsets.build_decoder(
            raw_tables,
            crate::decoding::sequence_section_decoder::OF_MAX_LOG,
        )?;

        if raw_tables.len() < of_size {
            return Err(DictionaryDecodeError::NotEnoughBytes);
        }
        let raw_tables = &raw_tables[of_size..];

        let ml_size = new_dict.fse.match_lengths.build_decoder(
            raw_tables,
            crate::decoding::sequence_section_decoder::ML_MAX_LOG,
        )?;

        if raw_tables.len() < ml_size {
            return Err(DictionaryDecodeError::NotEnoughBytes);
        }
        let raw_tables = &raw_tables[ml_size..];

        let ll_size = new_dict.fse.literal_lengths.build_decoder(
            raw_tables,
            crate::decoding::sequence_section_decoder::LL_MAX_LOG,
        )?;

        if raw_tables.len() < ll_size {
            return Err(DictionaryDecodeError::NotEnoughBytes);
        }
        let raw_tables = &raw_tables[ll_size..];

        if raw_tables.len() < 12 {
            return Err(DictionaryDecodeError::NotEnoughBytes);
        }
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

#[test]
fn truncated_dictionary() {
    use alloc::vec;

    // First case: Valid dictionary magic number, but missing the 4-byte dictionary ID.
    let raw = [0x37, 0xA4, 0x30, 0xEC];
    let _ = Dictionary::decode_dict(&raw);

    // Second case: Valid dictionary magic number, non-zero dictionary ID, table bytes known to parse successfully. But fewer than 12 bytes remain for the 3 offset-history u32 values..
    let mut raw = vec![0u8; 8];
    raw[0] = 0x37;
    raw[1] = 0xA4;
    raw[2] = 0x30;
    raw[3] = 0xEC;
    raw[4] = 0x01;
    raw[5] = 0x21;
    raw[6] = 0x23;
    raw[7] = 0x47;

    let raw_tables = [
        54, 16, 192, 155, 4, 0, 207, 59, 239, 121, 158, 116, 220, 93, 114, 229, 110, 41, 249, 95,
        165, 255, 83, 202, 254, 68, 74, 159, 63, 161, 100, 151, 137, 21, 184, 183, 189, 100, 235,
        209, 251, 174, 91, 75, 91, 185, 19, 39, 75, 146, 98, 177, 249, 14, 4, 35, 0, 0, 0, 40, 40,
        20, 10, 12, 204, 37, 196, 1, 173, 122, 0, 4, 0, 128, 1, 2, 2, 25, 32, 27, 27, 22, 24, 26,
        18, 12, 12, 15, 16, 11, 69, 37, 225, 48, 20, 12, 6, 2, 161, 80, 40, 20, 44, 137, 145, 204,
        46, 0, 0, 0, 0, 0, 116, 253, 16, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    raw.extend_from_slice(&raw_tables);

    // Fewer than 12 bytes remain for the 3 offset-history u32 values.
    raw.extend_from_slice(&[3, 0, 0, 0, 10, 0, 0, 0, 0xEF, 0xCD, 0xAB]);

    let _ = Dictionary::decode_dict(&raw);
}
