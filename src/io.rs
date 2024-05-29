//! Re-exports of std values for when the std is available.
#[cfg(feature = "std")]
pub use std::io::{Error, ErrorKind, Read, Write};
