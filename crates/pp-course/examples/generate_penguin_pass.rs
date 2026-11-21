//! Generate `courses/penguin_pass.json`.
//!
//! The loop is built from straights and circular arcs so the turn radius is
//! known exactly, then scaled so the 3D centerline is 5000 m. Grades change
//! pitch through the climb and the descent: easier hairpins, a mid-climb
//! breather, a steep kick near the top, and a rolling ridge. The areas cancel
//! so the lap closes in elevation.
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

use pp_core::{RiderProfile, TrainerSample};
use pp_course::{Course, MAX_SPEED_MPS, Ride, acceleration_mps2};
use serde_json::{Value, json};

const R_CLIMB: f64 = 36.0;
const R_DESC: f64 = 60.0;
const LEG_CLIMB: f64 = 250.0;
const LEG_DESC: f64 = 280.0;
const FLAT: f64 = 400.0;
const CLIMB_LEGS: u32 = 5;
const DESC_LEGS: u32 = 3;
const TARGET_LENGTH_M: f64 = 5_000.0;
const RIDGE_AMPLITUDE_PCT: f64 = 7.0;

struct Built {
    segments: Vec<Value>,
    pieces: Vec<Piece>,
    s_climb: f64,
    s_ridge: f64,
    s_descent: f64,
    s_flat2: f64,
    total_h: f64,
    end_x: f64,
    end_z: f64,
    end_heading: f64,
}

struct Piece {
    is_arc: bool,
    s0: f64,
    length: f64,
}

enum Span {
    Hold {
        length: f64,
        grade: f64,
    },
    Smooth {
        length: f64,
        to: f64,
    },
    Wave {
        length: f64,
        amplitude: f64,
        cycles: u32,
    },
}

struct Builder {
    x: f64,
    z: f64,
    heading: f64,
    s: f64,
    segments: Vec<Value>,
    pieces: Vec<Piece>,
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
            pieces: Vec::new(),
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
        self.pieces.push(Piece {
            is_arc: false,
            s0: self.s,
            length,
        });
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
        self.pieces.push(Piece {
            is_arc: true,
            s0: self.s,
            length: radius * angle.abs(),
        });
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
        pieces: b.pieces,
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

fn section_pieces(built: &Built, start: f64, end: f64) -> Vec<&Piece> {
    built
        .pieces
        .iter()
        .filter(|piece| piece.s0 >= start - 1.0e-6 && piece.s0 < end - 1.0e-6)
        .collect()
}

fn transition_len(delta: f64) -> f64 {
    // Keep the grade slope near 0.22 percentage points per meter.
    (delta.abs() * 1.5 / 0.22).clamp(36.0, 90.0)
}

fn rise_of(spans: &[Span]) -> f64 {
    let mut grade = 0.0;
    let mut rise = 0.0;
    for span in spans {
        match *span {
            Span::Hold {
                length,
                grade: next,
            } => {
                rise += next / 100.0 * length;
                grade = next;
            }
            Span::Smooth { length, to } => {
                rise += length / 100.0 * (grade + to) / 2.0;
                grade = to;
            }
            Span::Wave { .. } => grade = 0.0,
        }
    }
    rise
}

fn push_grade(out: &mut Vec<Span>, prev: &mut f64, grade: f64, length: f64) {
    if length <= 0.5 {
        return;
    }
    let delta = grade - *prev;
    if delta.abs() < 0.05 {
        out.push(Span::Hold {
            length,
            grade: *prev,
        });
        return;
    }
    let trans = transition_len(delta).min(length * 0.45);
    if let Some(Span::Hold {
        length: previous, ..
    }) = out.last_mut()
        && *previous > trans + 60.0
    {
        *previous -= trans;
        out.push(Span::Smooth {
            length: trans,
            to: grade,
        });
        out.push(Span::Hold { length, grade });
    } else {
        let steal = trans.min(length * 0.45);
        out.push(Span::Smooth {
            length: steal,
            to: grade,
        });
        if length - steal > 0.5 {
            out.push(Span::Hold {
                length: length - steal,
                grade,
            });
        }
    }
    *prev = grade;
}

fn climb_spans(pieces: &[&Piece]) -> Result<Vec<Span>, String> {
    if pieces.len() < 4 {
        return Err(format!("climb has only {} pieces", pieces.len()));
    }
    let climb_len: f64 = pieces.iter().map(|piece| piece.length).sum();
    let origin = pieces[0].s0;
    let breather = pieces
        .iter()
        .enumerate()
        .filter(|(_, piece)| piece.is_arc)
        .min_by(|(_, a), (_, b)| {
            let mid_a = (a.s0 + a.length * 0.5) - origin;
            let mid_b = (b.s0 + b.length * 0.5) - origin;
            (mid_a - climb_len * 0.48)
                .abs()
                .partial_cmp(&(mid_b - climb_len * 0.48).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(index, _)| index)
        .ok_or_else(|| "climb has no hairpin for the breather".to_string())?;

    let line_grades = [7.6, 8.6, 8.0, 8.8];
    let mut line_i = 0usize;
    let mut prev = 0.0;
    let mut out = Vec::new();
    let last = pieces.len() - 1;
    for (index, piece) in pieces.iter().enumerate().take(last) {
        let grade = if index == breather {
            3.5
        } else if piece.is_arc {
            if index == 0 { 5.4 } else { 5.2 }
        } else {
            let grade = line_grades[line_i.min(line_grades.len() - 1)];
            line_i += 1;
            grade
        };
        if index == 0 && piece.is_arc {
            out.push(Span::Smooth {
                length: piece.length,
                to: grade,
            });
            prev = grade;
        } else {
            push_grade(&mut out, &mut prev, grade, piece.length);
        }
    }

    let last_len = pieces[last].length;
    let kick = 10.0;
    let trans = transition_len(kick - prev).min(last_len * 0.2);
    let ramp = transition_len(kick).max(70.0).min(last_len * 0.3);
    let hold = last_len - trans - ramp;
    if hold < 140.0 {
        return Err(format!(
            "kick hold is {hold:.1} m; the last climb straight is too short ({last_len:.1} m)"
        ));
    }
    out.push(Span::Smooth {
        length: trans,
        to: kick,
    });
    out.push(Span::Hold {
        length: hold,
        grade: kick,
    });
    out.push(Span::Smooth {
        length: ramp,
        to: 0.0,
    });
    let sum: f64 = out.iter().map(Span::length).sum();
    if (sum - climb_len).abs() > 1.0e-6 {
        return Err(format!("climb spans sum to {sum}, section is {climb_len}"));
    }
    Ok(out)
}

fn descent_spans(pieces: &[&Piece]) -> Result<Vec<Span>, String> {
    if pieces.is_empty() {
        return Err("descent has no pieces".to_string());
    }
    let descent_len: f64 = pieces.iter().map(|piece| piece.length).sum();
    let easing = pieces
        .iter()
        .enumerate()
        .filter(|(_, piece)| piece.is_arc && piece.length >= 120.0)
        .max_by(|(_, a), (_, b)| {
            a.length
                .partial_cmp(&b.length)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(index, _)| index);
    let mut prev = 0.0;
    let mut out = Vec::new();
    let shoulders = [-9.0, -10.4, -10.8, -9.6];
    let mut shoulder_i = 0usize;
    for (index, piece) in pieces.iter().enumerate() {
        let is_last = index + 1 == pieces.len();
        if easing == Some(index) && piece.length > 200.0 {
            // Keep a protected 120 m easing so the transitions cannot eat it.
            let ease_hold = 120.0;
            let trans = 36.0;
            let side = (piece.length - ease_hold - 2.0 * trans) / 2.0;
            let shoulder = shoulders[shoulder_i.min(shoulders.len() - 1)];
            shoulder_i += 1;
            push_grade(&mut out, &mut prev, shoulder, side);
            out.push(Span::Smooth {
                length: trans,
                to: -5.5,
            });
            out.push(Span::Hold {
                length: ease_hold,
                grade: -5.5,
            });
            out.push(Span::Smooth {
                length: trans,
                to: shoulder,
            });
            out.push(Span::Hold {
                length: side,
                grade: shoulder,
            });
            prev = shoulder;
            continue;
        }
        let grade = shoulders[shoulder_i.min(shoulders.len() - 1)];
        shoulder_i += 1;
        if is_last {
            let ramp = transition_len(grade).max(70.0).min(piece.length * 0.35);
            let body = piece.length - ramp;
            push_grade(&mut out, &mut prev, grade, body);
            out.push(Span::Smooth {
                length: ramp,
                to: 0.0,
            });
            prev = 0.0;
        } else if index == 0 {
            out.push(Span::Smooth {
                length: piece.length.clamp(50.0, 80.0).min(piece.length),
                to: grade,
            });
            let used = match out.last() {
                Some(Span::Smooth { length, .. }) => *length,
                _ => 0.0,
            };
            prev = grade;
            if piece.length - used > 1.0 {
                out.push(Span::Hold {
                    length: piece.length - used,
                    grade,
                });
            }
        } else {
            push_grade(&mut out, &mut prev, grade, piece.length);
        }
    }
    let sum: f64 = out.iter().map(Span::length).sum();
    if (sum - descent_len).abs() > 1.0e-4 {
        return Err(format!(
            "descent spans sum to {sum}, section is {descent_len}"
        ));
    }
    // Absorb a rounding remainder into the longest hold.
    if (sum - descent_len).abs() > 1.0e-9 {
        let fix = descent_len - sum;
        if let Some(Span::Hold { length, .. }) = out
            .iter_mut()
            .find(|span| matches!(span, Span::Hold { length, .. } if *length > 50.0))
        {
            *length += fix;
        }
    }
    Ok(out)
}

fn scale_grades(spans: &mut [Span], alpha: f64) {
    for span in spans {
        match span {
            Span::Hold { grade, .. } => *grade *= alpha,
            Span::Smooth { to, .. } => *to *= alpha,
            Span::Wave { amplitude, .. } => *amplitude *= alpha,
        }
    }
}

fn balance(built: &Built) -> Result<Vec<Span>, String> {
    let climb_pieces = section_pieces(built, built.s_climb, built.s_ridge);
    let descent_pieces = section_pieces(built, built.s_descent, built.s_flat2);
    let climb = climb_spans(&climb_pieces)?;
    let mut descent = descent_spans(&descent_pieces)?;
    let climb_rise = rise_of(&climb);
    let descent_drop = -rise_of(&descent);
    if descent_drop < 1.0 || climb_rise < 1.0 {
        return Err(format!(
            "grade areas are not a climb and a descent ({climb_rise} / {descent_drop})"
        ));
    }
    let alpha = climb_rise / descent_drop;
    scale_grades(&mut descent, alpha);
    let steepest = descent.iter().fold(0.0_f64, |steep, span| {
        let grade = match *span {
            Span::Hold { grade, .. } => grade,
            Span::Smooth { to, .. } => to,
            Span::Wave { amplitude, .. } => -amplitude.abs(),
        };
        steep.min(grade)
    });
    if steepest < -11.35 {
        return Err(format!(
            "balanced descent reaches {steepest:.2}%, steeper than -11.3% (scale {alpha:.3})"
        ));
    }
    let ridge_len = built.s_descent - built.s_ridge;
    let cycles = if ridge_len >= 400.0 { 2 } else { 1 };
    let mut spans = Vec::new();
    spans.push(Span::Hold {
        length: built.s_climb,
        grade: 0.0,
    });
    spans.extend(climb);
    spans.push(Span::Wave {
        length: ridge_len,
        amplitude: RIDGE_AMPLITUDE_PCT,
        cycles,
    });
    spans.extend(descent);
    spans.push(Span::Hold {
        length: built.total_h - built.s_flat2,
        grade: 0.0,
    });
    let sum: f64 = spans.iter().map(Span::length).sum();
    if (sum - built.total_h).abs() > 1.0e-4 {
        return Err(format!("grade profile covers {sum} m of {}", built.total_h));
    }
    if rise_of(&spans).abs() > 1.0e-6 {
        return Err(format!("net elevation {} m", rise_of(&spans)));
    }
    Ok(spans)
}

impl Span {
    fn length(&self) -> f64 {
        match *self {
            Self::Hold { length, .. } | Self::Smooth { length, .. } | Self::Wave { length, .. } => {
                length
            }
        }
    }

    fn json(&self) -> Value {
        match *self {
            Self::Hold { length, grade } => json!({
                "type": "hold",
                "length_m": length,
                "grade_pct": grade,
            }),
            Self::Smooth { length, to } => json!({
                "type": "smooth",
                "length_m": length,
                "to_grade_pct": to,
            }),
            Self::Wave {
                length,
                amplitude,
                cycles,
            } => json!({
                "type": "wave",
                "length_m": length,
                "amplitude_pct": amplitude,
                "cycles": cycles,
            }),
        }
    }
}

fn course_json(built: &Built, spans: &[Span], start_x: f64, start_z: f64) -> Value {
    json!({
        "name": "Penguin Pass",
        "start": {"x": start_x, "z": start_z, "heading_rad": PI},
        "segments": built.segments,
        "grade": spans.iter().map(Span::json).collect::<Vec<_>>(),
    })
}

fn print_spans(spans: &[Span]) {
    println!("horizontal_m length_m kind grade_pct");
    let mut cursor = 0.0;
    for span in spans {
        let (kind, grade) = match *span {
            Span::Hold { grade, .. } => ("hold", grade),
            Span::Smooth { to, .. } => ("smooth_to", to),
            Span::Wave {
                amplitude, cycles, ..
            } => {
                println!(
                    "{cursor:>12.1} {length:>8.1} wave ±{amplitude:.2} x{cycles}",
                    length = span.length()
                );
                cursor += span.length();
                continue;
            }
        };
        println!(
            "{cursor:>12.1} {length:>8.1} {kind:<10} {grade:>7.2}",
            length = span.length()
        );
        cursor += span.length();
    }
}

fn fit(ridge: f64) -> Result<f64, String> {
    let mut k = 1.0;
    for _ in 0..8 {
        let built = build(ridge, k);
        let spans = balance(&built)?;
        let value = course_json(&built, &spans, 0.0, 0.0);
        let text = serde_json::to_string(&value).map_err(|err| err.to_string())?;
        let course = Course::from_str(&text).map_err(|err| err.to_string())?;
        let length = course.length_m();
        let ascent = f64::from(course.total_ascent_m());
        println!("fit k={k:.6} length={length:.3} ascent={ascent:.3}");
        if (length - TARGET_LENGTH_M).abs() < 0.01 && (155.0..=210.0).contains(&ascent) {
            return Ok(k);
        }
        k *= TARGET_LENGTH_M / length;
        if !k.is_finite() || !(0.2..5.0).contains(&k) {
            return Err(format!("scale factor diverged to {k}"));
        }
    }
    Err("fit did not reach 5000 m".to_string())
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

fn elevation_table(course: &Course, step_m: f64) -> String {
    let profile = course.profile(step_m);
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

fn region_bounds(course: &Course) -> (f64, f64, f64, f64) {
    let length = course.length_m();
    let mut climb_start = 0.0;
    let mut d = 0.0;
    while d < length {
        if f64::from(course.grade_pct_at(d)) > 2.0 {
            climb_start = d;
            break;
        }
        d += 1.0;
    }
    let mut seen_kick = false;
    let mut climb_end = climb_start;
    d = climb_start;
    while d < length {
        let grade = f64::from(course.grade_pct_at(d));
        if grade >= 9.0 {
            seen_kick = true;
        }
        if seen_kick && grade < 1.0 {
            climb_end = d;
            break;
        }
        d += 1.0;
    }
    let mut descent_start = climb_end;
    d = climb_end;
    while d < length {
        if f64::from(course.grade_pct_at(d)) < -2.0 {
            descent_start = d;
            break;
        }
        d += 1.0;
    }
    let mut seen_steep = false;
    let mut descent_end = length;
    d = descent_start;
    while d < length {
        let grade = f64::from(course.grade_pct_at(d));
        if grade < -8.0 {
            seen_steep = true;
        }
        if seen_steep && grade > -1.0 {
            descent_end = d;
            break;
        }
        d += 1.0;
    }
    (climb_start, climb_end, descent_start, descent_end)
}

fn mean_grade(course: &Course, start: f64, end: f64) -> f64 {
    let mut sum = 0.0;
    let mut n = 0.0;
    let mut d = start;
    while d < end {
        sum += f64::from(course.grade_pct_at(d));
        n += 1.0;
        d += 1.0;
    }
    if n == 0.0 { 0.0 } else { sum / n }
}

fn lap_report(course: &Course, climb_start: f64, climb_end: f64) {
    let mut ride = Ride::new(course.clone(), RiderProfile::default());
    let sample = TrainerSample {
        power_w: Some(200),
        ..TrainerSample::default()
    };
    let lap = course.length_m();
    let mut climb_s = 0.0;
    let mut guard = 0;
    while ride.telemetry().distance_m < lap {
        let at = ride.telemetry().lap_distance_m;
        ride.step(0.05, &sample);
        if at >= climb_start && at < climb_end {
            climb_s += 0.05;
        }
        guard += 1;
        if guard > 400_000 {
            break;
        }
    }
    let frame = ride.telemetry();
    println!(
        "lap 200 W: {:.1} s ({:.1} min), climbing {:.1} s ({:.1} min), distance {:.1} m, lap {}",
        frame.elapsed_s,
        frame.elapsed_s / 60.0,
        climb_s,
        climb_s / 60.0,
        frame.distance_m,
        frame.lap
    );
}

fn publish(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let scratch = PathBuf::from("/tmp/penguin_pass.json");
    fs::write(&scratch, text)?;
    let loaded = fs::read_to_string(&scratch)?;
    let course = Course::from_str(&loaded)?;
    println!("{}", ascii_map(&course));
    println!("distance_m elevation_m grade_pct");
    println!("{}", elevation_table(&course, 100.0));
    let mut crest = 0.0f32;
    let mut crest_at = 0.0;
    let mut d = 0.0;
    while d <= course.length_m() {
        let elev = course.elevation_at(d);
        if elev > crest {
            crest = elev;
            crest_at = d;
        }
        d += 1.0;
    }
    println!("crest {crest:.2} m at {crest_at:.0} m");
    let failures = spec_failures(&course);
    if !failures.is_empty() {
        return Err(format!("spec failed: {}", failures.join("; ")).into());
    }
    let (climb_start, climb_end, descent_start, descent_end) = region_bounds(&course);
    let average = mean_grade(&course, climb_start, climb_end);
    println!(
        "climb {:.0}..{:.0} m average grade {average:.2}%, descent {:.0}..{:.0} m",
        climb_start, climb_end, descent_start, descent_end
    );
    lap_report(&course, climb_start, climb_end);
    let dest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("courses/penguin_pass.json");
    let staging = dest.with_extension("json.staging");
    fs::write(&staging, text)?;
    fs::rename(&staging, &dest)?;
    println!("replaced {}", dest.display());
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ridge = solve_ridge()?;
    println!("solved unscaled ridge = {ridge:.3} m");
    let k = fit(ridge)?;
    let built = build(ridge, k);
    let spans = balance(&built)?;
    print_spans(&spans);
    let rough = course_json(&built, &spans, 0.0, 0.0);
    let rough_course = Course::from_str(&rough.to_string())?;
    let (min_x, max_x, min_z, max_z) = bbox(&rough_course);
    let value = course_json(
        &built,
        &spans,
        -0.5 * (min_x + max_x),
        -0.5 * (min_z + max_z),
    );
    let text = serde_json::to_string_pretty(&value)? + "\n";
    println!(
        "physics flat 200 W = {:.2} km/h, 8% 250 W = {:.2} km/h, cap = {:.2} km/h",
        equilibrium_kph(200.0, 0.0),
        equilibrium_kph(250.0, 8.0),
        MAX_SPEED_MPS * 3.6
    );
    publish(&text)?;
    Ok(())
}
