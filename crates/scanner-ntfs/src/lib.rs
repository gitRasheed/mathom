//! mathom-scanner-ntfs: hand-rolled NTFS MFT scan backend (milestone 5).
//!
//! The parsing and assembly layers are portable pure byte code — they compile
//! and test on every OS. Only the volume I/O that feeds them is
//! Windows-specific, behind the `mft-backend` feature.

use std::fmt;

pub mod assemble;
pub mod boot;
pub mod fixture;
pub mod pipeline;
pub mod record;
pub mod runs;

#[cfg(all(windows, feature = "mft-backend"))]
mod scanner;
#[cfg(all(windows, feature = "mft-backend"))]
mod volume;

#[cfg(all(windows, feature = "mft-backend"))]
pub use scanner::MftScanner;

/// Parse-boundary failure: a static reason, no allocation. A corrupt or
/// hostile byte stream must produce one of these — never a panic or an
/// out-of-bounds read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError(pub &'static str);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ParseError {}
