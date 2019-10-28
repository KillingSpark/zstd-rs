use crate::decoding::bit_reader::BitReader;
use crate::decoding::bit_reader_reverse::BitReaderReversed;

#[derive(Clone)]
pub struct FSETable {
    pub decode: Vec<Entry>, //used to decode symbols, and calculate the next state

    pub accuracy_log: u8,
    pub symbol_probablilities: Vec<i32>, //used while building the decode Vector
    symbol_counter: Vec<u32>,
}

pub struct FSEDecoder<'table> {
    pub state: usize,
    table: &'table FSETable,
}

#[derive(Copy, Clone)]
pub struct Entry {
    pub base_line: usize,
    pub num_bits: u8,
    pub symbol: u8,
}

const ACC_LOG_OFFSET: u8 = 5;

const fn num_bits<T>() -> usize {
    std::mem::size_of::<T>() * 8
}

fn highest_bit_set(x: u32) -> u32 {
    assert!(x > 0);
    num_bits::<u32>() as u32 - x.leading_zeros()
}

impl<'t> FSEDecoder<'t> {
    pub fn new(table: &'t FSETable) -> FSEDecoder {
        FSEDecoder {
            state: 0,
            table,
        }
    }

    pub fn decode_symbol(&self) -> u8 {
        self.table.decode[self.state].symbol
    }

    pub fn init_state(&mut self, bits: &mut BitReaderReversed) -> Result<(), String> {
        if self.table.accuracy_log == 0 {
            return Err("Tried to use an unitizialized table!".to_owned());
        }
        self.state = bits.get_bits(self.table.accuracy_log as usize)? as usize;

        Ok(())
    }

    pub fn update_state(&mut self, bits: &mut BitReaderReversed) -> Result<(), String> {
        let num_bits = self.table.decode[self.state].num_bits as usize;
        let add = bits.get_bits(num_bits)?;
        let base_line = self.table.decode[self.state].base_line;
        let new_state = base_line + add as usize;
        assert!(new_state < self.table.decode.len());
        self.state = new_state;

        //println!("Update: {}, {} -> {}", base_line, add,  self.state);
        Ok(())
    }
}

impl FSETable {
    pub fn new() -> FSETable {
        FSETable {
            symbol_probablilities: Vec::with_capacity(256), //will never be more than 256 symbols because u8
            symbol_counter: Vec::with_capacity(256), //will never be more than 256 symbols because u8
            decode: Vec::new(),                      //depending on acc_log.
            accuracy_log: 0,
        }
    }

    pub fn reset(&mut self) {
        self.symbol_counter.clear();
        self.symbol_probablilities.clear();
        self.decode.clear();
        self.accuracy_log = 0;
    }

    //returns how many BYTEs (not bits) were read while building the decoder
    pub fn build_decoder(&mut self, source: &[u8], max_log: u8) -> Result<usize, String> {
        self.accuracy_log = 0;

        let bytes_read = self.read_probabilities(source, max_log)?;
        self.build_decoding_table();

        Ok(bytes_read)
    }

    pub fn build_from_probabilities(
        &mut self,
        acc_log: u8,
        probs: &[i32],
    ) -> Result<(), String> {
        if acc_log == 0 {
            return Err("Acclog must be at least 1".to_owned());
        }
        self.symbol_probablilities = probs.to_vec();
        self.accuracy_log = acc_log;
        self.build_decoding_table();
        Ok(())
    }

    fn build_decoding_table(&mut self) {
        self.decode.clear();

        let table_size = 1 << self.accuracy_log;
        if self.decode.len() < table_size {
            self.decode.reserve(table_size as usize - self.decode.len());
        }
        //fill with dummy entries
        self.decode.resize(
            table_size,
            Entry {
                base_line: 0,
                num_bits: 0,
                symbol: 0,
            },
        );

        let mut negative_idx = table_size; //will point to the highest index with is already occupied by a negative-probability-symbol

        //first scan for all -1 probabilities and place them at the top of the table
        for symbol in 0..self.symbol_probablilities.len() {
            if self.symbol_probablilities[symbol] == -1 {
                negative_idx -= 1;
                let entry = &mut self.decode[negative_idx];
                entry.symbol = symbol as u8;
                entry.base_line = 0;
                entry.num_bits = self.accuracy_log;
            }
        }

        //then place in a semi-random order all of the other symbols
        let mut position = 0;
        for idx in 0..self.symbol_probablilities.len() {
            let symbol = idx as u8;
            if self.symbol_probablilities[idx] <= 0 {
                continue;
            }

            //for each probability point the symbol gets on slot
            let prob = self.symbol_probablilities[idx];
            for _ in 0..prob {
                let entry = &mut self.decode[position];
                entry.symbol = symbol as u8;

                position = next_position(position, table_size);
                while position >= negative_idx {
                    position = next_position(position, table_size);
                    //everything above negative_idx is already taken
                }
            }
        }

        // baselines and num_bits can only be caluclated when all symbols have been spread
        self.symbol_counter.clear();
        self.symbol_counter
            .resize(self.symbol_probablilities.len(), 0);
        for idx in 0..negative_idx {
            let entry = &mut self.decode[idx];
            let symbol = entry.symbol;
            let prob = self.symbol_probablilities[symbol as usize];

            let symbol_count = self.symbol_counter[symbol as usize];
            let (bl, nb) =
                calc_baseline_and_numbits(table_size as u32, prob as u32, symbol_count as u32);

            //println!("symbol: {:2}, table: {}, prob: {:3}, count: {:3}, bl: {:3}, nb: {:2}", symbol, table_size, prob, symbol_count, bl, nb);

            assert!(nb <= self.accuracy_log);
            self.symbol_counter[symbol as usize] += 1;

            entry.base_line = bl;
            entry.num_bits = nb;
        }
    }

    fn read_probabilities(&mut self, source: &[u8], max_log: u8) -> Result<usize, String> {
        self.symbol_probablilities.clear(); //just clear, we will fill a probability for each entry anyways. No need to force new allocs here

        let mut br = BitReader::new(source);
        self.accuracy_log = ACC_LOG_OFFSET + (br.get_bits(4)? as u8);
        if self.accuracy_log > max_log {
            return Err(format!(
                "Found FSE acc_log: {} bigger than allowed maximum in this case: {}",
                self.accuracy_log, max_log
            ));
        }
        if self.accuracy_log == 0 {
            return Err("Acclog must be at least 1".to_owned());
        }

        let probablility_sum = 1 << self.accuracy_log;
        let mut probability_counter = 0;

        while probability_counter < probablility_sum {
            let max_remaining_value = probablility_sum - probability_counter + 1;
            let bits_to_read = highest_bit_set(max_remaining_value);

            let unchecked_value = br.get_bits(bits_to_read as usize)? as u32;

            let low_threshold = ((1 << bits_to_read) - 1) - (max_remaining_value as u32);
            let mask = (1 << (bits_to_read - 1)) - 1;
            let small_value = unchecked_value & mask;

            let value = if small_value < low_threshold {
                br.return_bits(1);
                small_value
            } else if unchecked_value > mask {
    unchecked_value - low_threshold
} else {
    unchecked_value
};
            //println!("{}, {}, {}", self.symbol_probablilities.len(), unchecked_value, value);

            let prob = (value as i32) - 1;

            self.symbol_probablilities.push(prob);
            if prob != 0 {
                if prob > 0 {
                    probability_counter += prob as u32;
                } else {
                    // probability -1 counts as 1
                    assert!(prob == -1);
                    probability_counter += 1;
                }
            } else {
                //fast skip further zero probabilities
                loop {
                    let skip_amount = br.get_bits(2)?;

                    for _ in 0..skip_amount {
                        self.symbol_probablilities.push(0);
                    }
                    if skip_amount != 3 {
                        break;
                    }
                }
            }
        }

        if probability_counter != probablility_sum {
            return Err(format!("The counter: {} exceeded the expected sum: {}. This means an error or corrupted data \n {:?}", probability_counter, probablility_sum, self.symbol_probablilities));
        }
        if self.symbol_probablilities.len() > 256 {
            return Err(format!(
                "There are too many symbols in this distribution: {}. Max: 256",
                self.symbol_probablilities.len()
            ));
        }

        let bytes_read = if br.bits_read() % 8 == 0 {
            br.bits_read() / 8
        } else {
            (br.bits_read() / 8) + 1
        };
        Ok(bytes_read as usize)
    }
}

//utility functions for building the decoding table from probabilities
fn next_position(mut p: usize, table_size: usize) -> usize {
    p += (table_size >> 1) + (table_size >> 3) + 3;
    p &= table_size - 1;
    p
}

fn calc_baseline_and_numbits(
    num_states_total: u32,
    num_states_symbol: u32,
    state_number: u32,
) -> (usize, u8) {
    let num_state_slices = if 1 << (highest_bit_set(num_states_symbol) - 1) == num_states_symbol {
        num_states_symbol
    } else {
        1 << (highest_bit_set(num_states_symbol))
    }; //always power of two

    let num_double_width_state_slices = num_state_slices - num_states_symbol; //leftovers to the powerof two need to be distributed
    let num_single_width_state_slices = num_states_symbol - num_double_width_state_slices; //these will not receive a double width slice of states
    let slice_width = num_states_total / num_state_slices; //size of a single width slice of states
    let num_bits = highest_bit_set(slice_width) - 1; //number of bits needed to read for one slice

    if state_number < num_double_width_state_slices {
        let baseline = num_single_width_state_slices * slice_width + state_number * slice_width * 2;
        (baseline as usize, num_bits as u8 + 1)
    } else {
        let index_shifted = state_number - num_double_width_state_slices;
        ((index_shifted * slice_width) as usize, num_bits as u8)
    }
}
