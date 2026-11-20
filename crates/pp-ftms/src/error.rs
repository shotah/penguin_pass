//! Decode and encode failures.
//!
//! Truncated or malformed input returns one of these. Parsers do not panic.

use thiserror::Error;

/// A codec rejected a buffer or a numeric value.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FtmsError {
    /// The buffer ended before `what` was complete.
    #[error("truncated {what}: have {have} bytes, need {need}")]
    Truncated {
        /// Field that could not be read.
        what: &'static str,
        /// Bytes still available when the read was attempted.
        have: usize,
        /// Bytes that field needs.
        need: usize,
    },
    /// A required discriminator was wrong, or a value could not be represented.
    #[error("malformed {what}: {detail}")]
    Malformed {
        /// Field or message that failed.
        what: &'static str,
        /// Why it failed.
        detail: &'static str,
    },
    /// A float argument was NaN or infinite.
    #[error("value is not finite")]
    NonFinite,
}
