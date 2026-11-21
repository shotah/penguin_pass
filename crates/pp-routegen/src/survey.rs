//! Measurements of a parsed course, for reports and tests.

use pp_course::Course;

use crate::geom::{self, angle_diff};

/// Numbers that describe a generated loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteSurvey {
    /// 3D centerline length, in meters.
    pub length_m: f64,
    /// Total ascent, in meters.
    pub ascent_m: f64,
    /// Steepest uphill grade in the sample, in percent.
    pub max_grade_pct: f64,
    /// Steepest downhill grade in the sample, in percent (negative or zero).
    pub min_grade_pct: f64,
    /// Smallest horizontal radius estimated from heading change, in meters.
    pub min_radius_m: f64,
    /// East-west extent, in meters.
    pub footprint_east_m: f64,
    /// North-south extent, in meters.
    pub footprint_north_m: f64,
    /// Closest approach of non-consecutive centerline samples, in meters.
    pub closest_approach_m: f64,
    /// Lowest elevation in the sample, in meters.
    pub min_elevation_m: f64,
    /// Highest elevation in the sample, in meters.
    pub max_elevation_m: f64,
}

/// Sample `course` every `step_m` meters of 3D centerline.
#[must_use]
pub fn survey(course: &Course, step_m: f64) -> RouteSurvey {
    let length = course.length_m();
    let step = if step_m.is_finite() && step_m > 0.0 {
        step_m
    } else {
        5.0
    };
    let n = ((length / step).round() as usize).max(8);
    let mut min_g = f64::MAX;
    let mut max_g = f64::MIN;
    let mut min_e = f64::MAX;
    let mut max_e = f64::MIN;
    let mut min_radius = f64::INFINITY;
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let d0 = length * (i as f64) / (n as f64);
        let d1 = length * ((i + 1) as f64) / (n as f64);
        let g = f64::from(course.grade_pct_at(d0));
        let e = f64::from(course.elevation_at(d0));
        min_g = min_g.min(g);
        max_g = max_g.max(g);
        min_e = min_e.min(e);
        max_e = max_e.max(e);
        let p = course.position_at(d0);
        let x = f64::from(p[0]);
        let z = f64::from(p[2]);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
        pts.push((x, z));
        let t0 = course.tangent_at(d0);
        let t1 = course.tangent_at(d1);
        let h0 = f64::from(t0[2]).atan2(f64::from(t0[0]));
        let h1 = f64::from(t1[2]).atan2(f64::from(t1[0]));
        let dh = angle_diff(h1, h0).abs();
        let p1 = course.position_at(d1);
        let horiz = (f64::from(p1[0] - p[0])).hypot(f64::from(p1[2] - p[2]));
        if dh > 1.0e-4 && horiz > 1.0e-3 {
            min_radius = min_radius.min(horiz / dh);
        }
    }
    let along = length / n as f64;
    let closest = geom::closest_approach(&pts, along, length, crate::spec::CLEARANCE_WINDOW_M);
    RouteSurvey {
        length_m: length,
        ascent_m: f64::from(course.total_ascent_m()),
        max_grade_pct: max_g,
        min_grade_pct: min_g,
        min_radius_m: min_radius,
        footprint_east_m: max_x - min_x,
        footprint_north_m: max_z - min_z,
        closest_approach_m: closest,
        min_elevation_m: min_e,
        max_elevation_m: max_e,
    }
}
