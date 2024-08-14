//! After Magic_Number and Frame_Header, there are some number of blocks. Each frame must have at least one block,
//! but there is no upper limit on the number of blocks per frame.
//!
//! There are a few different kinds of blocks, and implementations for those kinds are
//! stored here.
mod raw;

pub(super) use raw::*;

// An error produced during block compression.
pub enum BlockError {}
