//! Turn a [`RouteSpec`](crate::RouteSpec) into a closed course.

use std::f64::consts::TAU;
use std::str::FromStr;

use pp_course::Course;
use serde_json::{Value, json};

use crate::error::RouteGenError;
use crate::geom::{self, Seg};
use crate::grade::{self, Span};
use crate::plan::{self, Style};
use crate::rng::{self, Stream};
use crate::spec::{self, RouteSpec, TerrainParams, ascent_tolerance_m};

/// A course produced by [`generate`].
#[derive(Clone)]
pub struct GeneratedRoute {
    spec: RouteSpec,
    json: String,
    course: Course,
    attempts: u32,
    min_radius_m: f64,
}

impl GeneratedRoute {
    /// The spec that produced this route.
    #[must_use]
    pub fn spec(&self) -> &RouteSpec {
        &self.spec
    }

    /// Course JSON, including the trailing newline.
    ///
    /// The bytes are a compatibility promise for this spec.
    #[must_use]
    pub fn json(&self) -> &str {
        &self.json
    }

    /// The parsed course. This is the same text as [`Self::json`].
    #[must_use]
    pub fn course(&self) -> &Course {
        &self.course
    }

    /// How many candidates were built, including the one that was kept.
    #[must_use]
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Tightest arc radius in the plan, in meters, after scaling.
    #[must_use]
    pub fn min_radius_m(&self) -> f64 {
        self.min_radius_m
    }
}

impl std::fmt::Debug for GeneratedRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneratedRoute")
            .field("name", &self.course.name())
            .field("length_m", &self.course.length_m())
            .field("attempts", &self.attempts)
            .finish()
    }
}

/// Build a closed, rideable course for `spec`.
///
/// The result parses with `Course::from_str`. Length is within 1 m of
/// `spec.distance_m`. The same spec always returns byte-identical JSON.
///
/// # Errors
///
/// Returns [`RouteGenError::Spec`](crate::RouteGenError::Spec) for a bad spec
/// and [`RouteGenError::Exhausted`](crate::RouteGenError::Exhausted) when
/// every candidate fails a rideability check.
pub fn generate(spec: &RouteSpec) -> Result<GeneratedRoute, RouteGenError> {
    spec.validate()?;
    let params = spec.terrain.params();
    let style = Style::of(spec.terrain);
    let mut last = String::from("no attempt");
    for attempt in 0..spec::MAX_ATTEMPTS {
        let seed = rng::mix_attempt(spec.seed, attempt);
        match try_once(spec, &params, style, seed, attempt + 1) {
            Ok(route) => return Ok(route),
            Err(reason) => last = reason,
        }
    }
    Err(RouteGenError::Exhausted {
        attempts: spec::MAX_ATTEMPTS,
        last_reason: last,
    })
}

fn try_once(
    spec: &RouteSpec,
    params: &TerrainParams,
    style: Style,
    seed: u64,
    attempts: u32,
) -> Result<GeneratedRoute, String> {
    let mut rng = Stream::new(seed);
    let h_target = spec.distance_m / 1.004;
    let mut plan = plan::build(&mut rng, h_target, params, style)?;
    let h = geom::horiz(&plan.segs);
    if !h.is_finite() || h <= 50.0 {
        return Err("plan is too short to scale".to_string());
    }
    geom::scale(&mut plan.segs, h_target / h);
    plan::radii_ok(&plan.segs, params)?;
    let floor = params.switchback_radius_m.min(params.min_radius_m);
    plan::check_shape(&plan.segs, plan.h0, spec.distance_m, floor)?;

    let target = params.ascent_per_km * spec.distance_m / 1000.0;
    let mut spans = grade::build(&plan.segs, target, params, style, &mut rng)?;
    fit_3d(&mut plan.segs, &mut spans, spec.distance_m)?;
    plan::radii_ok(&plan.segs, params)?;
    plan::check_shape(&plan.segs, plan.h0, spec.distance_m, floor)?;

    let turn = rng.range(0.0, TAU);
    let mut placed = place_json(spec, &plan.segs, &spans, plan.h0, turn)?;
    if (placed.course.length_m() - spec.distance_m).abs() > 0.4 {
        let k = spec.distance_m / placed.course.length_m();
        geom::scale(&mut plan.segs, k);
        grade::scale_lengths(&mut spans, k);
        grade::sync_length(&mut spans, geom::horiz(&plan.segs))?;
        plan::radii_ok(&plan.segs, params)?;
        plan::check_shape(&plan.segs, plan.h0, spec.distance_m, floor)?;
        placed = place_json(spec, &plan.segs, &spans, plan.h0, turn)?;
    }
    if (placed.course.length_m() - spec.distance_m).abs() > 1.0 {
        return Err(format!("length is {:.3} m", placed.course.length_m()));
    }
    let ascent = f64::from(placed.course.total_ascent_m());
    let tol = ascent_tolerance_m(target);
    if target > 1.0 && (ascent - target).abs() > tol {
        return Err(format!("ascent {ascent:.1} m vs target {target:.1} m"));
    }
    Ok(GeneratedRoute {
        spec: spec.clone(),
        json: placed.json,
        course: placed.course,
        attempts,
        min_radius_m: geom::min_radius(&plan.segs),
    })
}

struct Placed {
    json: String,
    course: Course,
}

fn place_json(
    spec: &RouteSpec,
    segs: &[Seg],
    spans: &[Span],
    h0: f64,
    turn: f64,
) -> Result<Placed, String> {
    let (x, z, heading) = geom::place(segs, h0, turn);
    let json = course_json(spec.course_name(), x, z, heading, segs, spans);
    let course = Course::from_str(&json).map_err(|err| err.to_string())?;
    Ok(Placed { json, course })
}

fn fit_3d(segs: &mut [Seg], spans: &mut [Span], distance_m: f64) -> Result<(), String> {
    for _ in 0..3 {
        let h = geom::horiz(segs);
        grade::sync_length(spans, h)?;
        let len3 = grade::length_3d_m(h, spans);
        if !len3.is_finite() || len3 <= 1.0 {
            return Err("3D length collapsed".to_string());
        }
        let k = distance_m / len3;
        if (k - 1.0).abs() < 2.0e-5 {
            break;
        }
        geom::scale(segs, k);
        grade::scale_lengths(spans, k);
    }
    grade::sync_length(spans, geom::horiz(segs))?;
    Ok(())
}

fn course_json(
    name: &str,
    start_x: f64,
    start_z: f64,
    heading: f64,
    segs: &[Seg],
    spans: &[Span],
) -> String {
    let segments: Vec<Value> = segs
        .iter()
        .map(|seg| match *seg {
            Seg::Line { length, .. } => json!({
                "type": "line",
                "length_m": length,
            }),
            Seg::Arc { radius, angle, .. } => json!({
                "type": "arc",
                "radius_m": radius,
                "angle_rad": angle,
            }),
        })
        .collect();
    let grade: Vec<Value> = spans.iter().copied().map(Span::to_json).collect();
    let value = json!({
        "name": name,
        "start": { "x": start_x, "z": start_z, "heading_rad": heading },
        "segments": segments,
        "grade": grade,
    });
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string());
    text.push('\n');
    text
}
