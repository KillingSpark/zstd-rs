use super::bit_reader::BitReader;
use super::bit_reader_reverse::BitReaderReversed;

pub struct FSETable {
    decode: Vec<Entry>, //used to decode symbols, and calculate the next state

    pub accuracy_log: u8,
    symbol_probablilities: Vec<i32>, //used while building the decode Vector
}

pub struct FSEDecoder<'table> {
    state: usize,
    table: &'table FSETable,
}

#[derive(Copy, Clone)]
struct Entry {
    base_line: usize,
    num_bits: u8,
    symbol: u8,
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
            table: table,
        }
    }

    pub fn decode_symbol(&self) -> u8 {
        self.table.decode[self.state].symbol
    }

    pub fn init_state(&mut self, bits: &mut BitReaderReversed) -> Result<(), String> {
        self.state = bits.get_bits(self.table.accuracy_log as usize)? as usize;
        Ok(())
    }

    pub fn update_state(&mut self, bits: &mut BitReaderReversed) -> Result<(), String> {
        let add = bits.get_bits(self.table.decode[self.state].num_bits as usize)?;
        self.state = self.table.decode[self.state].base_line + add as usize;
        Ok(())
    }
}

impl FSETable {
    pub fn new() -> FSETable {
        FSETable {
            symbol_probablilities: Vec::with_capacity(256), //will never be more than 256 symbols because u8
            decode: Vec::new(),                             //depending on acc_log.
            accuracy_log: 0,
        }
    }

    //returns how many BYTEs (not bits) were read while building the decoder
    pub fn build_decoder(&mut self, source: &[u8]) -> Result<usize, String> {
        self.accuracy_log = 0;

        let bytes_read = self.read_probabilities(source)?;
        self.build_decoding_table();

        Ok(bytes_read)
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
            for state_number_for_symbol in 0..prob {
                while position < negative_idx {
                    //everything above negative_idx is already taken
                    position = next_position(position, table_size);
                }
                let entry = &mut self.decode[negative_idx];
                entry.symbol = symbol as u8;

                let (bl, nb) = calc_baseline_and_numbits(
                    table_size as u32,
                    prob as u32,
                    state_number_for_symbol as u32,
                );

                entry.base_line = bl;
                entry.num_bits = nb;
            }
        }
    }

    fn read_probabilities(&mut self, source: &[u8]) -> Result<usize, String> {
        self.symbol_probablilities.clear(); //just clear, we will fill a probability for each entry anyways. Non eed to force new allocs here

        let mut bits_read = 0; //keep track of all bits read

        let mut br = BitReader::new(source);
        self.accuracy_log = ACC_LOG_OFFSET + (br.get_bits(4)? as u8);

        let probablility_sum = 1 << self.accuracy_log;
        let mut probability_counter = 0;

        while probability_counter < probablility_sum {
            let max_remaining_value = probablility_sum - probability_counter + 1; // '+ 1' because values are proabilities + 1
            let bits_to_read = highest_bit_set(max_remaining_value);

            let unchecked_value = br.get_bits(bits_to_read as usize)? as u32;
            bits_read += bits_to_read;

            let low_threshold = ((1 << bits_to_read) - 1) - (max_remaining_value as u32);
            let middle = (1 << bits_to_read) / 2;
            let high_threshold = middle + low_threshold;

            let value = if unchecked_value < low_threshold {
                //value is fine but need to push back one bit (which was a zero)
                br.return_bits(1);
                bits_read -= 1;
                unchecked_value
            } else {
                if unchecked_value < high_threshold {
                    //value is actually smaller, we read a '1' bit accidentally
                    br.return_bits(1);
                    bits_read -= 1;
                    let small_value = unchecked_value & (1 << bits_to_read - 1) - 1; //delete highest bit, which got pushed back to the br
                    small_value
                } else {
                    //value is fine, its just big
                    unchecked_value
                }
            };

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
                    bits_read += 2;

                    for _ in 0..skip_amount {
                        self.symbol_probablilities.push(0);
                    }
                    if skip_amount != 3 {
                        break;
                    }
                }
            }
        }

        assert!(probability_counter == probablility_sum, format!("The counter: {} exceeded the expected sum: {}. This means an error or corrupted data", probability_counter, probablility_sum));

        let bytes_read = if bits_read % 8 == 0 {
            bits_read / 8
        } else {
            bits_read / 8 + 1
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
    let num_state_slices = 1 << highest_bit_set(num_states_symbol); //always power of two

    let num_double_width_state_slices = num_state_slices - num_states_symbol; //leftovers to the powerof two need to be distributed
    let num_single_width_state_slices = num_state_slices - num_double_width_state_slices; //these will not receive a double width slice of states
    let slice_width = num_states_total / num_state_slices; //size of a single width slice of states
    let num_bits = highest_bit_set(slice_width); //number of bits needed to read for one slice

    if state_number < num_double_width_state_slices {
        //all single width
        let baseline = num_single_width_state_slices * slice_width + state_number * slice_width * 2;
        (baseline as usize, (num_bits + 1) as u8)
    } else {
        let index_in_single_width = state_number - num_double_width_state_slices;
        (
            (slice_width * index_in_single_width) as usize,
            num_bits as u8,
        )
    }
}
