use core::borrow::BorrowMut;

use crate::frame_decoder::{BlockDecodingStrategy, FrameDecoder, FrameDecoderError};
use crate::io::{Error, ErrorKind, Read};

/// High level decoder that implements a io::Read that can be used with
/// io::Read::read_to_end / io::Read::read_exact or passing this to another library / module as a source for the decoded content
///
/// The lower level FrameDecoder by comparison allows for finer grained control but need sto have it's decode_blocks method called continously
/// to decode the zstd-frame.
pub struct StreamingDecoder<READ: Read, DEC: BorrowMut<FrameDecoder>> {
    pub decoder: DEC,
    source: READ,
}

impl<READ: Read, DEC: BorrowMut<FrameDecoder>> StreamingDecoder<READ, DEC> {
    pub fn new_with_decoder(
        mut source: READ,
        mut decoder: DEC,
    ) -> Result<StreamingDecoder<READ, DEC>, FrameDecoderError> {
        decoder.borrow_mut().init(&mut source)?;
        Ok(StreamingDecoder { decoder, source })
    }
}

impl<READ: Read> StreamingDecoder<READ, FrameDecoder> {
    pub fn new(
        mut source: READ,
    ) -> Result<StreamingDecoder<READ, FrameDecoder>, FrameDecoderError> {
        let mut decoder = FrameDecoder::new();
        decoder.init(&mut source)?;
        Ok(StreamingDecoder { decoder, source })
    }

    pub fn inner(self) -> FrameDecoder {
        self.decoder
    }
}

impl<READ: Read, DEC: BorrowMut<FrameDecoder>> Read for StreamingDecoder<READ, DEC> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let decoder = self.decoder.borrow_mut();
        if decoder.is_finished() && decoder.can_collect() == 0 {
            //No more bytes can ever be decoded
            return Ok(0);
        }

        // need to loop. The UpToBytes strategy doesn't take any effort to actually reach that limit.
        // The first few calls can result in just filling the decode buffer but these bytes can not be collected.
        // So we need to call this until we can actually collect enough bytes

        // TODO add BlockDecodingStrategy::UntilCollectable(usize) that pushes this logic into the decode_blocks function
        while decoder.can_collect() < buf.len() && !decoder.is_finished() {
            //More bytes can be decoded
            let additional_bytes_needed = buf.len() - decoder.can_collect();
            match decoder.decode_blocks(
                &mut self.source,
                BlockDecodingStrategy::UptoBytes(additional_bytes_needed),
            ) {
                Ok(_) => { /*Nothing to do*/ }
                Err(e) => {
                    let err = Error::new(
                        ErrorKind::Other,
                        alloc::format!("Error in the zstd decoder: {:?}", e),
                    );
                    return Err(err);
                }
            }
        }

        decoder.read(buf)
    }
}
