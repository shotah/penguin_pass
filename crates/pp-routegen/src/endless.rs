//! Open road that can be sampled at any distance without storing the whole path.
//!
//! `pp-course` only accepts a closed loop, so this type lives here. Each
//! section is 2 km of 3D centerline and is a pure function of `(seed, index)`.
//! The road always travels east, with heading kept inside a forward band, so
//! it cannot cross itself. Elevation is a sum of sines and stays bounded.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::f64::consts::{PI, TAU};

use rand::RngExt;
use rand::SeedableRng;
use rand::rngs::ChaCha12Rng;

/// Length of one endless section, in meters along the 3D centerline.
pub const SECTION_M: f64 = 2_000.0;

/// Easting advanced by each section, in meters.
///
/// Shorter than [`SECTION_M`], so each section has room to bend.
const X_STEP_M: f64 = 1_640.0;

/// Largest |heading| the solver is allowed to use, in radians.
const HEADING_CAP_RAD: f64 = 0.95;

const CACHE_CAP: usize = 8;

/// Phases of the endless waves, fixed for a seed.
#[derive(Clone, Copy)]
struct Phases {
    z1: f64,
    z2: f64,
    e1: f64,
    e2: f64,
}

impl Phases {
    fn from_seed(seed: u64) -> Self {
        let mut rng = ChaCha12Rng::seed_from_u64(seed ^ 0x00E4_D1E5);
        Self {
            z1: rng.random::<f64>() * TAU,
            z2: rng.random::<f64>() * TAU,
            e1: rng.random::<f64>() * TAU,
            e2: rng.random::<f64>() * TAU,
        }
    }
}

/// An endless road for one seed.
///
/// Distance is meters along the 3D centerline, the same convention as
/// `pp_course::Course`, except it does not wrap. A negative or non-finite
/// distance samples the start. Memory stays at a handful of cached sections
/// no matter how far the query is.
pub struct EndlessRoute {
    phases: Phases,
    cache: RefCell<VecDeque<(u64, f64)>>,
}

impl EndlessRoute {
    /// Road for `seed`. Section 0 starts at the origin, heading east.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            phases: Phases::from_seed(seed),
            cache: RefCell::new(VecDeque::new()),
        }
    }

    /// Section length, in meters along the 3D centerline.
    #[must_use]
    pub fn section_len_m() -> f64 {
        SECTION_M
    }

    /// How many solved sections are cached. Never grows past 8.
    #[must_use]
    pub fn cache_len(&self) -> usize {
        self.cache.borrow().len()
    }

    /// Centerline position at `distance_m`, in meters, as `[x, y, z]`.
    ///
    /// `y` is elevation. The ground plane is x east, z north.
    #[must_use]
    pub fn position_at(&self, distance_m: f64) -> [f32; 3] {
        let (u, t, beta) = self.locate(distance_m);
        let (x, y, z) = point(self.phases, u, t, beta);
        [x as f32, y as f32, z as f32]
    }

    /// Unit tangent at `distance_m`, in the direction of travel.
    #[must_use]
    pub fn tangent_at(&self, distance_m: f64) -> [f32; 3] {
        let (u, t, beta) = self.locate(distance_m);
        let (dx, dy, dz) = deriv(self.phases, u, t, beta);
        unit(dx, dy, dz)
    }

    /// Elevation above the datum at `distance_m`, in meters.
    #[must_use]
    pub fn elevation_at(&self, distance_m: f64) -> f32 {
        let (u, _, _) = self.locate(distance_m);
        elev(self.phases, u) as f32
    }

    /// Grade at `distance_m`, in percent. Positive is uphill.
    #[must_use]
    pub fn grade_pct_at(&self, distance_m: f64) -> f32 {
        let (u, t, beta) = self.locate(distance_m);
        grade_pct(self.phases, u, t, beta) as f32
    }
}

impl EndlessRoute {
    fn locate(&self, distance_m: f64) -> (f64, f64, f64) {
        if !distance_m.is_finite() || distance_m <= 0.0 {
            return (0.0, 0.0, self.beta(0));
        }
        let index = (distance_m / SECTION_M).floor();
        let index_u = index as u64;
        let local = distance_m - index * SECTION_M;
        let beta = self.beta(index_u);
        let total = arc_len(self.phases, index_u, beta, 1.0);
        let target = if total > 1.0e-6 {
            (local / SECTION_M).clamp(0.0, 1.0) * total
        } else {
            0.0
        };
        let t = t_for_arc(self.phases, index_u, beta, target);
        (index + t, t, beta)
    }

    fn beta(&self, index: u64) -> f64 {
        if let Some((_, beta)) = self.cache.borrow().iter().find(|(i, _)| *i == index) {
            return *beta;
        }
        let beta = solve_beta(self.phases, index);
        let mut cache = self.cache.borrow_mut();
        cache.push_back((index, beta));
        if cache.len() > CACHE_CAP {
            cache.pop_front();
        }
        beta
    }
}

fn solve_beta(phases: Phases, index: u64) -> f64 {
    let base = arc_len(phases, index, 0.0, 1.0);
    if base >= SECTION_M {
        return 0.0;
    }
    let cap = beta_cap(phases, index);
    if arc_len(phases, index, cap, 1.0) < SECTION_M {
        return cap;
    }
    let mut lo = 0.0;
    let mut hi = cap;
    for _ in 0..26 {
        let mid = 0.5 * (lo + hi);
        if arc_len(phases, index, mid, 1.0) < SECTION_M {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

fn beta_cap(phases: Phases, index: u64) -> f64 {
    // Keep the extra slope inside the heading cap at the steepest base slope.
    let u = index as f64 + 0.25;
    let base = z_slope(phases, u).abs();
    let max_slope = X_STEP_M * HEADING_CAP_RAD.tan();
    let room = (max_slope - base).max(40.0);
    (room / PI).clamp(40.0, 520.0)
}

fn t_for_arc(phases: Phases, index: u64, beta: f64, target: f64) -> f64 {
    let mut lo = 0.0;
    let mut hi = 1.0;
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if arc_len(phases, index, beta, mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

fn arc_len(phases: Phases, index: u64, beta: f64, t_end: f64) -> f64 {
    if t_end <= 0.0 {
        return 0.0;
    }
    let steps = 48;
    let dt = t_end / steps as f64;
    let mut acc = 0.0;
    let speed = |t: f64| {
        let u = index as f64 + t;
        let (dx, dy, dz) = deriv(phases, u, t, beta);
        (dx * dx + dy * dy + dz * dz).sqrt()
    };
    let mut prev = speed(0.0);
    for i in 1..=steps {
        let t = dt * i as f64;
        let next = speed(t);
        acc += 0.5 * (prev + next) * dt;
        prev = next;
    }
    acc
}

fn point(phases: Phases, u: f64, t: f64, beta: f64) -> (f64, f64, f64) {
    let x = u * X_STEP_M;
    let z = z_base(phases, u) + bulge(t, beta);
    let y = elev(phases, u);
    (x, y, z)
}

fn deriv(phases: Phases, u: f64, t: f64, beta: f64) -> (f64, f64, f64) {
    let dx = X_STEP_M;
    let dz = z_slope(phases, u) + bulge_slope(t, beta);
    let dy = elev_slope(phases, u);
    (dx, dy, dz)
}

fn grade_pct(phases: Phases, u: f64, t: f64, beta: f64) -> f64 {
    let (dx, dy, dz) = deriv(phases, u, t, beta);
    let horiz = (dx * dx + dz * dz).sqrt();
    if horiz < 1.0e-9 {
        0.0
    } else {
        100.0 * dy / horiz
    }
}

fn z_base(phases: Phases, u: f64) -> f64 {
    460.0 * (0.42 * u + phases.z1).sin() + 160.0 * (0.97 * u + phases.z2).sin()
}

fn z_slope(phases: Phases, u: f64) -> f64 {
    460.0 * 0.42 * (0.42 * u + phases.z1).cos() + 160.0 * 0.97 * (0.97 * u + phases.z2).cos()
}

fn elev(phases: Phases, u: f64) -> f64 {
    380.0 + 150.0 * (0.26 * u + phases.e1).sin() + 55.0 * (0.61 * u + phases.e2).sin()
}

fn elev_slope(phases: Phases, u: f64) -> f64 {
    150.0 * 0.26 * (0.26 * u + phases.e1).cos() + 55.0 * 0.61 * (0.61 * u + phases.e2).cos()
}

/// Bump that is zero, with zero slope, at both ends of a section.
fn bulge(t: f64, beta: f64) -> f64 {
    beta * (1.0 - (TAU * t).cos()) * 0.5
}

fn bulge_slope(t: f64, beta: f64) -> f64 {
    beta * PI * (TAU * t).sin()
}

fn unit(x: f64, y: f64, z: f64) -> [f32; 3] {
    let mag = (x * x + y * y + z * z).sqrt();
    if mag > 0.0 {
        [(x / mag) as f32, (y / mag) as f32, (z / mag) as f32]
    } else {
        [1.0, 0.0, 0.0]
    }
}
