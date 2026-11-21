//! Computer-controlled riders for Penguin Pass.
//!
//! [`Peloton`] rides a handful of bots with the same bike model as the
//! player. Pace riders hold a fixed effort and lap the course. Company
//! riders follow a smoothed reading of the player's power, so the road
//! nearby stays populated without locking anyone to the player's speed.
//! The crate does not link Godot, Bluetooth, or the network. See
//! `docs/bots.md` for the calls the session makes and the numbers behind
//! the draft.
//!
//! ```
//! use pp_bots::{BotConfig, Peloton, PlayerInput};
//! use pp_course::Course;
//!
//! let course = Course::penguin_pass();
//! let mut peloton = Peloton::new(&course, 1, BotConfig::default());
//! assert!(peloton.riders().len() > 1);
//! let player = PlayerInput {
//!     total_distance_m: 0.0,
//!     speed_mps: 0.0,
//!     power_w: 180.0,
//!     slot: 2.0,
//!     mass_kg: 75.0,
//! };
//! let _events = peloton.tick(1.0 / 60.0, &player);
//! let factor = peloton.draft_factor(&player);
//! assert!((0.7..=1.0).contains(&factor));
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod config;
mod draft;
mod lane;
mod peloton;
mod rider;
mod rng;
mod types;

pub use config::BotConfig;
pub use draft::{DRAFT_CDA, DRAFT_FAR_M, DRAFT_NEAR_M, DRAFT_SLOT_FADE, draft};
pub use lane::{BIKE_LENGTH_M, MIN_SAME_SLOT_M, SLOT_COUNT};
pub use peloton::Peloton;
pub use types::{BotEvent, BotState, PlayerInput};

#[cfg(test)]
mod tests;
