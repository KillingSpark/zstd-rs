//! Re-exports of std values for when the std is available or local reimplementations if std is not available
#[cfg(feature = "std")]
pub use std::io::{Error, ErrorKind, Read, Write};
