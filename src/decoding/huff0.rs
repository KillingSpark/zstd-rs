use super::bit_reader::BitReader;

pub struct HuffmanDecoder {
    decode: Vec<Entry>,
    state: u64,

    weights: Vec<u8>,
    max_num_bits: u8,
    bits: Vec<u8>,
    bit_ranks: Vec<u8>,
    rank_indexes: Vec<usize>,
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

impl HuffmanDecoder {
    pub fn build_decoder(&mut self, source: &[u8]) -> Result<u32, String> {
        self.decode.clear();

        //TODO read weights
        let _ = source;
        self.build_table_from_weights()?;
        Ok(100 /* number of bytes needed while reading the weights */)
    }

    pub fn decode_symbol(&mut self) -> u8 {
        self.decode[self.state as usize].symbol
    }

    pub fn next_state(&mut self, br: &mut BitReader) -> Result<u8, String> {
        let num_bits = self.decode[self.state as usize].num_bits;
        let new_bits = br.get_bits(num_bits as usize)?; 
        self.state <<= num_bits;
        self.state |= new_bits as u64;
        Ok(num_bits)
    }

    fn build_table_from_weights(&mut self) -> Result<(), String >{
        self.bits.resize(self.weights.len() + 1, 0);

        let mut weight_sum: u32 = 0;
        for x in &self.weights {
            weight_sum += if *x > 0 { (1 as u32) << (*x - 1) } else { 0 };
        }

        let max_bits = highest_bit_set(weight_sum) as u8;
        let left_over = ((1 as u32) << max_bits) - weight_sum;

        //left_over must be power of two
        assert!(left_over & (left_over-1) == 0);
        let last_weight = highest_bit_set(left_over) as u8;

        for idx in 0..self.weights.len() {
            let weight = if self.weights[idx] > 0 { max_bits + 1 - self.weights[idx] } else { 0 };
            self.bits[idx] = weight;
        }
        self.bits[self.weights.len()] = max_bits + 1 - last_weight;
        self.max_num_bits = max_bits;

        if max_bits > MAX_MAX_NUM_BITS {
            return Err(format!("max_bits derived from weights is: {} should be lower than: {} ", max_bits, MAX_MAX_NUM_BITS));
        }


        self.bit_ranks.resize((max_bits+1) as usize, 0);
        for w in &self.bits {
            self.bit_ranks[(*w) as usize] += 1;
        }

        //fill with dummy symbols
        self.decode.resize(1 << self.max_num_bits, Entry{symbol: 0, num_bits: 0});

        //starting codes for each rank
        self.rank_indexes.resize((max_bits+1) as usize, 0);

        self.rank_indexes[max_bits as usize] = 0;
        for bits in (1..max_bits).rev() {
            self.rank_indexes[bits as usize - 1] = self.rank_indexes[bits as usize] + self.bit_ranks[bits as usize] as usize * (1 << (max_bits-bits));

            let lower_index = self.rank_indexes[bits as usize - 1]; 
            let higher_index = self.rank_indexes[bits as usize]; 
            for idx in lower_index..higher_index {
                self.decode[idx as usize].num_bits = bits;
            } 
        }

        assert!(self.rank_indexes[0] as usize == self.decode.len());

        for symbol in 0..self.bits.len() {
            if self.bits[symbol] != 0 {
                // allocate code for the symbol and set in the table
                // a code ignores all max_bits - bits[symbol] bits, so it gets
                // a range that spans all of those in the decoding table
                let len = 1 << (max_bits -self.bits[symbol]);
                for idx in 0..len {
                    self.decode[idx].symbol = symbol as u8;
                    self.rank_indexes[self.bits[symbol] as usize] += len;
                }
            }
        }

        Ok(())
    }
}
