use super::frame;
use crate::block;
use crate::decoding::scratch::DecoderScratch;
use std::io::{Read, Write};

pub struct FrameDecoder {
    pub frame: frame::Frame,
    decoder_scratch: DecoderScratch,
}

impl FrameDecoder {
    pub fn new(source: &mut Read) -> FrameDecoder {
        let frame = frame::read_frame_header(source).unwrap();
        let window_size = frame.header.window_size().unwrap();
        frame.check_valid().unwrap();
        FrameDecoder {
            frame: frame,
            decoder_scratch: DecoderScratch::new(window_size as usize),
        }
    }

    pub fn decode_blocks(&mut self, source: &mut Read, target: &mut Write) -> Result<(), String> {
        let mut block_dec = block::block_decoder::new();
        
        loop {
            let block_header = block_dec.read_block_header(source).unwrap();
            println!("");
            println!("Found {} block with size: {}", block_header.block_type, block_header.content_size);

            block_dec
                .decode_block_content(&block_header, &mut self.decoder_scratch, source, target)
                .unwrap();
            
            if block_header.last_block {
                //TODO flush buffer
                if self.frame.header.descriptor.content_checksum_flag() {
                    let rest: Vec<_> = source.bytes().collect();
                    assert!(rest.len() == 4);
                    println!("\n Checksum found: {:?}", rest);
                }else{
                    let rest: Vec<_> = source.bytes().collect();
                    assert!(rest.len() == 0);
                }
                break;
            }
        }

        Ok(())
    }
}
