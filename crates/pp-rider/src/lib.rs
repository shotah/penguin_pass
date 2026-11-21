//! Procedural road bike and rider for Penguin Pass.
//!
//! The crate has no engine dependency. [`RiderModel::build`] generates the
//! bike and the rider as rigid, named parts of plain mesh data, and
//! [`RiderModel::pose`] gives each part a transform for a crank angle, with
//! the feet locked to the pedals and the hands to the bars.
//! [`RideAnimator`] turns ride telemetry (distance, cadence, lean, grade)
//! into that pose input frame by frame.
//!
//! # Conventions
//!
//! - Units are meters and radians.
//! - Right-handed, Y up. The bike faces -Z. The rider's left is -X.
//! - The model origin is on the road under the bottom bracket. Both tyres
//!   touch `y = 0`.
//! - Triangles are counter-clockwise seen from outside, as in glTF. Godot's
//!   `ArrayMesh` wants the opposite; see [`Surface::indices_clockwise`].
//! - Colours are sRGB.
//!
//! # Example
//!
//! ```
//! use pp_rider::{PartId, PoseInput, RiderModel, RiderStyle};
//!
//! let model = RiderModel::build(&RiderStyle::default());
//! assert_eq!(model.parts().len(), PartId::COUNT);
//!
//! let pose = model.pose(&PoseInput {
//!     crank_angle_rad: 1.0,
//!     wheel_angle_rad: 3.0,
//!     effort: 0.5,
//!     ..PoseInput::default()
//! });
//! for (part, transform) in model.parts().iter().zip(&pose.transforms) {
//!     // Hand `part.surfaces()` to the renderer once, and `transform`
//!     // to the part's node every frame.
//!     let _ = (part.name(), transform.to_cols());
//! }
//! ```

mod animate;
mod bike;
mod body;
#[cfg(test)]
mod check;
mod geo;
mod mesh;
mod model;
mod paint;
mod pose;
mod rig;
mod style;
#[cfg(test)]
mod tests;

pub use animate::{RideAnimator, RideSample};
pub use mesh::{Material, Part, PartId, Surface};
pub use model::RiderModel;
pub use pose::{Anchors, MAX_STEER_RAD, PartTransform, Pose, PoseInput};
pub use style::{RiderStyle, StyleColor};

/// Outer radius of a wheel with its tyre, in meters. Divide the distance
/// travelled by this to get [`PoseInput::wheel_angle_rad`].
pub const WHEEL_RADIUS_M: f32 = rig::WHEEL_RADIUS;

/// Crank length, from the bottom bracket to the pedal axle, in meters.
pub const CRANK_LENGTH_M: f32 = rig::CRANK_LENGTH;
