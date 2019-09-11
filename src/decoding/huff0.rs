use super::bit_reader::BitReader;
use super::bit_reader_reverse::BitReaderReversed;
use super::fse::FSEDecoder;
use super::fse::FSETable;

pub struct HuffmanTable {
    decode: Vec<Entry>,

    weights: Vec<u8>,
    max_num_bits: u8,
    bits: Vec<u8>,
    bit_ranks: Vec<u8>,
    rank_indexes: Vec<usize>,

    fse_table: FSETable,
}

pub struct HuffmanDecoder<'table> {
    table: &'table HuffmanTable,
    state: u64,
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

    pub fn next_state(&mut self, br: &mut BitReader) -> Result<u8, String> {
        let num_bits = self.table.decode[self.state as usize].num_bits;
        let new_bits = br.get_bits(num_bits as usize)?;
        self.state <<= num_bits;
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

    pub fn build_decoder(&mut self, source: &[u8]) -> Result<u32, String> {
        self.decode.clear();

        let bytes_used = self.read_weights(source)?;
        self.build_table_from_weights()?;
        Ok(bytes_used)
    }

    fn read_weights(&mut self, source: &[u8]) -> Result<u32, String> {
        let header = source[0];
        let mut bits_read = 8;

        match header {
            0...127 => {
                let fse_stream = &source[1..];
                //fse decompress weights
                let bytes_used_by_fse_header = self.fse_table.build_decoder(fse_stream, /*TODO find actual max*/100)?;
                let mut dec1 = FSEDecoder::new(&self.fse_table);
                let mut dec2 = FSEDecoder::new(&self.fse_table);

                let compressed_start = bytes_used_by_fse_header as usize;
                let compressed_end = bytes_used_by_fse_header as usize + header as usize;

                
                let compressed_weights = &fse_stream[compressed_start..compressed_end];
                let mut br = BitReaderReversed::new(compressed_weights);

                bits_read += (bytes_used_by_fse_header + header as usize) * 8;

                dec1.init_state(&mut br)?;
                bits_read += self.fse_table.accuracy_log as usize; 
                dec2.init_state(&mut br)?;
                bits_read += self.fse_table.accuracy_log as usize; 

                self.weights.clear();

                loop {
                    let w = dec1.decode_symbol();
                    self.weights.push(w);
                    dec1.update_state(&mut br)?;

                    if br.bits_remaining() < 0 {
                        //collect final states
                        self.weights.push(dec2.decode_symbol());
                        self.weights.push(dec1.decode_symbol());
                        break;
                    }

                    let w = dec2.decode_symbol();
                    self.weights.push(w);
                    dec2.update_state(&mut br)?;

                    if br.bits_remaining() < 0 {
                        //collect final states
                        self.weights.push(dec1.decode_symbol());
                        self.weights.push(dec2.decode_symbol());
                        break;
                    }
                }

                //maximum number of weights is 255 because we use u8 symbols
                assert!(self.weights.len() <= 255);
            }
            _ => {
                // weights are directly encoded
                let weights_raw = &source[1..];
                let num_weights = header - 127;
                self.weights.resize(num_weights as usize, 0);

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
        self.bits.resize(self.weights.len() + 1, 0);

        let mut weight_sum: u32 = 0;
        for w in &self.weights {
            weight_sum += if *w > 0 { (1 as u32) << (*w - 1) } else { 0 };
        }

        let max_bits = highest_bit_set(weight_sum) as u8;
        let left_over = ((1 as u32) << max_bits) - weight_sum;

        //left_over must be power of two
        assert!(left_over & (left_over - 1) == 0);
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
        self.rank_indexes.resize((max_bits + 1) as usize, 0);

        self.rank_indexes[max_bits as usize] = 0;
        for bits in (1..max_bits).rev() {
            self.rank_indexes[bits as usize - 1] = self.rank_indexes[bits as usize]
                + self.bit_ranks[bits as usize] as usize * (1 << (max_bits - bits));
        }

        assert!(self.rank_indexes[0] as usize == self.decode.len());

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
