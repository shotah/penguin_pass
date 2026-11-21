//! Bounded toast queue.

use std::collections::VecDeque;
use std::path::PathBuf;

use pp_arcade::PowerupKind;

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
    /// The rider crossed a segment start line.
    SegmentEntered {
        /// Segment name, for a banner.
        name: String,
    },
    /// The rider finished a segment they had entered.
    SegmentFinished {
        /// Segment name.
        name: String,
        /// Finish time, in seconds.
        time_s: f64,
        /// Best time before this finish. `None` on the first effort.
        previous_best_s: Option<f64>,
        /// `true` when this finish is strictly faster than the stored best.
        ///
        /// A finish ridden while a powerup effect was active is not a record.
        record: bool,
    },
    /// The rider collected a fish.
    FishCollected {
        /// The fish is golden.
        golden: bool,
        /// Consecutive collects, including this one.
        combo: u32,
    },
    /// The rider passed a fish outside its slot. The combo resets.
    FishMissed,
    /// An empty slot received a powerup.
    PowerupAwarded {
        /// What is now held.
        kind: PowerupKind,
    },
    /// The rider started the held powerup.
    PowerupActivated {
        /// What started running.
        kind: PowerupKind,
    },
    /// A running powerup reached the end of its time.
    PowerupExpired {
        /// What ended.
        kind: PowerupKind,
    },
    /// This ride earned a badge the profile did not already have.
    BadgeEarned {
        /// Badge name.
        name: String,
        /// Kit unlocked with the badge, when it has one.
        kit: Option<String>,
    },
    /// The first lap beat the stored ghost.
    GhostBeaten {
        /// This lap, in seconds.
        time_s: f64,
        /// The stored ghost's lap, in seconds.
        previous_s: Option<f64>,
    },
    /// A workout step became the current one.
    WorkoutStepStarted {
        /// Short label, such as `Sweet spot 2 of 3`.
        label: String,
        /// Target at the start of the step, in watts. `None` when the step is open.
        target_w: Option<f32>,
    },
    /// The current step has a few seconds left.
    WorkoutStepEndingSoon {
        /// Seconds remaining in the warning.
        seconds: u8,
    },
    /// A workout cue fired.
    WorkoutCue {
        /// Cue text.
        text: String,
    },
    /// The workout plan ended.
    WorkoutFinished {
        /// Ramp-test FTP estimate, in watts, when this plan was a ramp test and a scored minute finished.
        ramp_test_ftp_w: Option<u16>,
    },
    /// Strava login finished.
    StravaConnected {
        /// Athlete name.
        athlete: String,
    },
    /// The Strava login stopped without a saved connection.
    StravaConnectFailed {
        /// A short reason safe to show.
        reason: String,
    },
    /// A FIT upload has started.
    StravaUploadStarted,
    /// Strava accepted the ride.
    StravaUploaded {
        /// Strava activity id.
        activity_id: u64,
        /// Activity page.
        url: String,
    },
    /// The upload did not finish.
    StravaUploadFailed {
        /// A short reason safe to show.
        reason: String,
        /// Strava reported this file as a duplicate.
        duplicate: bool,
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
