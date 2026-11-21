//! Turn a workout target into the watts the session should send.
//!
//! This type does not write to a trainer. The session sends the returned
//! watts, and only when [`ErgCommand::send`] is set. A `None` target releases
//! ERG: the session goes back to the grade command it already uses.

/// Minimum time between two numeric target changes, in seconds.
pub const ERG_MIN_INTERVAL_S: f32 = 1.0;
/// How long a step-change eases toward the new target, in seconds.
pub const ERG_EASE_S: f32 = 3.0;
/// A jump larger than this, in watts, starts a new ease. Smaller moves are ramps and are only stepped.
pub const ERG_JUMP_W: f32 = 15.0;
/// Highest watts the helper will ask for. Page 49 tops out here too.
pub const ERG_MAX_W: u16 = 4000;
/// Seconds of not pedalling before the helper releases the target.
pub const ERG_STOP_S: f32 = 1.0;

/// One decision from [`ErgGuide::update`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErgCommand {
    /// Watts to hold. `None` means release ERG (free ride, the workout is over, or the rider has stopped pedalling).
    pub watts: Option<u16>,
    /// `true` when `watts` differs from the last command the helper told the session to send.
    ///
    /// A release, and the first number after a release, are sent at once. A
    /// change from one number to another waits until [`ERG_MIN_INTERVAL_S`]
    /// has passed.
    pub send: bool,
}

/// Rate limit, rounding, and a short ease after a step change.
///
/// Ramps are followed, then rounded to a whole watt and sent at most once a
/// second. A jump of more than [`ERG_JUMP_W`] eases from the previous command
/// (or from half the new target, when there is no previous command) up to the
/// target over [`ERG_EASE_S`]. When `pedaling` stays false for [`ERG_STOP_S`],
/// the helper releases the target so the trainer does not load a stopped
/// crank. Pedalling again starts a fresh ease from half the target.
#[derive(Debug, Clone)]
pub struct ErgGuide {
    clock_s: f32,
    last_sent: Option<u16>,
    last_send_s: f32,
    sent_once: bool,
    eased: Option<f32>,
    ease_from: f32,
    ease_to: f32,
    ease_left_s: f32,
    ease_total_s: f32,
    stopped_s: f32,
    tracking: Option<f32>,
}

impl Default for ErgGuide {
    fn default() -> Self {
        Self::new()
    }
}

impl ErgGuide {
    /// A guide that has not sent anything.
    #[must_use]
    pub fn new() -> Self {
        Self {
            clock_s: 0.0,
            last_sent: None,
            last_send_s: 0.0,
            sent_once: false,
            eased: None,
            ease_from: 0.0,
            ease_to: 0.0,
            ease_left_s: 0.0,
            ease_total_s: 0.0,
            stopped_s: 0.0,
            tracking: None,
        }
    }

    /// Decide what the trainer should be holding after this frame.
    ///
    /// `target_w` is [`crate::WorkoutRun::target_w`]. `pedaling` is false when
    /// the crank is stopped. A sensible rule: cadence is reported and under
    /// 5 rpm, or cadence is missing and power is under 15 W.
    ///
    /// A non-finite `dt_s` or a negative `dt_s` is treated as zero. A
    /// non-finite target releases ERG. Negative watts are treated as zero.
    #[must_use]
    pub fn update(&mut self, dt_s: f32, target_w: Option<f32>, pedaling: bool) -> ErgCommand {
        let dt = if dt_s.is_finite() && dt_s > 0.0 {
            dt_s
        } else {
            0.0
        };
        self.clock_s += dt;
        if pedaling {
            self.stopped_s = 0.0;
        } else {
            self.stopped_s += dt;
        }
        let target = target_w
            .filter(|watts| watts.is_finite())
            .map(|watts| watts.max(0.0));
        let released = target.is_none() || self.stopped_s >= ERG_STOP_S;
        if released {
            self.eased = None;
            self.tracking = None;
            self.ease_left_s = 0.0;
            return self.publish(None);
        }
        let target = target.unwrap_or(0.0);
        self.follow(dt, target);
        self.publish(self.eased)
    }

    fn follow(&mut self, dt: f32, target: f32) {
        let jump = match self.tracking {
            None => true,
            Some(previous) => (target - previous).abs() > ERG_JUMP_W,
        };
        self.tracking = Some(target);
        if jump {
            self.ease_from = self.eased.unwrap_or(target * 0.5);
            self.ease_to = target;
            self.ease_total_s = ERG_EASE_S;
            self.ease_left_s = ERG_EASE_S;
            self.eased = Some(self.ease_from);
        }
        if self.ease_left_s > 0.0 {
            let step = dt.min(self.ease_left_s);
            self.ease_left_s -= step;
            let progress = if self.ease_total_s <= 0.0 {
                1.0
            } else {
                1.0 - self.ease_left_s / self.ease_total_s
            };
            self.eased = Some(self.ease_from + (self.ease_to - self.ease_from) * progress);
        } else {
            self.eased = Some(target);
        }
    }

    fn publish(&mut self, watts: Option<f32>) -> ErgCommand {
        let rounded = watts.map(round_watts);
        if self.sent_once && rounded == self.last_sent {
            return ErgCommand {
                watts: self.last_sent,
                send: false,
            };
        }
        let due = !self.sent_once || self.clock_s - self.last_send_s >= ERG_MIN_INTERVAL_S;
        // A release, and the first number after a release, are not delayed.
        // The one-second gap is only between two numeric targets.
        let crosses_release = rounded.is_none() || self.last_sent.is_none();
        if crosses_release || due {
            self.last_sent = rounded;
            self.last_send_s = self.clock_s;
            self.sent_once = true;
            ErgCommand {
                watts: rounded,
                send: true,
            }
        } else {
            ErgCommand {
                watts: self.last_sent,
                send: false,
            }
        }
    }
}

fn round_watts(watts: f32) -> u16 {
    let clamped = watts.clamp(0.0, f32::from(ERG_MAX_W));
    let rounded = clamped.round();
    if rounded >= f32::from(ERG_MAX_W) {
        ERG_MAX_W
    } else if rounded <= 0.0 {
        0
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            rounded as u16
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_command_is_half_and_then_the_rate_limit_holds() {
        let mut guide = ErgGuide::new();
        let first = guide.update(0.0, Some(200.0), true);
        assert!(first.send);
        assert_eq!(first.watts, Some(100));

        let held = guide.update(0.5, Some(200.0), true);
        assert!(!held.send);
        assert_eq!(held.watts, Some(100));

        let next = guide.update(0.5, Some(200.0), true);
        assert!(next.send);
        // One second into a three-second ease from 100 W to 200 W.
        assert_eq!(next.watts, Some(133));
    }

    #[test]
    fn rounding_lands_on_the_nearest_watt_after_the_ease() {
        let mut low = ErgGuide::new();
        let mut high = ErgGuide::new();
        let mut low_sent = None;
        let mut high_sent = None;
        for _ in 0..4 {
            low_sent = Some(low.update(1.0, Some(200.4), true));
            high_sent = Some(high.update(1.0, Some(200.6), true));
        }
        assert_eq!(low_sent.unwrap().watts, Some(200));
        assert_eq!(high_sent.unwrap().watts, Some(201));
    }

    #[test]
    fn a_small_ramp_sends_at_most_once_per_second() {
        let mut guide = ErgGuide::new();
        let _ = guide.update(0.0, Some(200.0), true);
        // Finish the ease with three one-second steps, then creep by 2 W.
        let _ = guide.update(1.0, Some(200.0), true);
        let _ = guide.update(1.0, Some(200.0), true);
        let settled = guide.update(1.0, Some(200.0), true);
        assert_eq!(settled.watts, Some(200));
        assert!(settled.send);

        let mut sends = 0;
        let mut target = 200.0_f32;
        for _ in 0..4 {
            target += 2.0;
            let command = guide.update(0.25, Some(target), true);
            if command.send {
                sends += 1;
            } else {
                assert_eq!(command.watts, Some(200));
            }
        }
        assert_eq!(sends, 1);
    }

    #[test]
    fn stopping_releases_immediately_and_starting_eases_again() {
        let mut guide = ErgGuide::new();
        let _ = guide.update(3.0, Some(180.0), true);
        let brief = guide.update(0.4, Some(180.0), false);
        assert_eq!(brief.watts, Some(180));
        assert!(!brief.send);
        let released = guide.update(0.6, Some(180.0), false);
        assert_eq!(released.watts, None);
        assert!(released.send);
        let again = guide.update(0.0, Some(180.0), true);
        assert!(again.send);
        assert_eq!(again.watts, Some(90));
    }

    #[test]
    fn a_missing_target_releases_and_a_huge_target_clamps() {
        let mut guide = ErgGuide::new();
        let _ = guide.update(0.0, Some(100.0), true);
        let gone = guide.update(0.0, None, true);
        assert_eq!(gone.watts, None);
        assert!(gone.send);
        let huge = guide.update(0.0, Some(50_000.0), true);
        assert!(huge.send);
        assert!(huge.watts.unwrap() <= ERG_MAX_W);
        let bad = guide.update(1.0, Some(f32::NAN), true);
        assert_eq!(bad.watts, None);
    }
}
