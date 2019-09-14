use super::frame;
use crate::block;
use crate::decoding::scratch::DecoderScratch;
use std::io::Read;

pub struct FrameDecoder {
    pub frame: frame::Frame,
    decoder_scratch: DecoderScratch,
}

impl FrameDecoder {
    pub fn new(source: &mut Read) -> Result<FrameDecoder, String> {
        let frame = frame::read_frame_header(source)?;
        let window_size = frame.header.window_size()?;
        frame.check_valid()?;
        Ok(FrameDecoder {
            frame: frame,
            decoder_scratch: DecoderScratch::new(window_size as usize),
        })
    }

    pub fn decode_blocks(&mut self, source: &mut Read) -> Result<(), String> {
        let mut block_dec = block::block_decoder::new();

        let mut block_counter = 0;
        loop {
            if crate::VERBOSE {
                println!("################");
                println!("Next Block: {}", block_counter);
                println!("################");
            }
            let block_header = block_dec.read_block_header(source)?;
            if crate::VERBOSE {
                println!("");
                println!(
                    "Found {} block with size: {}, which will be of size: {}",
                    block_header.block_type,
                    block_header.content_size,
                    block_header.decompressed_size
                );
            }

            block_dec.decode_block_content(&block_header, &mut self.decoder_scratch, source)?;

            if crate::VERBOSE {
                println!("Output: {}", self.decoder_scratch.buffer.len());
            }

            if block_header.last_block {
                //TODO flush buffer
                if self.frame.header.descriptor.content_checksum_flag() {
                    let rest: Vec<_> = source.bytes().collect();
                    assert!(rest.len() == 4);
                    if crate::VERBOSE {
                        println!("\n Checksum found: {:?}", rest);
                    }
                } else {
                    let rest: Vec<_> = source.bytes().collect();
                    assert!(rest.len() == 0);
                }
                break;
            }
            block_counter += 1;
        }

        Ok(())
    }

    pub fn drain_buffer_completely(&mut self) -> Vec<u8> {
        self.decoder_scratch.buffer.drain()
    }
}
