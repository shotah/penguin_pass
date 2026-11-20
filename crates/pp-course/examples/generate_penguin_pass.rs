//! Generate `courses/penguin_pass.json`.
//!
//! The loop is built from straights and circular arcs so the turn radius is
//! known exactly, then scaled so the 3D centerline is 5000 m. Grades are a
//! smooth climb, a sine wave along the ridge, and a matching descent, so the
//! lap closes in elevation.
//!
//! Plan, traveling west from the start line: a short flat, a right hairpin
//! into a five-leg switchback climb, an eastbound ridge, a wider three-leg
//! descent, and the other half of the flat back to the line. The arc sampler
//! matches `sample_arc` in `src/course.rs` (quarter circle and right hairpin
//! are locked by that file's tests).

use std::env;
use std::f64::consts::{PI, TAU};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use pp_core::RiderProfile;
use pp_course::{Course, MAX_SPEED_MPS, acceleration_mps2};
use serde_json::{Value, json};

const R_CLIMB: f64 = 36.0;
const R_DESC: f64 = 60.0;
const LEG_CLIMB: f64 = 250.0;
const LEG_DESC: f64 = 280.0;
const FLAT: f64 = 400.0;
const CLIMB_LEGS: u32 = 5;
const DESC_LEGS: u32 = 3;
const TARGET_LENGTH_M: f64 = 5_000.0;
const TARGET_ASCENT_M: f64 = 185.0;

struct Built {
    segments: Vec<Value>,
    s_climb: f64,
    s_ridge: f64,
    s_descent: f64,
    s_flat2: f64,
    total_h: f64,
    end_x: f64,
    end_z: f64,
    end_heading: f64,
}

struct Grades {
    climb_pct: f64,
    descent_pct: f64,
    amplitude_pct: f64,
    cycles: u32,
    ramp_up: f64,
    ramp_down: f64,
    ramp_in: f64,
    ramp_out: f64,
}

struct Builder {
    x: f64,
    z: f64,
    heading: f64,
    s: f64,
    segments: Vec<Value>,
    marks: Vec<(&'static str, f64)>,
}

impl Builder {
    fn start_west() -> Self {
        Self {
            x: 0.0,
            z: 0.0,
            heading: PI,
            s: 0.0,
            segments: Vec::new(),
            marks: Vec::new(),
        }
    }

    fn mark(&mut self, name: &'static str) {
        self.marks.push((name, self.s));
    }

    fn line(&mut self, length: f64) {
        if length <= 0.0 {
            return;
        }
        self.segments.push(json!({
            "type": "line",
            "length_m": length,
        }));
        self.x += length * self.heading.cos();
        self.z += length * self.heading.sin();
        self.s += length;
    }

    fn arc(&mut self, radius: f64, angle: f64) {
        self.segments.push(json!({
            "type": "arc",
            "radius_m": radius,
            "angle_rad": angle,
        }));
        let (x, z, heading) = sample_arc(
            self.x,
            self.z,
            self.heading,
            radius,
            angle,
            radius * angle.abs(),
        );
        self.x = x;
        self.z = z;
        self.heading = heading;
        self.s += radius * angle.abs();
    }

    fn ladder(&mut self, legs: u32, leg: f64, radius: f64) {
        let mut turn_left = true;
        for i in 0..legs {
            self.line(leg);
            if i + 1 == legs {
                break;
            }
            let angle = if turn_left { PI } else { -PI };
            self.arc(radius, angle);
            turn_left = !turn_left;
        }
    }
}

fn sample_arc(
    x: f64,
    z: f64,
    heading: f64,
    radius: f64,
    angle: f64,
    distance: f64,
) -> (f64, f64, f64) {
    let dir = if angle >= 0.0 { 1.0 } else { -1.0 };
    let travelled = dir * distance / radius;
    let normal = heading + dir * std::f64::consts::FRAC_PI_2;
    let cx = x + radius * normal.cos();
    let cz = z + radius * normal.sin();
    let start_from_center = heading - dir * std::f64::consts::FRAC_PI_2;
    let a = start_from_center + travelled;
    (
        cx + radius * a.cos(),
        cz + radius * a.sin(),
        heading + travelled,
    )
}

fn build(ridge: f64, k: f64) -> Built {
    let rc = R_CLIMB * k;
    let rd = R_DESC * k;
    let leg_c = LEG_CLIMB * k;
    let leg_d = LEG_DESC * k;
    let flat = FLAT * k;
    let ridge = ridge * k;
    let half = flat / 2.0;

    let mut b = Builder::start_west();
    b.line(half);
    b.mark("climb");
    b.arc(rc, -PI);
    b.ladder(CLIMB_LEGS, leg_c, rc);
    b.mark("ridge");
    b.line(ridge);
    b.mark("descent");
    b.arc(rd, -PI);
    b.ladder(DESC_LEGS, leg_d, rd);
    b.mark("flat2");
    b.line(half);
    b.mark("end");

    let mark = |name: &str| -> f64 {
        b.marks
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, s)| *s)
            .unwrap_or(0.0)
    };
    Built {
        segments: b.segments,
        s_climb: mark("climb"),
        s_ridge: mark("ridge"),
        s_descent: mark("descent"),
        s_flat2: mark("flat2"),
        total_h: b.s,
        end_x: b.x,
        end_z: b.z,
        end_heading: b.heading,
    }
}

fn solve_ridge() -> Result<f64, String> {
    let x0 = build(0.0, 1.0).end_x;
    let x1 = build(1.0, 1.0).end_x;
    let slope = x1 - x0;
    if slope.abs() < 0.5 {
        return Err(format!("ridge does not move the end in x (slope {slope})"));
    }
    let ridge = -x0 / slope;
    let closed = build(ridge, 1.0);
    let gap = closed.end_x.hypot(closed.end_z);
    let heading_gap = angle_diff(closed.end_heading, PI).abs();
    if gap > 1e-6 || heading_gap > 1e-8 {
        return Err(format!(
            "unscaled loop does not close: gap {gap} m, heading error {heading_gap} rad, ridge {ridge}"
        ));
    }
    if ridge < 80.0 {
        return Err(format!(
            "solved ridge {ridge} m is too short to separate the ladders"
        ));
    }
    Ok(ridge)
}

fn angle_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(TAU);
    if d > PI { d - TAU } else { d }
}

fn choose_grades(built: &Built) -> Result<Grades, String> {
    let climb = built.s_ridge - built.s_climb;
    let ridge = built.s_descent - built.s_ridge;
    let descent = built.s_flat2 - built.s_descent;
    if climb < 200.0 || ridge < 100.0 || descent < 200.0 {
        return Err(format!(
            "section lengths are too short: climb {climb}, ridge {ridge}, descent {descent}"
        ));
    }
    let ramp_up = climb * 0.12;
    let ramp_down = climb * 0.12;
    let ramp_in = descent * 0.12;
    let ramp_out = descent * 0.12;
    let climb_eff = climb - 0.5 * ramp_up - 0.5 * ramp_down;
    let descent_eff = descent - 0.5 * ramp_in - 0.5 * ramp_out;
    let amplitude_pct = 7.0;
    let cycles = if ridge >= 400.0 { 2 } else { 1 };
    let ridge_ascent = (amplitude_pct / 100.0) * ridge / PI;
    let mut climb_pct = (TARGET_ASCENT_M - ridge_ascent) / climb_eff * 100.0;
    let cap = 11.0 * descent_eff / climb_eff;
    climb_pct = climb_pct.min(cap).min(11.0);
    if climb_pct < 4.0 {
        return Err(format!(
            "climb grade {climb_pct}% is too gentle to be a mountain"
        ));
    }
    let descent_pct = -climb_pct * climb_eff / descent_eff;
    if descent_pct.abs() > 11.5 {
        return Err(format!("descent grade {descent_pct}% exceeds 11.5%"));
    }
    Ok(Grades {
        climb_pct,
        descent_pct,
        amplitude_pct,
        cycles,
        ramp_up,
        ramp_down,
        ramp_in,
        ramp_out,
    })
}

fn grade_profile(built: &Built, grades: &Grades) -> Vec<Value> {
    let climb = built.s_ridge - built.s_climb;
    let ridge = built.s_descent - built.s_ridge;
    let descent = built.s_flat2 - built.s_descent;
    let flat2 = built.total_h - built.s_flat2;
    let hold_climb = climb - grades.ramp_up - grades.ramp_down;
    let hold_desc = descent - grades.ramp_in - grades.ramp_out;
    vec![
        json!({"type": "hold", "length_m": built.s_climb, "grade_pct": 0.0}),
        json!({"type": "smooth", "length_m": grades.ramp_up, "to_grade_pct": grades.climb_pct}),
        json!({"type": "hold", "length_m": hold_climb, "grade_pct": grades.climb_pct}),
        json!({"type": "smooth", "length_m": grades.ramp_down, "to_grade_pct": 0.0}),
        json!({
            "type": "wave",
            "length_m": ridge,
            "amplitude_pct": grades.amplitude_pct,
            "cycles": grades.cycles,
        }),
        json!({"type": "smooth", "length_m": grades.ramp_in, "to_grade_pct": grades.descent_pct}),
        json!({"type": "hold", "length_m": hold_desc, "grade_pct": grades.descent_pct}),
        json!({"type": "smooth", "length_m": grades.ramp_out, "to_grade_pct": 0.0}),
        json!({"type": "hold", "length_m": flat2, "grade_pct": 0.0}),
    ]
}

fn course_json(built: &Built, grades: &Grades, start_x: f64, start_z: f64) -> Value {
    json!({
        "name": "Penguin Pass",
        "start": {"x": start_x, "z": start_z, "heading_rad": PI},
        "segments": built.segments,
        "grade": grade_profile(built, grades),
    })
}

fn fit(ridge: f64) -> Result<(f64, Grades, String), String> {
    let mut k = 1.0;
    let mut chosen: Option<(f64, Grades, String)> = None;
    for _ in 0..8 {
        let built = build(ridge, k);
        let grades = choose_grades(&built)?;
        let value = course_json(&built, &grades, 0.0, 0.0);
        let text = serde_json::to_string(&value).map_err(|err| err.to_string())?;
        let course = Course::from_str(&text).map_err(|err| err.to_string())?;
        let length = course.length_m();
        let ascent = f64::from(course.total_ascent_m());
        println!(
            "fit k={k:.6} length={length:.3} ascent={ascent:.3} climb={:.2}% descent={:.2}%",
            grades.climb_pct, grades.descent_pct
        );
        let scale = TARGET_LENGTH_M / length;
        chosen = Some((k, grades, text));
        if (length - TARGET_LENGTH_M).abs() < 0.01 && (160.0..=210.0).contains(&ascent) {
            break;
        }
        k *= scale;
        if !k.is_finite() || !(0.2..5.0).contains(&k) {
            return Err(format!("scale factor diverged to {k}"));
        }
    }
    chosen.ok_or_else(|| "fit produced no course".to_string())
}

fn bbox(course: &Course) -> (f64, f64, f64, f64) {
    let n = course.length_m().round() as usize;
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_z = f64::MAX;
    let mut max_z = f64::MIN;
    for i in 0..n {
        let p = course.position_at(course.length_m() * i as f64 / n as f64);
        let x = f64::from(p[0]);
        let z = f64::from(p[2]);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }
    (min_x, max_x, min_z, max_z)
}

fn spec_failures(course: &Course) -> Vec<String> {
    let mut failures = Vec::new();
    let length = course.length_m();
    if (length - TARGET_LENGTH_M).abs() > 5.0 {
        failures.push(format!("length {length}"));
    }
    let ascent = f64::from(course.total_ascent_m());
    if !(150.0..=220.0).contains(&ascent) {
        failures.push(format!("ascent {ascent}"));
    }
    let (min_x, max_x, min_z, max_z) = bbox(course);
    let dx = max_x - min_x;
    let dz = max_z - min_z;
    if dx > 1_450.0 || dz > 1_450.0 {
        failures.push(format!("footprint {dx:.1} by {dz:.1}"));
    }
    let cx = 0.5 * (min_x + max_x);
    let cz = 0.5 * (min_z + max_z);
    if cx.abs() > 5.0 || cz.abs() > 5.0 {
        failures.push(format!("center {cx:.2}, {cz:.2}"));
    }

    let n = length.round() as usize;
    let step = length / n as f64;
    let mut min_g = f32::MAX;
    let mut max_g = f32::MIN;
    let mut max_jump = 0.0f32;
    let mut prev = course.grade_pct_at(0.0);
    let mut min_radius = f64::MAX;
    for i in 0..n {
        let d = i as f64 * step;
        let g = course.grade_pct_at(d);
        min_g = min_g.min(g);
        max_g = max_g.max(g);
        let d2 = if i + 1 == n {
            length
        } else {
            (i + 1) as f64 * step
        };
        let g2 = course.grade_pct_at(d2);
        max_jump = max_jump.max((g2 - prev).abs());
        prev = g2;

        let p0 = course.position_at(d);
        let p1 = course.position_at(d2);
        let horiz = f64::hypot(f64::from(p1[0] - p0[0]), f64::from(p1[2] - p0[2]));
        let h0 = f64::from(course.tangent_at(d)[2]).atan2(f64::from(course.tangent_at(d)[0]));
        let h1 = f64::from(course.tangent_at(d2)[2]).atan2(f64::from(course.tangent_at(d2)[0]));
        let dh = angle_diff(h1, h0).abs();
        if horiz > 1e-3 && dh > 1e-6 {
            min_radius = min_radius.min(horiz / dh);
        }
    }
    if min_g < -12.0 || max_g > 12.0 {
        failures.push(format!("grade {min_g}..{max_g}"));
    }
    if max_jump > 1.0 {
        failures.push(format!("grade jump {max_jump}"));
    }
    if min_radius < 14.0 {
        failures.push(format!("radius {min_radius}"));
    }

    let stride = 2.0;
    let m = (length / stride).round() as usize;
    let mut pts = Vec::with_capacity(m);
    for i in 0..m {
        let p = course.position_at(i as f64 * stride);
        pts.push((f64::from(p[0]), f64::from(p[2])));
    }
    let mut min_sq = f64::MAX;
    let mut pair = (0usize, 0usize);
    let consecutive = 60.0;
    for i in 0..m {
        for j in (i + 1)..m {
            let along = (j - i) as f64 * stride;
            if along.min(length - along) <= consecutive {
                continue;
            }
            let dx = pts[j].0 - pts[i].0;
            let dz = pts[j].1 - pts[i].1;
            let dsq = dx * dx + dz * dz;
            if dsq < min_sq {
                min_sq = dsq;
                pair = (i, j);
            }
        }
    }
    let clearance = min_sq.sqrt();
    if clearance < 25.0 {
        failures.push(format!(
            "clearance {clearance:.2} m between samples {} and {}",
            pair.0, pair.1
        ));
    }
    println!(
        "spec length={length:.3} ascent={ascent:.3} grade={min_g:.2}..{max_g:.2} jump={max_jump:.3} radius={min_radius:.2} clearance={clearance:.2} box={dx:.1}x{dz:.1} center=({cx:.2},{cz:.2})"
    );
    failures
}

fn ascii_map(course: &Course) -> String {
    let n = 500;
    let mut pts = Vec::with_capacity(n);
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_z = f64::MAX;
    let mut max_z = f64::MIN;
    for i in 0..n {
        let p = course.position_at(course.length_m() * i as f64 / n as f64);
        let x = f64::from(p[0]);
        let z = f64::from(p[2]);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
        pts.push((x, z));
    }
    let cols = 72;
    let rows = 28;
    let mut grid = vec![vec![' '; cols]; rows];
    for (i, (x, z)) in pts.iter().enumerate() {
        let c = ((*x - min_x) / (max_x - min_x).max(1.0) * (cols - 1) as f64).round() as usize;
        let r = ((max_z - *z) / (max_z - min_z).max(1.0) * (rows - 1) as f64).round() as usize;
        let ch = if i == 0 { 'S' } else { '#' };
        if grid[r][c] == ' ' || ch == 'S' {
            grid[r][c] = ch;
        }
    }
    grid.into_iter()
        .map(|row| row.into_iter().collect())
        .collect::<Vec<String>>()
        .join("\n")
}

fn elevation_table(course: &Course) -> String {
    let profile = course.profile(250.0);
    let max_e = profile.iter().fold(0.0f32, |acc, p| acc.max(p.elevation_m));
    let mut lines = Vec::new();
    for point in &profile {
        let g = course.grade_pct_at(point.distance_m);
        let bars = if max_e > 0.0 {
            (point.elevation_m / max_e * 36.0).round() as usize
        } else {
            0
        };
        lines.push(format!(
            "{distance:>7.0} {elev:>7.2} {grade:>7.2}  {bar}",
            distance = point.distance_m,
            elev = point.elevation_m,
            grade = g,
            bar = "#".repeat(bars),
        ));
    }
    lines.join("\n")
}

fn equilibrium_kph(power_w: f32, grade_pct: f32) -> f32 {
    let rider = RiderProfile::default();
    let mut lo = 1.0f32;
    let mut hi = 40.0f32;
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if acceleration_mps2(mid, power_w, grade_pct, &rider) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi) * 3.6
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ridge = solve_ridge()?;
    println!("solved unscaled ridge = {ridge:.3} m");
    let (k, grades, _) = fit(ridge)?;
    let built = build(ridge, k);
    let rough = course_json(&built, &grades, 0.0, 0.0);
    let rough_course = Course::from_str(&rough.to_string())?;
    let (min_x, max_x, min_z, max_z) = bbox(&rough_course);
    let start_x = -0.5 * (min_x + max_x);
    let start_z = -0.5 * (min_z + max_z);
    let value = course_json(&built, &grades, start_x, start_z);
    let text = serde_json::to_string_pretty(&value)? + "\n";
    let course = Course::from_str(&text)?;
    println!("{}", ascii_map(&course));
    println!("distance_m elevation_m grade_pct");
    println!("{}", elevation_table(&course));
    println!(
        "physics flat 200 W = {:.2} km/h, 8% 250 W = {:.2} km/h, cap = {:.2} km/h",
        equilibrium_kph(200.0, 0.0),
        equilibrium_kph(250.0, 8.0),
        MAX_SPEED_MPS * 3.6
    );
    let mut climb_start = None;
    let mut climb_end = None;
    let mut max_elev = 0.0f32;
    let mut max_elev_d = 0.0;
    let mut d = 0.0;
    while d <= course.length_m() {
        let g = course.grade_pct_at(d);
        let elev = course.elevation_at(d);
        if elev > max_elev {
            max_elev = elev;
            max_elev_d = d;
        }
        if g > 2.0 && climb_start.is_none() {
            climb_start = Some(d);
        }
        if g > 2.0 {
            climb_end = Some(d);
        }
        d += 10.0;
    }
    println!(
        "climb grade>2% from {:?} to {:?}, max elevation {max_elev:.2} m at {max_elev_d:.0} m",
        climb_start, climb_end
    );
    let failures = spec_failures(&course);
    if !failures.is_empty() {
        return Err(format!("spec failed: {}", failures.join("; ")).into());
    }
    let path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("courses/penguin_pass.json");
    fs::write(&path, text)?;
    println!("wrote {}", path.display());
    Ok(())
}
