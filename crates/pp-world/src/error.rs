//! Construction errors for the chunk grid and the planner.

use std::fmt;

/// A chunk edge or a planner distance was not usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldError {
    /// The chunk edge was not finite or was not greater than zero.
    ChunkEdge,
    /// A radius, corridor width, or margin was not finite or was negative.
    Config {
        /// Name of the [`crate::PlannerConfig`] field that failed.
        field: &'static str,
    },
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChunkEdge => f.write_str("chunk edge must be finite and greater than 0 m"),
            Self::Config { field } => {
                write!(f, "{field} must be finite and at least 0 m")
            }
        }
    }
}

impl std::error::Error for WorldError {}
