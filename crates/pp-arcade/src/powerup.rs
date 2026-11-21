//! One powerup slot and three short-lived physics changes.
//!
//! The rider's real power is never changed. Feather, Aero, and Burst alter
//! the simulated mass or drag area only. The session keeps the settings
//! profile, applies [`PhysicsModifiers`] to a copy, and passes that copy to
//! the bike model. Watts from the trainer stay the trainer's watts.
//!
//! For a 75 kg rider and a 9 kg bike (84 kg total, CdA 0.32 m², Crr 0.004)
//! at a steady 200 W, the equilibrium speeds are about:
//!
//! | | flat | 6% climb |
//! | --- | --- | --- |
//! | no powerup | 34.2 km/h | 13.0 km/h |
//! | Feather, −10 kg | 34.5 km/h | 14.5 km/h |
//! | Aero, drag area × 0.75 | 37.5 km/h | 13.2 km/h |
//! | Burst, drag area × 0.45 | 43.9 km/h | 13.4 km/h |
//!
//! None of them doubles the speed. Feather is the climb tool. Aero and Burst
//! are the flat and descent tools. The numbers are checked against
//! `pp_course::acceleration_mps2` in the tests.

use pp_core::RiderProfile;

use crate::rng::Rng;

/// How long Feather lasts after it is activated, in seconds.
pub const FEATHER_DURATION_S: f32 = 20.0;

/// Kilograms added to total mass while Feather is active.
///
/// Negative, so the bike is lighter. On a 6% climb at 200 W a 75 kg rider
/// goes from about 13.0 km/h to about 14.5 km/h. On the flat the same change
/// is about 0.2 km/h, from 34.2 km/h to 34.5 km/h. The rider's watts do not
/// change.
pub const FEATHER_MASS_DELTA_KG: f32 = -10.0;

/// How long Aero lasts after it is activated, in seconds.
pub const AERO_DURATION_S: f32 = 25.0;

/// Scale applied to drag area while Aero is active.
///
/// 0.75 means 25% less CdA. On the flat at 200 W a 75 kg rider goes from
/// about 34.2 km/h to about 37.5 km/h. On a 6% climb the same change is about
/// 0.2 km/h. The rider's watts do not change.
pub const AERO_CDA_SCALE: f32 = 0.75;

/// How long Burst lasts after it is activated, in seconds.
pub const BURST_DURATION_S: f32 = 8.0;

/// Scale applied to drag area while Burst is active.
///
/// 0.45 is a strong draft: a bit over half the drag area is gone. On the flat
/// at 200 W a 75 kg rider goes from about 34.2 km/h to about 43.9 km/h, which
/// is well under double. On a 6% climb the change is about 0.3 km/h. The
/// rider's watts do not change.
pub const BURST_CDA_SCALE: f32 = 0.45;

/// Grade, in percent, at and above which Feather is the likely award.
pub const CLIMB_GRADE_PCT: f32 = 4.0;

/// Grade, in percent, at and below which Aero and Burst are the likely awards.
pub const DESCENT_GRADE_PCT: f32 = -2.0;

/// Feather, Aero, and Burst weights out of 100 on a climb.
pub const CLIMB_WEIGHTS: (u32, u32, u32) = (70, 15, 15);

/// Feather, Aero, and Burst weights out of 100 on a descent.
pub const DESCENT_WEIGHTS: (u32, u32, u32) = (10, 50, 40);

/// Feather, Aero, and Burst weights out of 100 on the flat and on mild grades.
///
/// This band is everything between [`DESCENT_GRADE_PCT`] and [`CLIMB_GRADE_PCT`].
pub const ROLLING_WEIGHTS: (u32, u32, u32) = (25, 45, 30);

const _: () = {
    assert!(FEATHER_MASS_DELTA_KG < 0.0);
    assert!(AERO_CDA_SCALE > 0.0 && AERO_CDA_SCALE < 1.0);
    assert!(BURST_CDA_SCALE > 0.0 && BURST_CDA_SCALE < AERO_CDA_SCALE);
};

/// Which powerup is held or running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerupKind {
    /// Less total mass, for climbs.
    Feather,
    /// Less drag area, for flats and descents.
    Aero,
    /// A few seconds of draft.
    Burst,
}

impl PowerupKind {
    /// How long the effect runs after [`PowerupSlot::activate`], in seconds.
    #[must_use]
    pub const fn duration_s(self) -> f32 {
        match self {
            Self::Feather => FEATHER_DURATION_S,
            Self::Aero => AERO_DURATION_S,
            Self::Burst => BURST_DURATION_S,
        }
    }

    /// Mass and drag while this effect is running.
    #[must_use]
    pub const fn modifiers(self) -> PhysicsModifiers {
        match self {
            Self::Feather => PhysicsModifiers {
                mass_delta_kg: FEATHER_MASS_DELTA_KG,
                cda_scale: 1.0,
            },
            Self::Aero => PhysicsModifiers {
                mass_delta_kg: 0.0,
                cda_scale: AERO_CDA_SCALE,
            },
            Self::Burst => PhysicsModifiers {
                mass_delta_kg: 0.0,
                cda_scale: BURST_CDA_SCALE,
            },
        }
    }
}

impl std::fmt::Display for PowerupKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Feather => "Feather",
            Self::Aero => "Aero",
            Self::Burst => "Burst",
        })
    }
}

/// Why the session offered a powerup. The kind does not depend on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AwardReason {
    /// A combo of fish. The session decides the threshold, usually ten.
    Combo,
    /// A golden fish.
    GoldenFish,
    /// A timed segment was finished.
    Segment,
    /// A lap was completed.
    Lap,
}

impl std::fmt::Display for AwardReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Combo => "Combo",
            Self::GoldenFish => "Golden fish",
            Self::Segment => "Segment",
            Self::Lap => "Lap",
        })
    }
}

/// A change in the powerup slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerupEvent {
    /// The slot was empty and now holds `kind`.
    Awarded {
        /// What was granted.
        kind: PowerupKind,
        /// Why the session asked.
        reason: AwardReason,
    },
    /// The held powerup started running and the slot is empty again.
    Activated {
        /// What started.
        kind: PowerupKind,
    },
    /// The running effect reached the end of its time.
    Expired {
        /// What ended.
        kind: PowerupKind,
    },
}

/// What the session applies to a copy of the rider profile.
///
/// [`Self::IDENTITY`] is no change. `mass_delta_kg` is added to total mass.
/// `cda_scale` multiplies drag area. Power and FTP are not fields here: the
/// trainer's watts are used as measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsModifiers {
    /// Kilograms added to rider-plus-bike mass. Negative makes the bike lighter.
    pub mass_delta_kg: f32,
    /// Multiplier for `cda_m2`. `1.0` leaves drag area alone.
    pub cda_scale: f32,
}

impl PhysicsModifiers {
    /// No change to mass or drag.
    pub const IDENTITY: Self = Self {
        mass_delta_kg: 0.0,
        cda_scale: 1.0,
    };

    /// A copy of `rider` with this mass and drag change.
    ///
    /// The bike mass is kept when it still fits, and the rider mass absorbs
    /// the delta. Total mass stays at or above 1 kg so the force balance
    /// stays defined. A non-finite or non-positive drag scale is ignored.
    /// FTP, rolling resistance, and the watts the session passes in are
    /// copied unchanged.
    #[must_use]
    pub fn apply(self, mut rider: RiderProfile) -> RiderProfile {
        let delta = if self.mass_delta_kg.is_finite() {
            self.mass_delta_kg
        } else {
            0.0
        };
        let scale = if self.cda_scale.is_finite() && self.cda_scale > 0.0 {
            self.cda_scale
        } else {
            1.0
        };
        if delta == 0.0 && scale == 1.0 {
            return rider;
        }
        let bike = if rider.bike_mass_kg.is_finite() {
            rider.bike_mass_kg.max(0.0)
        } else {
            0.0
        };
        let rider_mass = if rider.rider_mass_kg.is_finite() {
            rider.rider_mass_kg.max(0.0)
        } else {
            0.0
        };
        let total = (rider_mass + bike + delta).max(1.0);
        let bike = bike.min(total);
        rider.bike_mass_kg = bike;
        rider.rider_mass_kg = total - bike;
        if rider.cda_m2.is_finite() {
            rider.cda_m2 *= scale;
            if rider.cda_m2 < 0.0 {
                rider.cda_m2 = 0.0;
            }
        }
        rider
    }
}

#[derive(Clone, Copy, Debug)]
struct Active {
    kind: PowerupKind,
    remaining_s: f32,
}

/// The single powerup slot, plus the effect that is running.
///
/// The slot holds one unactivated powerup. [`Self::award`] does nothing when
/// the slot is full. [`Self::activate`] empties the slot and starts the
/// effect, so another powerup can be awarded while one is running. Activating
/// during an effect replaces it and does not emit [`PowerupEvent::Expired`].
/// [`Self::modifiers`] is identity until an effect is running: holding a
/// powerup does not change the bike.
#[derive(Debug)]
pub struct PowerupSlot {
    rng: Rng,
    held: Option<PowerupKind>,
    active: Option<Active>,
}

impl PowerupSlot {
    /// A slot with nothing held and nothing running.
    ///
    /// `seed` chooses the kinds. The same seed and the same grades award the
    /// same sequence.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            held: None,
            active: None,
        }
    }

    /// Grant a powerup if the slot is empty.
    ///
    /// The kind is weighted by `grade_pct` (positive uphill). A non-finite
    /// grade is treated as flat. Returns `None` when the slot already holds
    /// a powerup; the held kind is left as it was. `reason` is stored on the
    /// event and does not change the kind.
    #[must_use]
    pub fn award(&mut self, reason: AwardReason, grade_pct: f32) -> Option<PowerupEvent> {
        if self.held.is_some() {
            return None;
        }
        let kind = pick_kind(&mut self.rng, grade_pct);
        self.held = Some(kind);
        Some(PowerupEvent::Awarded { kind, reason })
    }

    /// Start the held powerup.
    ///
    /// Returns `None` when the slot is empty. A running effect is replaced.
    #[must_use]
    pub fn activate(&mut self) -> Option<PowerupEvent> {
        let kind = self.held.take()?;
        self.active = Some(Active {
            kind,
            remaining_s: kind.duration_s(),
        });
        Some(PowerupEvent::Activated { kind })
    }

    /// Run the active effect down by `dt_s` seconds.
    ///
    /// A non-finite or non-positive `dt_s` changes nothing. Returns
    /// [`PowerupEvent::Expired`] when the timer reaches zero.
    #[must_use]
    pub fn tick(&mut self, dt_s: f32) -> Option<PowerupEvent> {
        if self.active.is_none() || !dt_s.is_finite() || dt_s <= 0.0 {
            return None;
        }
        let remaining = self.active.as_mut().map(|active| {
            active.remaining_s -= dt_s;
            active.remaining_s
        })?;
        if remaining > 0.0 {
            return None;
        }
        let kind = self.active.take()?.kind;
        Some(PowerupEvent::Expired { kind })
    }

    /// Mass and drag for this frame. Identity when nothing is running.
    #[must_use]
    pub fn modifiers(&self) -> PhysicsModifiers {
        self.active
            .map(|active| active.kind.modifiers())
            .unwrap_or(PhysicsModifiers::IDENTITY)
    }

    /// The powerup waiting in the slot.
    #[must_use]
    pub fn held(&self) -> Option<PowerupKind> {
        self.held
    }

    /// The effect that is running.
    #[must_use]
    pub fn active(&self) -> Option<PowerupKind> {
        self.active.map(|active| active.kind)
    }

    /// Seconds left on the running effect, or 0 when nothing is running.
    #[must_use]
    pub fn remaining_s(&self) -> f32 {
        self.active
            .map(|active| active.remaining_s.max(0.0))
            .unwrap_or(0.0)
    }
}

fn pick_kind(rng: &mut Rng, grade_pct: f32) -> PowerupKind {
    let (feather, aero, _burst) = weights(grade_pct);
    let roll = rng.index(100);
    if roll < feather {
        PowerupKind::Feather
    } else if roll < feather + aero {
        PowerupKind::Aero
    } else {
        PowerupKind::Burst
    }
}

fn weights(grade_pct: f32) -> (u32, u32, u32) {
    let grade = if grade_pct.is_finite() {
        grade_pct
    } else {
        0.0
    };
    if grade >= CLIMB_GRADE_PCT {
        CLIMB_WEIGHTS
    } else if grade <= DESCENT_GRADE_PCT {
        DESCENT_WEIGHTS
    } else {
        ROLLING_WEIGHTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pp_course::acceleration_mps2;

    fn award_kind(slot: &mut PowerupSlot, grade: f32) -> PowerupKind {
        match slot.award(AwardReason::Lap, grade) {
            Some(PowerupEvent::Awarded { kind, .. }) => kind,
            other => panic!("expected an award, got {other:?}"),
        }
    }

    #[test]
    fn durations_and_scales_sit_in_the_brief() {
        assert!((15.0..=30.0).contains(&FEATHER_DURATION_S));
        assert!((15.0..=30.0).contains(&AERO_DURATION_S));
        assert!((8.0..=10.0).contains(&BURST_DURATION_S));
        let (feather, aero, burst) = CLIMB_WEIGHTS;
        assert_eq!(feather + aero + burst, 100);
        assert!(feather > aero && feather > burst);
        let (feather, aero, burst) = DESCENT_WEIGHTS;
        assert_eq!(feather + aero + burst, 100);
        assert!(feather < aero && feather < burst);
        let (feather, aero, burst) = ROLLING_WEIGHTS;
        assert_eq!(feather + aero + burst, 100);
        assert!(aero >= feather);
    }

    #[test]
    fn one_slot_identity_until_activate_then_expiry() {
        let mut slot = PowerupSlot::new(7);
        assert_eq!(slot.modifiers(), PhysicsModifiers::IDENTITY);
        assert!(slot.activate().is_none());
        let awarded = slot.award(AwardReason::Combo, 8.0).expect("award");
        let PowerupEvent::Awarded { kind, reason } = awarded else {
            panic!("awarded");
        };
        assert_eq!(reason, AwardReason::Combo);
        assert!(slot.award(AwardReason::GoldenFish, 8.0).is_none());
        assert_eq!(slot.held(), Some(kind));
        assert_eq!(slot.modifiers(), PhysicsModifiers::IDENTITY);

        assert_eq!(slot.activate(), Some(PowerupEvent::Activated { kind }));
        assert!(slot.held().is_none());
        assert_eq!(slot.active(), Some(kind));
        assert_eq!(slot.modifiers(), kind.modifiers());
        assert!((slot.remaining_s() - kind.duration_s()).abs() < 1e-4);

        let half = kind.duration_s() * 0.5;
        assert!(slot.tick(half).is_none());
        assert!((slot.remaining_s() - half).abs() < 1e-3);
        assert!(slot.tick(0.0).is_none());
        assert!(slot.tick(-1.0).is_none());
        assert!(slot.tick(f32::NAN).is_none());
        assert!(slot.award(AwardReason::Lap, -5.0).is_some());
        assert!(slot.held().is_some());
        assert_eq!(slot.modifiers(), kind.modifiers());

        let left = slot.remaining_s();
        assert_eq!(slot.tick(left), Some(PowerupEvent::Expired { kind }));
        assert_eq!(slot.modifiers(), PhysicsModifiers::IDENTITY);
        assert_eq!(slot.remaining_s(), 0.0);
        assert!(slot.active().is_none());
        assert!(slot.held().is_some());
    }

    #[test]
    fn activating_again_replaces_the_running_effect() {
        let mut slot = PowerupSlot::new(3);
        let _ = slot.award(AwardReason::Segment, 10.0);
        let _ = slot.activate();
        let _ = slot.award(AwardReason::Lap, 10.0);
        let second = slot.held().expect("held");
        assert_eq!(
            slot.activate(),
            Some(PowerupEvent::Activated { kind: second })
        );
        assert_eq!(slot.active(), Some(second));
        assert!(slot.held().is_none());
        assert_eq!(slot.modifiers(), second.modifiers());
    }

    #[test]
    fn the_reason_does_not_change_the_kind() {
        let mut combo = PowerupSlot::new(42);
        let mut segment = PowerupSlot::new(42);
        assert_eq!(award_kind(&mut combo, 1.5), award_kind(&mut segment, 1.5));
        for seed in 0..40 {
            let mut finite = PowerupSlot::new(seed);
            let mut missing = PowerupSlot::new(seed);
            assert_eq!(
                award_kind(&mut finite, 0.0),
                award_kind(&mut missing, f32::NAN)
            );
        }
    }

    #[test]
    fn grades_weight_the_useful_powerup() {
        let climb = tally(8.0);
        assert!(climb[0] > 240, "feather on a climb: {climb:?}");
        let descent = tally(-6.0);
        assert!(descent[0] < 80, "feather on a descent: {descent:?}");
        assert!(descent[1] > descent[0], "{descent:?}");
        let flat = tally(0.0);
        assert!(flat[1] > flat[0], "aero on the flat: {flat:?}");
    }

    fn tally(grade: f32) -> [u32; 3] {
        let mut counts = [0; 3];
        for seed in 0..400 {
            let mut slot = PowerupSlot::new(seed);
            let index = match award_kind(&mut slot, grade) {
                PowerupKind::Feather => 0,
                PowerupKind::Aero => 1,
                PowerupKind::Burst => 2,
            };
            counts[index] += 1;
        }
        counts
    }

    fn equilibrium(rider: &RiderProfile, grade_pct: f32) -> f32 {
        let mut lo = 1.0_f32;
        let mut hi = 40.0_f32;
        if acceleration_mps2(lo, 200.0, grade_pct, rider) <= 0.0 {
            return 0.0;
        }
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if acceleration_mps2(mid, 200.0, grade_pct, rider) > 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    fn kph(rider: &RiderProfile, grade_pct: f32) -> f32 {
        equilibrium(rider, grade_pct) * 3.6
    }

    #[test]
    fn powerups_move_speed_in_the_expected_direction_and_size() {
        let base = RiderProfile::default();
        let flat = kph(&base, 0.0);
        let climb = kph(&base, 6.0);
        assert!((33.5..35.0).contains(&flat), "{flat}");
        assert!((12.5..13.5).contains(&climb), "{climb}");

        let feather_rider = PowerupKind::Feather.modifiers().apply(base);
        assert!((feather_rider.total_mass_kg() - 74.0).abs() < 1e-3);
        assert_eq!(feather_rider.ftp_w, base.ftp_w);
        assert_eq!(feather_rider.cda_m2, base.cda_m2);
        let feather_flat = kph(&feather_rider, 0.0);
        let feather_climb = kph(&feather_rider, 6.0);
        assert!(feather_climb > climb + 1.0, "{climb} -> {feather_climb}");
        assert!(feather_climb < climb * 1.25, "{feather_climb}");
        assert!(feather_flat - flat < 0.6, "{flat} -> {feather_flat}");
        assert!(feather_flat + 0.05 >= flat);

        let aero_rider = PowerupKind::Aero.modifiers().apply(base);
        assert!((aero_rider.cda_m2 - base.cda_m2 * AERO_CDA_SCALE).abs() < 1e-6);
        assert_eq!(aero_rider.total_mass_kg(), base.total_mass_kg());
        assert_eq!(aero_rider.ftp_w, 200);
        let aero_flat = kph(&aero_rider, 0.0);
        let aero_climb = kph(&aero_rider, 6.0);
        assert!(aero_flat > flat + 2.0, "{flat} -> {aero_flat}");
        assert!(aero_flat < flat * 1.2, "{aero_flat}");
        assert!(aero_climb - climb < 0.5, "{climb} -> {aero_climb}");

        let burst_rider = PowerupKind::Burst.modifiers().apply(base);
        assert!((burst_rider.cda_m2 - base.cda_m2 * BURST_CDA_SCALE).abs() < 1e-6);
        assert_eq!(burst_rider.ftp_w, base.ftp_w);
        let burst_flat = kph(&burst_rider, 0.0);
        let burst_climb = kph(&burst_rider, 6.0);
        assert!(burst_flat > flat + 7.0, "{flat} -> {burst_flat}");
        assert!(burst_flat < flat * 1.5, "{burst_flat}");
        assert!(burst_climb - climb < 1.0, "{climb} -> {burst_climb}");

        assert!(feather_climb - climb > feather_flat - flat);
        assert!(aero_flat - flat > aero_climb - climb);
        assert!(burst_flat - flat > aero_flat - flat);

        let untouched = PhysicsModifiers::IDENTITY.apply(base);
        assert_eq!(untouched, base);
        assert!((kph(&untouched, 0.0) - flat).abs() < 1e-3);

        let uphill = acceleration_mps2(3.5, 200.0, 6.0, &feather_rider);
        assert!(uphill > acceleration_mps2(3.5, 200.0, 6.0, &base));
        let drafted = acceleration_mps2(9.0, 200.0, 0.0, &burst_rider);
        let aero_a = acceleration_mps2(9.0, 200.0, 0.0, &aero_rider);
        assert!(drafted > aero_a);
        assert!(aero_a > acceleration_mps2(9.0, 200.0, 0.0, &base));

        let floored = PhysicsModifiers {
            mass_delta_kg: -10_000.0,
            cda_scale: -2.0,
        }
        .apply(base);
        assert!(floored.total_mass_kg() >= 1.0);
        assert_eq!(floored.cda_m2, base.cda_m2);
        assert_eq!(floored.ftp_w, base.ftp_w);
    }
}
