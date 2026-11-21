//! Plan geometry shared with `pp-course`: lines, arcs, and JSON.
//!
//! Arc sampling matches `sample_arc` in `pp-course` so a path that closes
//! here still closes after `Course::from_str`.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// What a segment is for, when the grade profile is laid on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Start or finish straight. Grade stays at 0%.
    Flat,
    /// Switchback climb.
    Climb,
    /// Ridge between the climb and the descent.
    Crest,
    /// Descent, including its hairpins.
    Descent,
    /// Ordinary road with no special grade slot.
    Valley,
}

/// One centerline piece.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    /// Straight, in meters.
    Line {
        /// Length, in meters.
        length: f64,
        /// Grade-profile role.
        role: Role,
    },
    /// Circular arc.
    Arc {
        /// Radius, in meters.
        radius: f64,
        /// Signed sweep, in radians. Positive is left.
        angle: f64,
        /// Grade-profile role.
        role: Role,
    },
}

impl Seg {
    /// Straight of `length` meters.
    #[must_use]
    pub fn line(length: f64, role: Role) -> Self {
        Self::Line { length, role }
    }

    /// Arc of `radius` meters sweeping `angle` radians.
    #[must_use]
    pub fn arc(radius: f64, angle: f64, role: Role) -> Self {
        Self::Arc {
            radius,
            angle,
            role,
        }
    }

    /// Horizontal length, in meters.
    #[must_use]
    pub fn length(&self) -> f64 {
        match *self {
            Self::Line { length, .. } => length,
            Self::Arc { radius, angle, .. } => radius * angle.abs(),
        }
    }

    /// Role used by the grade profile.
    #[must_use]
    pub fn role(self) -> Role {
        match self {
            Self::Line { role, .. } | Self::Arc { role, .. } => role,
        }
    }

    /// Arc radius, in meters, if this is an arc.
    #[must_use]
    pub fn radius(&self) -> Option<f64> {
        match *self {
            Self::Arc { radius, .. } => Some(radius),
            Self::Line { .. } => None,
        }
    }

    /// Signed arc angle, in radians, if this is an arc.
    #[must_use]
    pub fn angle(&self) -> Option<f64> {
        match *self {
            Self::Arc { angle, .. } => Some(angle),
            Self::Line { .. } => None,
        }
    }

    /// Replace the length of a straight, in meters. Arcs are left unchanged.
    pub(crate) fn set_line_length(&mut self, length: f64) {
        if let Self::Line { length: slot, .. } = self {
            *slot = length;
        }
    }

    /// Length of a straight, in meters.
    #[must_use]
    pub(crate) fn line_length(&self) -> Option<f64> {
        match *self {
            Self::Line { length, .. } => Some(length),
            Self::Arc { .. } => None,
        }
    }
}

/// Sum of horizontal lengths, in meters.
#[must_use]
pub fn horiz(segs: &[Seg]) -> f64 {
    segs.iter().map(Seg::length).sum()
}

/// Scale every length and radius by `k`. Angles stay put, so a closed
/// path stays closed.
pub fn scale(segs: &mut [Seg], k: f64) {
    for seg in segs {
        match seg {
            Seg::Line { length, .. } => *length *= k,
            Seg::Arc { radius, .. } => *radius *= k,
        }
    }
}

/// Smallest arc radius, in meters. Lines do not count.
#[must_use]
pub fn min_radius(segs: &[Seg]) -> f64 {
    segs.iter()
        .filter_map(Seg::radius)
        .fold(f64::INFINITY, f64::min)
}

/// Point and heading `distance` meters along a circular arc.
///
/// `angle` is the full signed sweep. `distance` runs from 0 to
/// `radius * angle.abs()`. This is the same construction `pp-course` uses.
#[must_use]
pub fn sample_arc(
    x: f64,
    z: f64,
    heading: f64,
    radius: f64,
    angle: f64,
    distance: f64,
) -> (f64, f64, f64) {
    let dir = if angle >= 0.0 { 1.0 } else { -1.0 };
    let travelled = dir * distance / radius;
    let normal = heading + dir * FRAC_PI_2;
    let cx = x + radius * normal.cos();
    let cz = z + radius * normal.sin();
    let start_from_center = heading - dir * FRAC_PI_2;
    let a = start_from_center + travelled;
    (
        cx + radius * a.cos(),
        cz + radius * a.sin(),
        heading + travelled,
    )
}

/// Signed shortest difference `a - b`, in radians, in `(-π, π]`.
#[must_use]
pub fn angle_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(TAU);
    if d > PI { d - TAU } else { d }
}

/// End pose of `segs` starting at the origin with heading `h0`.
///
/// Returns `(x_m, z_m, heading_rad)`.
#[must_use]
pub fn end_pose(segs: &[Seg], h0: f64) -> (f64, f64, f64) {
    let mut x = 0.0;
    let mut z = 0.0;
    let mut h = h0;
    for seg in segs {
        match *seg {
            Seg::Line { length, .. } => {
                x += length * h.cos();
                z += length * h.sin();
            }
            Seg::Arc { radius, angle, .. } => {
                let (nx, nz, nh) = sample_arc(x, z, h, radius, angle, radius * angle.abs());
                x = nx;
                z = nz;
                h = nh;
            }
        }
    }
    (x, z, h)
}

/// Heading at the start of segment `index`.
#[must_use]
pub fn heading_at(segs: &[Seg], index: usize, h0: f64) -> f64 {
    let mut h = h0;
    for seg in segs.iter().take(index) {
        if let Seg::Arc { angle, .. } = seg {
            h += *angle;
        }
    }
    h
}

/// Pose at horizontal distance `s_m` from the start of the path.
#[must_use]
pub fn pose_at(segs: &[Seg], h0: f64, s_m: f64) -> (f64, f64, f64) {
    let total = horiz(segs);
    let s = if s_m.is_finite() {
        s_m.clamp(0.0, total)
    } else {
        0.0
    };
    let mut x = 0.0;
    let mut z = 0.0;
    let mut h = h0;
    let mut cursor = 0.0;
    for seg in segs {
        let len = seg.length();
        if cursor + len + 1.0e-9 >= s {
            let d = (s - cursor).clamp(0.0, len);
            return match *seg {
                Seg::Line { .. } => (x + d * h.cos(), z + d * h.sin(), h),
                Seg::Arc { radius, angle, .. } => sample_arc(x, z, h, radius, angle, d),
            };
        }
        match *seg {
            Seg::Line { length, .. } => {
                x += length * h.cos();
                z += length * h.sin();
            }
            Seg::Arc { radius, angle, .. } => {
                let (nx, nz, nh) = sample_arc(x, z, h, radius, angle, radius * angle.abs());
                x = nx;
                z = nz;
                h = nh;
            }
        }
        cursor += len;
    }
    (x, z, h)
}

/// Uniform samples of the horizontal centerline, one every `step_m`.
#[must_use]
pub fn sample_xy(segs: &[Seg], h0: f64, step_m: f64) -> Vec<(f64, f64)> {
    let total = horiz(segs);
    if total <= 0.0 || !step_m.is_finite() || step_m <= 0.0 {
        return vec![(0.0, 0.0)];
    }
    let n = ((total / step_m).round() as usize).max(8);
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let (x, z, _) = pose_at(segs, h0, total * (i as f64) / (n as f64));
        pts.push((x, z));
    }
    pts
}

/// Gap, in meters, and absolute heading error, in radians, versus the start.
#[must_use]
pub fn closure_error(segs: &[Seg], h0: f64) -> (f64, f64) {
    let (x, z, h) = end_pose(segs, h0);
    (x.hypot(z), angle_diff(h, h0).abs())
}

/// Brute-force minimum distance between samples whose along-track separation
/// is at least `window_m`, in meters.
///
/// Use this when the answer itself matters. [`closest_approach`] is the fast
/// check for "is anything nearer than about 50 m?".
#[must_use]
pub fn closest_approach_brute(
    points: &[(f64, f64)],
    step_m: f64,
    length_m: f64,
    window_m: f64,
) -> f64 {
    let mut best = f64::INFINITY;
    for i in 0..points.len() {
        for j in (i + 1)..points.len() {
            let along = (j - i) as f64 * step_m;
            let along = along.min(length_m - along);
            if along < window_m {
                continue;
            }
            let d = (points[i].0 - points[j].0).hypot(points[i].1 - points[j].1);
            if d < best {
                best = d;
            }
        }
    }
    best
}

/// Minimum distance between samples whose along-track separation is at
/// least `window_m`, in meters, ignoring pairs farther apart than about 50 m.
///
/// `step_m` is the spacing used to build `points` along a loop of length
/// `length_m`. The search is a grid, so it is linear in the sample count.
/// Returns infinity when nothing inside the grid neighbourhood is that close.
#[must_use]
pub fn closest_approach(points: &[(f64, f64)], step_m: f64, length_m: f64, window_m: f64) -> f64 {
    if points.len() < 2 {
        return f64::INFINITY;
    }
    let cell = 24.0_f64;
    let mut grid: std::collections::HashMap<(i32, i32), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, p) in points.iter().enumerate() {
        let key = ((p.0 / cell).floor() as i32, (p.1 / cell).floor() as i32);
        grid.entry(key).or_default().push(i);
    }
    let mut best = f64::INFINITY;
    for (i, p) in points.iter().enumerate() {
        let cx = (p.0 / cell).floor() as i32;
        let cz = (p.1 / cell).floor() as i32;
        for dx in -2..=2 {
            for dz in -2..=2 {
                let Some(bucket) = grid.get(&(cx + dx, cz + dz)) else {
                    continue;
                };
                for &j in bucket {
                    if j <= i {
                        continue;
                    }
                    let along = (j - i) as f64 * step_m;
                    let along = along.min(length_m - along);
                    if along < window_m {
                        continue;
                    }
                    let d = (p.0 - points[j].0).hypot(p.1 - points[j].1);
                    if d < best {
                        best = d;
                    }
                }
            }
        }
    }
    best
}

/// Axis-aligned width and height of the samples, in meters: `(east, north)`.
#[must_use]
pub fn footprint(points: &[(f64, f64)]) -> (f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for (x, z) in points {
        min_x = min_x.min(*x);
        max_x = max_x.max(*x);
        min_z = min_z.min(*z);
        max_z = max_z.max(*z);
    }
    if points.is_empty() {
        return (0.0, 0.0);
    }
    (max_x - min_x, max_z - min_z)
}

/// Translate and rotate a closed path so its bounding box is centered on
/// the origin and then turned by `extra_turn` radians.
///
/// Returns the start `(x_m, z_m, heading_rad)` to write into the course.
#[must_use]
pub fn place(segs: &[Seg], h0: f64, extra_turn: f64) -> (f64, f64, f64) {
    let pts = sample_xy(segs, h0, 2.0);
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for (x, z) in pts {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }
    let cx = 0.5 * (min_x + max_x);
    let cz = 0.5 * (min_z + max_z);
    let (sx, sz) = rotate(-cx, -cz, extra_turn);
    (sx, sz, h0 + extra_turn)
}

fn rotate(x: f64, z: f64, angle: f64) -> (f64, f64) {
    let (s, c) = angle.sin_cos();
    (x * c - z * s, x * s + z * c)
}

/// Adjust one straight so the path ends on the start's easting.
///
/// The straight must run mostly east-west. Northing is left alone, which
/// is what the mountain closer wants: hairpin radius fixes northing, one
/// straight fixes easting. Returns the updated segment list.
pub fn solve_line_for_x(
    segs: &[Seg],
    h0: f64,
    candidates: &[usize],
    min_len: f64,
    max_len: f64,
) -> Option<Vec<Seg>> {
    let (x, _, _) = end_pose(segs, h0);
    for &idx in candidates {
        let Some(old) = segs.get(idx).and_then(Seg::line_length) else {
            continue;
        };
        let heading = heading_at(segs, idx, h0);
        let c = heading.cos();
        if c.abs() < 0.35 {
            continue;
        }
        let new_len = old - x / c;
        if new_len.is_finite() && new_len >= min_len && new_len <= max_len {
            let mut out = segs.to_vec();
            out[idx].set_line_length(new_len);
            return Some(out);
        }
    }
    None
}

/// Lengths of the flat run at each end of the segment list, in meters.
#[must_use]
pub fn flat_lengths(segs: &[Seg]) -> (f64, f64) {
    let mut flat1 = 0.0;
    for seg in segs {
        if seg.role() != Role::Flat {
            break;
        }
        flat1 += seg.length();
    }
    let mut flat2 = 0.0;
    for seg in segs.iter().rev() {
        if seg.role() != Role::Flat {
            break;
        }
        flat2 += seg.length();
    }
    (flat1, flat2)
}

/// Sum of horizontal length by role, in meters.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoleLengths {
    /// Flat, in meters. Both ends are included.
    pub flat: f64,
    /// Climb, in meters.
    pub climb: f64,
    /// Crest, in meters.
    pub crest: f64,
    /// Descent, in meters.
    pub descent: f64,
    /// Valley, in meters.
    pub valley: f64,
}

/// Horizontal length of each role, in meters.
#[must_use]
pub fn role_lengths(segs: &[Seg]) -> RoleLengths {
    let mut out = RoleLengths::default();
    for seg in segs {
        let len = seg.length();
        match seg.role() {
            Role::Flat => out.flat += len,
            Role::Climb => out.climb += len,
            Role::Crest => out.crest += len,
            Role::Descent => out.descent += len,
            Role::Valley => out.valley += len,
        }
    }
    out
}
