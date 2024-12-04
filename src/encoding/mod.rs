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
    /// Get a space where we can put data to be matched on
    fn get_next_space(&mut self) -> alloc::vec::Vec<u8>;
    /// Get a reference to the last commited space
    fn get_last_space(&mut self) -> &[u8];
    /// Commit a space to the matcher so it can be matched against
    fn commit_space(&mut self, space: alloc::vec::Vec<u8>);
    /// Just process the data in the last commited space for future matching
    fn skip_matching(&mut self);
    /// Process the data in the last commited space for future matching AND generate matches for the data
    fn start_matching(&mut self, handle_sequence: impl for<'a> FnMut(Sequence<'a>));
}
