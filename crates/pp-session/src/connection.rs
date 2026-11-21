//! Structured link status for the trainer chip and the pairing screen.
//!
//! [`crate::SessionStatus::line`] stays as it is. This module is the data
//! the interface matches on instead of parsing that line.

use crate::session::ActiveSource;

/// Where the trainer link is, independent of the status sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPhase {
    /// No scan is running. The simulated rider is the source.
    Idle,
    /// Looking for trainers.
    Searching,
    /// A connection attempt is in progress.
    Connecting,
    /// Measurements are arriving.
    Connected,
    /// The link dropped, or the control mode is being switched, and the
    /// session is bringing the trainer back.
    Reconnecting,
    /// Bluetooth or the last attempt failed, and nothing is retrying a live ride.
    Failed,
}

/// Whether the session is allowed to change the trainer's resistance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResistanceMode {
    /// Grade commands are being sent.
    Controlled,
    /// The game only reads. This is also the mode when control was requested
    /// and the trainer refused it.
    ReadingOnly,
}

/// Link facts for the trainer chip and the pairing headline.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionStatus {
    /// Who is feeding the physics.
    pub source: ActiveSource,
    /// Trainer name when a scan or a link exists.
    pub trainer_name: Option<String>,
    /// Link phase.
    pub phase: LinkPhase,
    /// The mode actually in effect, not only the settings toggle.
    pub resistance: ResistanceMode,
    /// `false` when the adapter is missing or Bluetooth could not start.
    pub bluetooth_available: bool,
    /// One sentence for the pairing screen. `None` when the link is not failed.
    pub failure_reason: Option<String>,
    /// The trainer crate's own text, for the log.
    pub failure_detail: Option<String>,
}

/// Turn a trainer or Bluetooth error into one sentence.
///
/// The original text is kept separately on [`ConnectionStatus::failure_detail`].
#[must_use]
pub fn plain_failure(raw: &str) -> String {
    let text = raw.to_ascii_lowercase();
    if text.contains("adapter")
        || text.contains("bluetooth")
        || text.contains("not available")
        || text.contains("no bluetooth")
    {
        "Bluetooth is off, or this computer has no Bluetooth adapter.".to_owned()
    } else if text.contains("timeout") || text.contains("timed out") || text.contains("not answer")
    {
        "The trainer did not answer in time.".to_owned()
    } else if text.contains("no trainer")
        || text.contains("not in range")
        || text.contains("not found")
    {
        "That trainer is not in range.".to_owned()
    } else {
        "Could not connect to the trainer.".to_owned()
    }
}

/// `true` when a warning means the trainer refused control.
#[must_use]
pub fn control_was_refused(warning: Option<&str>) -> bool {
    warning.is_some_and(|warning| {
        let warning = warning.to_ascii_lowercase();
        warning.contains("control not permitted") || warning.contains("control refused")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_failure_maps_the_common_cases() {
        assert_eq!(
            plain_failure("bluetooth adapter not found"),
            "Bluetooth is off, or this computer has no Bluetooth adapter."
        );
        assert_eq!(
            plain_failure("control timeout"),
            "The trainer did not answer in time."
        );
        assert_eq!(
            plain_failure("no trainer matches Neo"),
            "That trainer is not in range."
        );
        assert_eq!(
            plain_failure("something else"),
            "Could not connect to the trainer."
        );
        assert!(control_was_refused(Some("Control not permitted")));
        assert!(!control_was_refused(Some("grade rejected")));
    }
}
