//! Modules used for compressing/encoding data into the Zstd format.
// TODO: put behind a feature gate
pub(crate) mod bit_writer;
pub(crate) mod block_header;
pub(crate) mod blocks;
mod frame_encoder;
pub use frame_encoder::*;
use match_generator::Sequence;
pub(crate) mod frame_header;
pub(crate) mod match_generator;
pub(crate) mod util;

pub(crate) trait Matcher {
    fn get_next_space(&mut self) -> &mut [u8];
    fn get_last_space(&mut self) -> &[u8];
    fn commit_space(&mut self, len: usize);
    fn start_matching(&mut self, len: usize, handle_sequence: impl for<'a> FnMut(Sequence<'a>));
}
