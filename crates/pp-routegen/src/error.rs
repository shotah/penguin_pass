//! Errors from spec validation and route generation.

use std::fmt;

use crate::spec::SpecError;

/// Why [`crate::generate`] refused a spec or could not build a route.
#[derive(Debug)]
pub enum RouteGenError {
    /// The spec failed validation.
    Spec(SpecError),
    /// Every attempt missed a rideability check.
    ///
    /// `attempts` is how many candidates were built. `last_reason` is the
    /// check that rejected the final candidate.
    Exhausted {
        /// Candidates built before giving up.
        attempts: u32,
        /// Why the last candidate was rejected.
        last_reason: String,
    },
}

impl fmt::Display for RouteGenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spec(err) => write!(f, "{err}"),
            Self::Exhausted {
                attempts,
                last_reason,
            } => write!(
                f,
                "no rideable route after {attempts} attempts ({last_reason})"
            ),
        }
    }
}

impl std::error::Error for RouteGenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spec(err) => Some(err),
            Self::Exhausted { .. } => None,
        }
    }
}

impl From<SpecError> for RouteGenError {
    fn from(err: SpecError) -> Self {
        Self::Spec(err)
    }
}
