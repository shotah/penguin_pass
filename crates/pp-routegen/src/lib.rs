//! Seeded route generator for Penguin Pass.
//!
//! [`generate`] turns a [`RouteSpec`] (seed, distance, terrain, name) into a
//! closed course in the `pp-course` JSON format. [`EndlessRoute`] is an open
//! road for a future endless mode; `pp-course` cannot store one yet.
//!
//! The mapping from spec to JSON is a compatibility promise: the same spec
//! always produces the same bytes. Random draws come from ChaCha12
//! (`rand_chacha` 0.3.1 seeded through `rand` 0.8.5). See
//! `docs/route-generator.md`.
//!
//! ```
//! use pp_routegen::{RouteSpec, Terrain, TerrainPreset, generate};
//!
//! let spec = RouteSpec::new("Sample", 1, 5_000.0, Terrain::Preset(TerrainPreset::Rolling));
//! let route = generate(&spec).expect("sample route");
//! assert!((route.course().length_m() - 5_000.0).abs() < 1.0);
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod endless;
mod error;
mod generate;
mod geom;
mod grade;
mod plan;
mod rng;
mod spec;
mod survey;
mod svg;

pub use endless::EndlessRoute;
pub use error::RouteGenError;
pub use generate::{GeneratedRoute, generate};
pub use rng::fingerprint;
pub use spec::{
    CENTERLINE_CLEARANCE_M, CLEARANCE_WINDOW_M, MAX_ATTEMPTS, MAX_DISTANCE_M, MAX_ELEVATION_M,
    MAX_FOOTPRINT_FRACTION, MIN_DISTANCE_M, MIN_FOOTPRINT_FRACTION, RouteSpec, SpecError, Terrain,
    TerrainParams, TerrainPreset, ascent_tolerance_m,
};
pub use survey::RouteSurvey;
pub use svg::{render_contact_sheet, render_svg};

/// Measure a parsed course for reports and tests.
///
/// `step_m` is the sample spacing in meters along the 3D centerline.
#[must_use]
pub fn survey(course: &pp_course::Course, step_m: f64) -> RouteSurvey {
    survey::survey(course, step_m)
}
