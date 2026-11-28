//! LAN companion dashboard for Penguin Pass.
//!
//! The game calls [`CastServer::publish`] once per frame with the latest
//! [`pp_core::Telemetry`]. A browser on the same network opens one of the
//! URLs from [`CastServer::urls`] and renders power, speed, and the lap
//! profile in couch-readable type. Optional status can also show a workout,
//! a timed segment, draft, fish, a powerup, and a reversed course.
//!
//! [`CastServer::start`] binds the socket and serves it on a background
//! thread. [`CastServer::publish`] only stores the newest snapshot, so a
//! 60 fps frame loop never waits on the network. Browsers are pushed a few
//! times a second, not once per frame.
//!
//! # Example
//!
//! ```no_run
//! use pp_cast::{CastConfig, CastServer};
//! use pp_core::Telemetry;
//!
//! let server = CastServer::start(CastConfig::default())?;
//! for url in server.urls() {
//!     println!("{url}");
//! }
//! server.publish(Telemetry {
//!     power_w: 210.0,
//!     ..Telemetry::default()
//! });
//! # Ok::<(), pp_cast::CastError>(())
//! ```

mod http;
mod server;
mod status;
mod zones;

pub use server::{CastError, CastServer, DEFAULT_PORT};
pub use status::{
    LinkPhase, PowerupStatus, ResistanceMode, RideSource, RideStatus, SegmentStatus, WorkoutStatus,
};
pub use zones::{GradeBand, ZoneStep, grade_bands, grade_color, power_zones, zone_for_power};

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// One sample of a course elevation profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProfilePoint {
    /// Distance from the start of the course, in meters.
    pub distance_m: f64,
    /// Elevation above the course datum, in meters.
    pub elevation_m: f32,
}

/// Elevation profile drawn on the dashboard.
///
/// The game fills this from its own course data. This crate does not depend
/// on a course crate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CourseProfile {
    /// Course name shown at the top of the dashboard.
    pub name: String,
    /// Course length, in meters.
    pub length_m: f64,
    /// Profile samples, ordered by increasing [`ProfilePoint::distance_m`].
    pub points: Vec<ProfilePoint>,
}

impl Default for CourseProfile {
    /// An unnamed course with no length and no samples.
    fn default() -> Self {
        Self {
            name: String::new(),
            length_m: 0.0,
            points: Vec::new(),
        }
    }
}

/// Settings for [`CastServer::start`].
#[derive(Debug, Clone, PartialEq)]
pub struct CastConfig {
    /// Local socket to bind. Port `0` asks the operating system for a free port.
    pub bind: SocketAddr,
    /// Course drawn on the dashboard. An empty profile still serves telemetry.
    pub course: CourseProfile,
}

impl Default for CastConfig {
    /// Bind `0.0.0.0:7878` and serve an empty course.
    fn default() -> Self {
        Self {
            bind: SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, DEFAULT_PORT)),
            course: CourseProfile::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn default_bind_is_all_interfaces_on_7878() {
        let config = CastConfig::default();
        assert_eq!(
            config.bind,
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, DEFAULT_PORT))
        );
        assert_eq!(config.course, CourseProfile::default());
        assert!(config.course.name.is_empty());
        assert!(config.course.points.is_empty());
    }
}
