//! Bounded toast queue.

use std::collections::VecDeque;
use std::path::PathBuf;

/// How many events are kept when the game does not drain them.
pub const EVENT_QUEUE_CAP: usize = 32;

/// Something the interface may toast.
#[derive(Debug, Clone, PartialEq)]
pub enum RideEvent {
    /// A trainer reached `Connected` for the first time this link.
    TrainerConnected {
        /// Trainer name.
        name: String,
    },
    /// The link dropped and the trainer is retrying.
    TrainerLinkLost {
        /// Trainer name.
        name: String,
    },
    /// The link came back after [`Self::TrainerLinkLost`].
    TrainerReconnected {
        /// Trainer name.
        name: String,
    },
    /// Control was requested and the trainer refused it, so the session only reads.
    ControlRefused {
        /// Trainer name.
        name: String,
    },
    /// [`crate::Session::save_settings`] wrote the file.
    SettingsSaved,
    /// [`crate::Session::note_settings_load`] saw a warning.
    SettingsFileDamaged {
        /// Parser or I/O warning, for the log.
        detail: String,
    },
    /// The dashboard thread reported a bind failure.
    DashboardFailed {
        /// Short reason from the dashboard.
        reason: String,
    },
    /// A remembered or chosen heart-rate strap reached `Connected`.
    HeartRateConnected {
        /// Strap name.
        name: String,
    },
    /// The strap link dropped after [`Self::HeartRateConnected`].
    HeartRateLost {
        /// Strap name.
        name: String,
    },
    /// The rider crossed the start line and finished this lap.
    LapCompleted {
        /// Lap number, starting at 1.
        lap: u32,
        /// Time for that lap, in seconds.
        time_s: f64,
    },
    /// [`crate::Session::set_course`] swapped in a new course.
    CourseSelected {
        /// [`crate::CourseChoice::label`] of the course now loaded.
        name: String,
    },
    /// A saved course could not be built, so the ride is Penguin Pass.
    CourseUnavailable {
        /// Plain-language reason from the route generator.
        detail: String,
    },
    /// [`crate::Session::end_ride`] wrote a FIT file.
    RideSaved {
        /// Absolute or relative path of the file.
        path: PathBuf,
    },
    /// The ride was too short to save, or the file could not be written.
    RideNotSaved {
        /// One sentence for a toast.
        reason: String,
    },
}

#[derive(Debug, Default)]
pub(crate) struct EventQueue {
    items: VecDeque<RideEvent>,
}

impl EventQueue {
    pub(crate) fn push(&mut self, event: RideEvent) {
        if self.items.len() >= EVENT_QUEUE_CAP {
            self.items.pop_front();
        }
        self.items.push_back(event);
    }

    pub(crate) fn drain(&mut self) -> Vec<RideEvent> {
        self.items.drain(..).collect()
    }

    pub(crate) fn clear(&mut self) {
        self.items.clear();
    }
}
