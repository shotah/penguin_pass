//! Errors from the trainer interface.
//!
//! These are returned on the game thread. A background Bluetooth failure is
//! also copied into [`crate::ConnectionState::Failed`] so the frame loop can
//! see it without waiting on [`Trainer::send`](crate::Trainer::send).

use thiserror::Error;

use pp_ftms::FtmsError;

/// Why a trainer call could not be queued.
#[derive(Debug, Error)]
pub enum TrainerError {
    /// The background thread has exited.
    #[error("trainer background thread has stopped")]
    Stopped,
    /// [`crate::Trainer::send`] was called before the link reached `Connected`.
    #[error("trainer is not connected")]
    NotConnected,
    /// The session is subscribed to measurements and will not write the control point.
    #[error("trainer is read-only")]
    ReadOnly,
    /// A grade or another numeric argument was NaN or infinite.
    #[error("{0}")]
    Message(String),
    /// The FTMS codec rejected a value before it was written.
    #[error(transparent)]
    Codec(#[from] FtmsError),
}
