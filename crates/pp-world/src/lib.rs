//! Chunk streaming for the Penguin Pass world.
//!
//! [`ChunkPlanner`] decides which square chunks of the ground plane should
//! exist around the rider. It does not build meshes, sample noise, or depend
//! on Godot. The Godot side turns a [`ChunkCoord`] plus [`chunk_seed`] into
//! terrain and scatters, and frees a chunk when the planner unloads it.
//!
//! Distances are meters. The ground plane is X east and Z north, matching
//! [`pp_course::Course`]. See `docs/world-chunks.md`.
//!
//! ```
//! use pp_course::Course;
//! use pp_world::{chunk_seed, ChunkPlanner};
//!
//! let course = Course::penguin_pass();
//! let mut planner = ChunkPlanner::default();
//! let world_seed = 7_u64;
//! for coord in planner.prime(&course, 0.0) {
//!     let _terrain = chunk_seed(world_seed, coord, 0);
//! }
//! let delta = planner.update(&course, 1.0);
//! assert!(delta.load.is_empty());
//! assert_eq!(planner.pending_count(), 0);
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod atmosphere;
mod error;
mod grid;
mod ground;
mod heightfield;
mod landmarks;
mod planner;
mod route;
mod seed;

pub use atmosphere::{
    AUTO_DAY_SECONDS, AUTO_START_HOUR, AtmosphereState, DAY_HOUR, PrecipKind, TimeOfDay, Weather,
    WeatherMix, atmosphere_at, hour_at, mix_atmosphere, resolve_weather, ride_atmosphere,
};
pub use error::WorldError;
pub use grid::{ChunkBounds, ChunkCoord, ChunkGrid};
pub use ground::{GroundPlan, GroundVertex, PlannedChunk, PlannedFar, SKIRT_M, Skirt, plan};
pub use heightfield::Field;
pub use landmarks::{
    GroundQuery, LandmarkRequest, PlacedLandmark, ScatterSite, SiteKind, horizon_anchor,
    horizon_bearing, horizon_in_view, keeps_scatter, place_landmarks,
};
pub use planner::{ChunkDelta, ChunkPlanner, PlannerConfig};
pub use route::{Route, StraightRoute};
pub use seed::chunk_seed;
