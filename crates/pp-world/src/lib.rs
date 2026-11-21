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

mod error;
mod grid;
mod ground;
mod heightfield;
mod planner;
mod route;
mod seed;

pub use error::WorldError;
pub use grid::{ChunkBounds, ChunkCoord, ChunkGrid};
pub use ground::{GroundPlan, GroundVertex, PlannedChunk, PlannedFar, SKIRT_M, Skirt, plan};
pub use heightfield::Field;
pub use planner::{ChunkDelta, ChunkPlanner, PlannerConfig};
pub use route::{Route, StraightRoute};
pub use seed::chunk_seed;
