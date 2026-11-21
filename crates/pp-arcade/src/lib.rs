//! Fish, powerups, and the rider profile for Penguin Pass.
//!
//! The rider's real power is never changed. [`PhysicsModifiers`] change the
//! simulated mass and drag only, and the session applies them to a copy of
//! the rider profile before it integrates the bike. See `docs/arcade.md`.
//!
//! ```
//! use pp_arcade::{AwardReason, FishField, PhysicsModifiers, PowerupSlot};
//!
//! let field = FishField::generate(10_000.0, |_| 0.0, 1, 5);
//! assert!(!field.is_empty());
//! let mut power = PowerupSlot::new(1);
//! assert!(power.award(AwardReason::Lap, 8.0).is_some());
//! assert_eq!(power.modifiers(), PhysicsModifiers::IDENTITY);
//! assert!(power.activate().is_some());
//! assert_ne!(power.modifiers(), PhysicsModifiers::IDENTITY);
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod fish;
mod powerup;
mod profile;
mod rng;

pub use fish::{
    COLLECT_HALF_SLOT, END_CLEAR_M, FISH_SPACING_MAX_M, FISH_SPACING_MIN_M, Fish, FishAhead,
    FishEvent, FishField, FishGroup, FishRun, GROUP_GAP_MAX_M, GROUP_GAP_MIN_M,
    LANE_CHANGE_SPEED_MPS, LANE_EASE_S, METERS_PER_SLOT, MODERATE_GRADE_PCT, Pattern,
    START_CLEAR_M, STEEP_GRADE_PCT,
};
pub use powerup::{
    AERO_CDA_SCALE, AERO_DURATION_S, AwardReason, BURST_CDA_SCALE, BURST_DURATION_S,
    CLIMB_GRADE_PCT, CLIMB_WEIGHTS, DESCENT_GRADE_PCT, DESCENT_WEIGHTS, FEATHER_DURATION_S,
    FEATHER_MASS_DELTA_KG, PhysicsModifiers, PowerupEvent, PowerupKind, PowerupSlot,
    ROLLING_WEIGHTS,
};
pub use profile::{
    Badge, BadgeId, BadgeUnit, COURSES, EarnedBadge, HOUR_RIDE_S, KIT_GLACIER, KIT_KOM_DOTS,
    KIT_SUNRISE, LocalDate, MIN_RIDE_S, Profile, ProfileError, RideOutcome, STREAK_RIDE_S,
};
