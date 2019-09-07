use super::bit_reader::BitReader;

pub struct FSEDecoder {
    state: usize,
    decode: Vec<Entry>, //used to decode symbols, and calculate the next state

    accuracy_log: u8,
    symbol_probablilities: Vec<i32>, //used while building the decode Vector
}

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

impl FSEDecoder {
    //returns how many BYTEs (not bits) were read while building the decoder

    pub fn build_decoder(&mut self, source: &[u8]) -> Result<usize, String> {
        self.decode.clear();
        self.symbol_probablilities.clear();
        self.state = 0;
        self.accuracy_log = 0;

        let bytes_read = self.build_probabilities(source)?;
        self.build_decoding_table();

        Ok(bytes_read)
    }

    fn build_decoding_table(&mut self) {
        //TODO build decoding table
    }

    fn build_probabilities(&mut self, source: &[u8]) -> Result<usize, String> {
        let mut bits_read = 0; //keep track of all bits read

        let mut br = BitReader::new();
        self.accuracy_log = ACC_LOG_OFFSET + (br.get_bits(4, source)? as u8);

        let probablility_sum = 1 << self.accuracy_log;
        let mut probability_counter = 0;

        while probability_counter < probablility_sum {
            let max_remaining_value = probablility_sum - probability_counter + 1; // '+ 1' because values are proabilities + 1
            let bits_to_read = highest_bit_set(max_remaining_value);

            let unchecked_value = br.get_bits(bits_to_read as usize, source)? as u32;
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
                }
            } else {
                //fast skip zero probabilities
                loop {
                    let skip_amount = br.get_bits(2, source)?;
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
