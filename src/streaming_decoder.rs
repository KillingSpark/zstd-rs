use crate::frame_decoder::{BlockDecodingStrategy, FrameDecoder, FrameDecoderError};
use std::io::Read;

/// High level decoder that implements a io::Read that can be used with
/// io::Read::read_to_end / io::Read::read_exact or passing this to another library / module as a source for the decoded content
///
/// The lower level FrameDecoder by comparison allows for finer grained control but need sto have it's decode_blocks method called continously
/// to decode the zstd-frame.
pub struct StreamingDecoder<READ: Read> {
    pub decoder: FrameDecoder,
    source: READ,
}

impl<READ: Read> StreamingDecoder<READ> {
    pub fn new(mut source: READ) -> Result<StreamingDecoder<READ>, FrameDecoderError> {
        let mut decoder = FrameDecoder::new();
        decoder.init(&mut source)?;
        Ok(StreamingDecoder { decoder, source })
    }

    pub fn new_with_decoder(
        mut source: READ,
        mut decoder: FrameDecoder,
    ) -> Result<StreamingDecoder<READ>, FrameDecoderError> {
        decoder.init(&mut source)?;
        Ok(StreamingDecoder { decoder, source })
    }

    pub fn inner(self) -> FrameDecoder {
        self.decoder
    }
}

impl<READ: Read> Read for StreamingDecoder<READ> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.decoder.is_finished() && self.decoder.can_collect() == 0 {
            //No more bytes can ever be decoded
            return Ok(0);
        }

        // need to loop. The UpToBytes strategy doesn't take any effort to actually reach that limit.
        // The first few calls can result in just filling the decode buffer but these bytes can not be collected.
        // So we need to call this until we can actually collect enough bytes

        // TODO add BlockDecodingStrategy::UntilCollectable(usize) that pushes this logic into the decode_blocks function
        while self.decoder.can_collect() < buf.len() && !self.decoder.is_finished() {
            //More bytes can be decoded
            let additional_bytes_needed = buf.len() - self.decoder.can_collect();
            match self.decoder.decode_blocks(
                &mut self.source,
                BlockDecodingStrategy::UptoBytes(additional_bytes_needed),
            ) {
                Ok(_) => { /*Nothing to do*/ }
                Err(e) => {
                    let err = std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Error in the zstd decoder: {:?}", e),
                    );
                    return Err(err);
                }
            }
        }

        self.decoder.read(buf)
    }
}
