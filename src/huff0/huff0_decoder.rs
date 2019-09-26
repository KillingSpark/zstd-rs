use crate::decoding::bit_reader_reverse::BitReaderReversed;
use crate::fse::FSEDecoder;
use crate::fse::FSETable;

pub struct HuffmanTable {
    decode: Vec<Entry>,

    weights: Vec<u8>,
    pub max_num_bits: u8,
    bits: Vec<u8>,
    bit_ranks: Vec<u32>,
    rank_indexes: Vec<usize>,

    fse_table: FSETable,
}

pub struct HuffmanDecoder<'table> {
    table: &'table HuffmanTable,
    pub state: u64,
}

#[derive(Copy, Clone)]
pub struct Entry {
    symbol: u8,
    num_bits: u8,
}

const MAX_MAX_NUM_BITS: u8 = 11;

const fn num_bits<T>() -> usize {
    std::mem::size_of::<T>() * 8
}

fn highest_bit_set(x: u32) -> u32 {
    assert!(x > 0);
    num_bits::<u32>() as u32 - x.leading_zeros()
}

impl<'t> HuffmanDecoder<'t> {
    pub fn new(table: &'t HuffmanTable) -> HuffmanDecoder<'t> {
        HuffmanDecoder {
            table: table,
            state: 0,
        }
    }

    pub fn reset(mut self, new_table: Option<&'t HuffmanTable>) {
        self.state = 0;
        if new_table.is_some() {
            self.table = new_table.unwrap();
        }
    }

    pub fn decode_symbol(&mut self) -> u8 {
        self.table.decode[self.state as usize].symbol
    }

    pub fn init_state(&mut self, br: &mut BitReaderReversed) -> Result<u8, String> {
        let num_bits = self.table.max_num_bits;
        let new_bits = br.get_bits(num_bits as usize)?;
        self.state = new_bits as u64;
        Ok(num_bits)
    }

    pub fn next_state(&mut self, br: &mut BitReaderReversed) -> Result<u8, String> {
        let num_bits = self.table.decode[self.state as usize].num_bits;
        let new_bits = br.get_bits(num_bits as usize)?;
        self.state <<= num_bits;
        self.state &= self.table.decode.len() as u64 - 1;
        self.state |= new_bits as u64;
        Ok(num_bits)
    }
}

impl HuffmanTable {
    pub fn new() -> HuffmanTable {
        HuffmanTable {
            decode: Vec::new(),

            weights: Vec::with_capacity(256),
            max_num_bits: 0,
            bits: Vec::with_capacity(256),
            bit_ranks: Vec::with_capacity(11),
            rank_indexes: Vec::with_capacity(11),
            fse_table: FSETable::new(),
        }
    }

    pub fn reset(&mut self) {
        self.decode.clear();
        self.weights.clear();
        self.max_num_bits = 0;
        self.bits.clear();
        self.bit_ranks.clear();
        self.rank_indexes.clear();
        self.fse_table.reset();
    }

    pub fn build_decoder(&mut self, source: &[u8]) -> Result<u32, String> {
        self.decode.clear();

        let bytes_used = self.read_weights(source)?;
        self.build_table_from_weights()?;
        Ok(bytes_used)
    }

    fn read_weights(&mut self, source: &[u8]) -> Result<u32, String> {
        if source.len() < 1 {
            return Err("Source needs to have at least one byte".to_owned());
        }
        let header = source[0];
        let mut bits_read = 8;

        match header {
            0...127 => {
                let fse_stream = &source[1..];
                if header as usize > fse_stream.len() {
                    return Err(format!("Header says there should be {} bytes for the weights but there are only {} bytes in the stream", header, fse_stream.len()));
                }
                //fse decompress weights
                let bytes_used_by_fse_header = self
                    .fse_table
                    .build_decoder(fse_stream, /*TODO find actual max*/ 100)?;

                if bytes_used_by_fse_header > header as usize {
                    return Err(format!("FSE table used more bytes: {} than were meant to be used for the whole stream of huffman weights", bytes_used_by_fse_header));
                }

                if crate::VERBOSE {
                    println!(
                        "Building fse table for huffman weights used: {}",
                        bytes_used_by_fse_header
                    );
                }
                let mut dec1 = FSEDecoder::new(&self.fse_table);
                let mut dec2 = FSEDecoder::new(&self.fse_table);

                let compressed_start = bytes_used_by_fse_header as usize;
                let compressed_length = header as usize - bytes_used_by_fse_header as usize;

                let compressed_weights = &fse_stream[compressed_start..];
                if compressed_weights.len() < compressed_length {
                    return Err(format!(
                        "Not enough bytes in stream to decompress weights. Is: {}, Should be: {}",
                        compressed_weights.len(),
                        compressed_length
                    ));
                }
                let compressed_weights = &compressed_weights[..compressed_length];
                let mut br = BitReaderReversed::new(compressed_weights);

                bits_read += (bytes_used_by_fse_header + compressed_length) * 8;

                //skip the 0 padding at the end of the last byte of the bit stream and throw away the first 1 found
                let mut skipped_bits = 0;
                loop {
                    let val = br.get_bits(1)?;
                    skipped_bits += 1;
                    if val == 1 || skipped_bits > 8 {
                        break;
                    }
                }
                if skipped_bits > 8 {
                    //if more than 7 bits are 0, this is not the correct end of the bitstream. Either a bug or corrupted data
                    return Err(format!("Padding at the end of the sequence_section was more than a byte long: {}. Probably cause by data corruption", skipped_bits));
                }

                dec1.init_state(&mut br)?;
                dec2.init_state(&mut br)?;

                self.weights.clear();

                loop {
                    let w = dec1.decode_symbol();
                    self.weights.push(w);
                    dec1.update_state(&mut br)?;

                    if br.bits_remaining() <= -1 {
                        //collect final states
                        self.weights.push(dec2.decode_symbol());
                        break;
                    }

                    let w = dec2.decode_symbol();
                    self.weights.push(w);
                    dec2.update_state(&mut br)?;

                    if br.bits_remaining() <= -1 {
                        //collect final states
                        self.weights.push(dec1.decode_symbol());
                        break;
                    }
                    //maximum number of weights is 255 because we use u8 symbols and the last weight is infered from the sum of all others
                    if self.weights.len() > 255 {
                        return Err(
                            "More than 255 weights decoded. Stream is probably corrupted"
                                .to_owned(),
                        );
                    }
                }
            }
            _ => {
                // weights are directly encoded
                let weights_raw = &source[1..];
                let num_weights = header - 127;
                self.weights.resize(num_weights as usize, 0);

                let bytes_needed = if num_weights % 2 == 0 {
                    (num_weights as usize / 2)
                } else {
                    (num_weights as usize / 2) + 1
                };

                if weights_raw.len() < bytes_needed {
                    return Err(format!(
                        "Source needs to have at least {} bytes",
                        bytes_needed
                    ));
                }

                for idx in 0..num_weights {
                    if idx % 2 == 0 {
                        self.weights[idx as usize] = weights_raw[idx as usize / 2] >> 4;
                    } else {
                        self.weights[idx as usize] = weights_raw[idx as usize / 2] & 0xF;
                    }
                    bits_read += 4;
                }
            }
        }

        let bytes_read = if bits_read % 8 == 0 {
            bits_read / 8
        } else {
            (bits_read / 8) + 1
        };
        Ok(bytes_read as u32)
    }

    fn build_table_from_weights(&mut self) -> Result<(), String> {
        self.bits.clear();
        self.bits.resize(self.weights.len() + 1, 0);

        let mut weight_sum: u32 = 0;
        for w in &self.weights {
            if *w > MAX_MAX_NUM_BITS {
                return Err(format!(
                    "Cant have weight: {} bigger than max_num_bits: {}",
                    *w, MAX_MAX_NUM_BITS
                ));
            }
            weight_sum += if *w > 0 { (1 as u32) << (*w - 1) } else { 0 };
        }

        if weight_sum == 0 {
            return Err("Cant build huffman table without any weights".to_owned());
        }

        let max_bits = highest_bit_set(weight_sum) as u8;
        let left_over = ((1 as u32) << max_bits) - weight_sum;

        //left_over must be power of two
        if left_over & (left_over - 1) != 0 {
            return Err(format!(
                "Leftover must be power of two but is: {}",
                left_over
            ));
        }

        let last_weight = highest_bit_set(left_over) as u8;

        for symbol in 0..self.weights.len() {
            let bits = if self.weights[symbol] > 0 {
                max_bits + 1 - self.weights[symbol]
            } else {
                0
            };
            self.bits[symbol] = bits;
        }

        self.bits[self.weights.len()] = max_bits + 1 - last_weight;
        self.max_num_bits = max_bits;

        if max_bits > MAX_MAX_NUM_BITS {
            return Err(format!(
                "max_bits derived from weights is: {} should be lower than: {} ",
                max_bits, MAX_MAX_NUM_BITS
            ));
        }

        self.bit_ranks.clear();
        self.bit_ranks.resize((max_bits + 1) as usize, 0);
        for num_bits in &self.bits {
            self.bit_ranks[(*num_bits) as usize] += 1;
        }

        //fill with dummy symbols
        self.decode.resize(
            1 << self.max_num_bits,
            Entry {
                symbol: 0,
                num_bits: 0,
            },
        );

        //starting codes for each rank
        self.rank_indexes.clear();
        self.rank_indexes.resize((max_bits + 1) as usize, 0);

        self.rank_indexes[max_bits as usize] = 0;
        for bits in (1..self.rank_indexes.len() as u8).rev() {
            self.rank_indexes[bits as usize - 1] = self.rank_indexes[bits as usize]
                + self.bit_ranks[bits as usize] as usize * (1 << (max_bits - bits));
        }

        assert!(
            self.rank_indexes[0] == self.decode.len(),
            format!(
                "rank_idx[0]: {} should be: {}",
                self.rank_indexes[0],
                self.decode.len()
            )
        );

        for symbol in 0..self.bits.len() {
            let bits_for_symbol = self.bits[symbol];
            if bits_for_symbol != 0 {
                // allocate code for the symbol and set in the table
                // a code ignores all max_bits - bits[symbol] bits, so it gets
                // a range that spans all of those in the decoding table
                let base_idx = self.rank_indexes[bits_for_symbol as usize];
                let len = 1 << (max_bits - bits_for_symbol);
                self.rank_indexes[bits_for_symbol as usize] += len;
                for idx in 0..len {
                    self.decode[base_idx + idx].symbol = symbol as u8;
                    self.decode[base_idx + idx].num_bits = bits_for_symbol;
                }
            }
        }

        Ok(())
    }
}
