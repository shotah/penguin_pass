//! Grade profile along horizontal distance.
//!
//! Spans use the same hold / smooth / wave shapes as `pp-course`. A smooth
//! span eases with a smoothstep, so its grade matches the spans on either
//! side and its slope is zero at both ends. Net elevation of a loop is zero
//! because every hill climbs and then descends by the same vertical amount.

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::geom::{RoleLengths, Seg, flat_lengths, horiz, role_lengths};
use crate::plan::Style;
use crate::spec::TerrainParams;

/// One grade span along horizontal distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Span {
    /// Constant grade.
    Hold {
        /// Horizontal length, in meters.
        length: f64,
        /// Grade, in percent.
        grade: f64,
    },
    /// Smoothstep from the previous grade to `to`.
    Smooth {
        /// Horizontal length, in meters.
        length: f64,
        /// Grade at the end of the span, in percent.
        to: f64,
    },
    /// Sine wave that starts and ends at 0% grade.
    Wave {
        /// Horizontal length, in meters.
        length: f64,
        /// Peak grade, in percent. May be negative.
        amplitude: f64,
        /// Whole number of cycles. The span starts and ends at 0%.
        cycles: u32,
    },
}

impl Span {
    /// Horizontal length, in meters.
    #[must_use]
    pub fn length(self) -> f64 {
        match self {
            Self::Hold { length, .. } | Self::Smooth { length, .. } | Self::Wave { length, .. } => {
                length
            }
        }
    }

    fn add_length(&mut self, extra: f64) {
        match self {
            Self::Hold { length, .. } | Self::Smooth { length, .. } | Self::Wave { length, .. } => {
                *length += extra;
            }
        }
    }

    /// JSON object for this span.
    #[must_use]
    pub fn to_json(self) -> Value {
        match self {
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

/// Scale every stored grade by `alpha`. A 0% endpoint stays 0%.
pub fn scale_grades(spans: &mut [Span], alpha: f64) {
    for span in spans {
        match span {
            Span::Hold { grade, .. } => *grade *= alpha,
            Span::Smooth { to, .. } => *to *= alpha,
            Span::Wave { amplitude, .. } => *amplitude *= alpha,
        }
    }
}

/// Scale every span length by `k`.
pub fn scale_lengths(spans: &mut [Span], k: f64) {
    for span in spans {
        match span {
            Span::Hold { length, .. } | Span::Smooth { length, .. } | Span::Wave { length, .. } => {
                *length *= k;
            }
        }
    }
}

/// Make the spans sum to `horizontal_m` by adjusting the last span.
///
/// The last span is the finish flat at 0% grade, so a sub-metre fix does
/// not change elevation.
pub fn sync_length(spans: &mut [Span], horizontal_m: f64) -> Result<(), String> {
    if spans.is_empty() {
        return Err("grade profile is empty".to_string());
    }
    let sum: f64 = spans.iter().map(|s| s.length()).sum();
    let fix = horizontal_m - sum;
    let last = spans.len() - 1;
    spans[last].add_length(fix);
    if spans[last].length() <= 1.0 {
        return Err("finish flat vanished while matching the horizontal length".to_string());
    }
    Ok(())
}

/// Net elevation change of complete spans, in meters.
///
/// Smoothstep integrates to the average of the endpoint grades, which is
/// the same formula `pp-course` uses for a full span.
#[must_use]
pub fn net_elevation_m(spans: &[Span]) -> f64 {
    let mut grade = 0.0;
    let mut rise = 0.0;
    for span in spans {
        match *span {
            Span::Hold { length, grade: g } => {
                rise += g / 100.0 * length;
                grade = g;
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

/// Total ascent, in meters, counting only upward travel.
#[must_use]
pub fn ascent_m(spans: &[Span]) -> f64 {
    let mut grade = 0.0;
    let mut up = 0.0;
    for span in spans {
        match *span {
            Span::Hold { length, grade: g } => {
                if g > 0.0 {
                    up += g / 100.0 * length;
                }
                grade = g;
            }
            Span::Smooth { length, to } => {
                if grade >= 0.0 && to >= 0.0 {
                    up += (length / 100.0 * (grade + to) / 2.0).max(0.0);
                }
                grade = to;
            }
            Span::Wave {
                length, amplitude, ..
            } => {
                up += amplitude.abs() / 100.0 * length / PI;
                grade = 0.0;
            }
        }
    }
    up
}

/// Lowest and highest elevation along the spans, in meters.
///
/// The walk uses the closed-form gain of each full span, and samples waves
/// at eight points per cycle so a crest is not missed.
#[must_use]
pub fn elevation_bounds_m(spans: &[Span]) -> (f64, f64) {
    let mut grade = 0.0_f64;
    let mut elev = 0.0_f64;
    let mut lo = 0.0_f64;
    let mut hi = 0.0_f64;
    for span in spans {
        match *span {
            Span::Hold { length, grade: g } => {
                let end = elev + g / 100.0 * length;
                lo = lo.min(elev).min(end);
                hi = hi.max(elev).max(end);
                elev = end;
                grade = g;
            }
            Span::Smooth { length, to } => {
                let steps = 8;
                for i in 1..=steps {
                    let s = length * (i as f64) / (steps as f64);
                    let t = s / length;
                    let y = elev + smooth_gain(length, grade, to, t);
                    lo = lo.min(y);
                    hi = hi.max(y);
                }
                elev += smooth_gain(length, grade, to, 1.0);
                grade = to;
            }
            Span::Wave {
                length,
                amplitude,
                cycles,
            } => {
                let steps = (cycles * 8).max(8);
                let omega = std::f64::consts::TAU * f64::from(cycles) / length;
                for i in 1..=steps {
                    let s = length * (i as f64) / (steps as f64);
                    let y = if omega == 0.0 {
                        elev
                    } else {
                        elev + (amplitude / 100.0) / omega * (1.0 - (omega * s).cos())
                    };
                    lo = lo.min(y);
                    hi = hi.max(y);
                }
                elev += 0.0;
                grade = 0.0;
            }
        }
    }
    (lo, hi)
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn smooth_gain(length: f64, from: f64, to: f64, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let s = t * length;
    let t3 = t * t * t;
    let integral = t3 - 0.5 * t3 * t;
    (from / 100.0) * s + ((to - from) / 100.0) * length * integral
}

/// Grade at horizontal distance `s_m`, in percent.
#[must_use]
pub fn grade_at(spans: &[Span], s_m: f64) -> f64 {
    let mut cursor = 0.0;
    let mut prev = 0.0;
    for (index, span) in spans.iter().enumerate() {
        let len = span.length();
        let end = cursor + len;
        let last = index + 1 == spans.len();
        if s_m <= end + 1.0e-9 || last {
            let s = (s_m - cursor).clamp(0.0, len);
            return match *span {
                Span::Hold { grade, .. } => grade,
                Span::Smooth { length, to } => {
                    let t = if length > 0.0 { s / length } else { 0.0 };
                    prev + (to - prev) * smoothstep(t)
                }
                Span::Wave {
                    length,
                    amplitude,
                    cycles,
                } => {
                    let omega = std::f64::consts::TAU * f64::from(cycles) / length;
                    amplitude * (omega * s).sin()
                }
            };
        }
        prev = match *span {
            Span::Hold { grade, .. } => grade,
            Span::Smooth { to, .. } => to,
            Span::Wave { .. } => 0.0,
        };
        cursor = end;
    }
    0.0
}

/// 3D centerline length implied by the horizontal path and this profile, in meters.
///
/// Simpson's rule on 0.5 m steps. Close enough to size the loop; the parsed
/// course is the length that is actually checked.
#[must_use]
pub fn length_3d_m(horizontal_m: f64, spans: &[Span]) -> f64 {
    if horizontal_m <= 0.0 {
        return 0.0;
    }
    let step = 0.5_f64;
    let mut s = 0.0;
    let mut acc = 0.0;
    while s < horizontal_m - 1.0e-9 {
        let ds = step.min(horizontal_m - s);
        let g0 = grade_at(spans, s);
        let gm = grade_at(spans, s + 0.5 * ds);
        let g1 = grade_at(spans, s + ds);
        let factor = |g: f64| (1.0 + (g / 100.0) * (g / 100.0)).sqrt();
        acc += ds / 6.0 * (factor(g0) + 4.0 * factor(gm) + factor(g1));
        s += ds;
    }
    acc
}

/// Build a grade profile for a closed plan.
///
/// `target_ascent_m` is the vertical metres the rider should climb over the
/// whole loop. The profile starts and ends at 0% so the lap seam is flat.
pub fn build(
    segs: &[Seg],
    target_ascent_m: f64,
    params: &TerrainParams,
    style: Style,
    rng: &mut crate::rng::Stream,
) -> Result<Vec<Span>, String> {
    let (flat1, flat2) = flat_lengths(segs);
    if flat1 < 70.0 || flat2 < 70.0 {
        return Err("start straight is shorter than 70 m on a side".to_string());
    }
    let roles = role_lengths(segs);
    let middle = horiz(segs) - flat1 - flat2;
    if middle < 200.0 {
        return Err("middle of the loop is too short for a grade profile".to_string());
    }
    let n = hill_count(middle, target_ascent_m, params, style);
    let spans = if roles.climb > 1.0 && n == 1 && roles.valley < 1.0 {
        slotted(&roles, flat1, flat2, target_ascent_m, params, style, rng)?
    } else {
        free(
            (flat1, middle, flat2),
            target_ascent_m,
            params,
            style,
            n,
            rng,
        )?
    };
    check_profile(&spans, params, target_ascent_m)?;
    Ok(spans)
}

fn hill_count(middle: f64, target: f64, params: &TerrainParams, style: Style) -> usize {
    let style_max = match style {
        Style::Mountain => 4,
        Style::Desert => 3,
        Style::Rolling => 12,
        Style::Flat => 6,
    };
    let by_len = (middle / params.typical_climb_m)
        .round()
        .clamp(1.0, style_max as f64) as usize;
    let by_peak = (target / 850.0).ceil().max(1.0) as usize;
    by_len.max(by_peak).clamp(1, style_max.max(by_peak))
}

fn free(
    ends: (f64, f64, f64),
    target: f64,
    params: &TerrainParams,
    style: Style,
    n: usize,
    rng: &mut crate::rng::Stream,
) -> Result<Vec<Span>, String> {
    let (flat1, middle, flat2) = ends;
    let mut weights = vec![1.0; n];
    for w in &mut weights {
        *w *= rng.range(0.55, 1.7);
    }
    let wsum: f64 = weights.iter().sum();
    let mut lengths: Vec<f64> = weights.iter().map(|w| w / wsum * middle).collect();
    let acc: f64 = lengths.iter().take(n.saturating_sub(1)).sum();
    if let Some(last) = lengths.last_mut() {
        *last = middle - acc;
    }
    let mut spans = vec![Span::Hold {
        length: flat1,
        grade: 0.0,
    }];
    for len in lengths {
        let share = if middle > 0.0 {
            target * len / middle
        } else {
            0.0
        };
        spans.extend(one_hill(len, share, params, style, rng)?);
    }
    spans.push(Span::Hold {
        length: flat2,
        grade: 0.0,
    });
    Ok(spans)
}

fn slotted(
    roles: &RoleLengths,
    flat1: f64,
    flat2: f64,
    target: f64,
    params: &TerrainParams,
    style: Style,
    rng: &mut crate::rng::Stream,
) -> Result<Vec<Span>, String> {
    // One hill: the climb slot is the uphill, the crest is the top, and
    // the descent slot brings the rider back to the datum.
    let climb_len = roles.climb;
    let crest_len = roles.crest;
    let descent_len = roles.descent;
    if descent_len < 80.0 || climb_len < 80.0 {
        return Err("climb or descent slot is too short".to_string());
    }
    let (amp, cycles) = crest_wave(style, crest_len, params, rng);
    let crest = crest_spans(crest_len, amp, cycles, params.max_grade_rate)?;
    let wave_up = ascent_m(&crest);
    let climb_target = (target - wave_up).max(target * 0.5);

    let mut climb = varied_flex(climb_len, style, params.max_up_grade_pct, 1.0, params)?;
    fit_rise(&mut climb, climb_target, params.max_up_grade_pct)?;
    let climb_rise = net_elevation_m(&climb);

    let mut descent = varied_flex(descent_len, style, params.max_down_grade_pct, -1.0, params)?;
    fit_rise(&mut descent, -climb_rise, params.max_down_grade_pct)?;
    // If the descent could not give back all of the climb, shrink the climb.
    let desc_rise = net_elevation_m(&descent);
    if climb_rise + desc_rise > 1.0e-4 && climb_rise > 1.0e-6 {
        let fix = (-desc_rise / climb_rise).clamp(0.05, 1.0);
        scale_grades(&mut climb, fix);
    }

    let mut spans = vec![Span::Hold {
        length: flat1,
        grade: 0.0,
    }];
    spans.extend(climb);
    spans.extend(crest);
    spans.extend(descent);
    spans.push(Span::Hold {
        length: flat2,
        grade: 0.0,
    });
    Ok(spans)
}

fn one_hill(
    length: f64,
    share: f64,
    params: &TerrainParams,
    style: Style,
    rng: &mut crate::rng::Stream,
) -> Result<Vec<Span>, String> {
    let crest_frac = match style {
        Style::Mountain => 0.10,
        Style::Desert => 0.06,
        Style::Rolling => 0.10,
        Style::Flat => 0.12,
    };
    let mut crest_len = length * crest_frac;
    let mut climb_len = (length - crest_len) * 0.48;
    let mut descent_len = length - crest_len - climb_len;
    if climb_len < 40.0 || descent_len < 40.0 {
        crest_len = 0.0;
        climb_len = length * 0.5;
        descent_len = length - climb_len;
    }
    let (amp, cycles) = crest_wave(style, crest_len, params, rng);
    let crest = crest_spans(crest_len, amp, cycles, params.max_grade_rate)?;
    let wave_up = ascent_m(&crest);
    let climb_target = (share - wave_up).max(0.0);

    let mut climb = varied_flex(climb_len, style, params.max_up_grade_pct, 1.0, params)?;
    fit_rise(&mut climb, climb_target, params.max_up_grade_pct)?;
    let climb_rise = net_elevation_m(&climb);

    let mut descent = varied_flex(descent_len, style, params.max_down_grade_pct, -1.0, params)?;
    fit_rise(&mut descent, -climb_rise, params.max_down_grade_pct)?;
    let desc_rise = net_elevation_m(&descent);
    if climb_rise + desc_rise > 1.0e-4 && climb_rise > 1.0e-6 {
        scale_grades(&mut climb, (-desc_rise / climb_rise).clamp(0.05, 1.0));
    }
    let mut spans = climb;
    spans.extend(crest);
    spans.extend(descent);
    Ok(spans)
}

fn crest_wave(
    style: Style,
    length: f64,
    params: &TerrainParams,
    rng: &mut crate::rng::Stream,
) -> (f64, u32) {
    if length < 180.0 || matches!(style, Style::Flat) {
        return (0.0, 1);
    }
    let amp = match style {
        Style::Mountain => rng.range(1.6, 3.4).min(params.max_up_grade_pct * 0.45),
        Style::Rolling => rng.range(1.2, 2.6).min(params.max_up_grade_pct * 0.55),
        Style::Desert => rng.range(0.6, 1.4).min(params.max_up_grade_pct * 0.35),
        Style::Flat => 0.0,
    };
    let cycles = if length > 900.0 && rng.coin() { 2 } else { 1 };
    (amp, cycles)
}

fn crest_spans(length: f64, amp: f64, cycles: u32, rate: f64) -> Result<Vec<Span>, String> {
    if length <= 0.5 {
        return Ok(Vec::new());
    }
    if amp < 0.05 {
        return Ok(vec![Span::Hold { length, grade: 0.0 }]);
    }
    // Max slope of A sin(2π n s / L) is A * 2π n / L.
    let max_cycles = ((rate * length) / (amp * std::f64::consts::TAU))
        .floor()
        .clamp(1.0, 4.0) as u32;
    let cycles = cycles.clamp(1, max_cycles.max(1));
    let slope = amp * std::f64::consts::TAU * f64::from(cycles) / length;
    if slope > rate * 1.02 {
        return Ok(vec![Span::Hold { length, grade: 0.0 }]);
    }
    Ok(vec![Span::Wave {
        length,
        amplitude: amp,
        cycles,
    }])
}

fn fit_rise(spans: &mut [Span], desired: f64, max_grade: f64) -> Result<(), String> {
    let now = net_elevation_m(spans);
    if now.abs() < 1.0e-9 {
        return if desired.abs() < 1.0e-6 {
            Ok(())
        } else {
            Err("grade region has no area to scale".to_string())
        };
    }
    let peak = peak_grade(spans).max(1.0e-6);
    let cap = (max_grade - 0.03).max(0.2) / peak;
    let beta = (desired / now).clamp(0.0, cap);
    scale_grades(spans, beta);
    Ok(())
}

fn peak_grade(spans: &[Span]) -> f64 {
    spans
        .iter()
        .map(|span| match *span {
            Span::Hold { grade, .. } => grade.abs(),
            Span::Smooth { to, .. } => to.abs(),
            Span::Wave { amplitude, .. } => amplitude.abs(),
        })
        .fold(0.0, f64::max)
}

fn varied_flex(
    length: f64,
    style: Style,
    max_grade: f64,
    sign: f64,
    params: &TerrainParams,
) -> Result<Vec<Span>, String> {
    let patterns = patterns(style, max_grade, sign);
    let mut last = String::from("no grade pattern fitted");
    for (levels, weights) in patterns {
        match varied(length, &levels, &weights, params.max_grade_rate) {
            Ok(spans) => return Ok(spans),
            Err(err) => last = err,
        }
    }
    Err(last)
}

fn patterns(style: Style, max_grade: f64, sign: f64) -> Vec<(Vec<f64>, Vec<f64>)> {
    let g = |frac: f64| sign * max_grade * frac;
    let full: (Vec<f64>, Vec<f64>) = match style {
        Style::Mountain => (
            vec![0.0, g(0.72), g(0.36), g(0.94), g(0.64), 0.0],
            vec![1.25, 0.55, 0.90, 1.15],
        ),
        Style::Desert => (
            vec![0.0, g(0.34), g(0.24), g(0.90), g(0.30), 0.0],
            vec![1.7, 1.2, 0.42, 1.45],
        ),
        Style::Rolling => (
            vec![0.0, g(0.70), g(0.40), g(0.96), g(0.58), 0.0],
            vec![1.1, 0.5, 0.85, 1.0],
        ),
        Style::Flat => (
            vec![0.0, g(0.55), g(0.28), g(0.90), g(0.42), 0.0],
            vec![1.2, 0.6, 0.7, 1.0],
        ),
    };
    let mid = (vec![0.0, g(0.62), g(0.92), 0.0], vec![1.3, 0.8]);
    let plain = (vec![0.0, g(0.75), 0.0], vec![1.0]);
    vec![full, mid, plain]
}

/// Varied grades from `levels[0]` to the last level, with a hold at each
/// interior level. `weights` has one entry per interior level.
fn varied(length: f64, levels: &[f64], weights: &[f64], rate: f64) -> Result<Vec<Span>, String> {
    if levels.len() < 2 || weights.len() + 2 != levels.len() {
        return Err("grade pattern is malformed".to_string());
    }
    let mut trans = Vec::with_capacity(levels.len() - 1);
    for pair in levels.windows(2) {
        trans.push(transition_len(pair[0], pair[1], rate));
    }
    let tsum: f64 = trans.iter().sum();
    if tsum > length * 0.92 {
        return Err("grade transitions do not fit in the hill".to_string());
    }
    let hold_budget = length - tsum;
    let wsum: f64 = weights.iter().sum();
    if wsum <= 0.0 {
        return Err("grade weights sum to zero".to_string());
    }
    let mut holds: Vec<f64> = weights.iter().map(|w| w / wsum * hold_budget).collect();
    let hsum: f64 = holds.iter().sum();
    if let Some(last) = holds.last_mut() {
        *last += hold_budget - hsum;
    }
    let mut spans = Vec::new();
    let mut hold_i = 0;
    for (i, tlen) in trans.iter().enumerate() {
        let to = levels[i + 1];
        let from = levels[i];
        if (from - to).abs() < 1.0e-9 {
            if *tlen > 0.05 {
                spans.push(Span::Hold {
                    length: *tlen,
                    grade: to,
                });
            }
        } else if *tlen > 0.05 {
            spans.push(Span::Smooth { length: *tlen, to });
        }
        let interior = i + 1 < levels.len() - 1;
        if interior {
            let hl = holds[hold_i];
            hold_i += 1;
            if hl > 0.05 {
                spans.push(Span::Hold {
                    length: hl,
                    grade: to,
                });
            }
        }
    }
    let sum: f64 = spans.iter().map(|s| s.length()).sum();
    if let Some(last) = spans.last_mut() {
        last.add_length(length - sum);
    }
    if spans.iter().any(|s| s.length() <= 0.0) {
        return Err("a grade span collapsed to zero length".to_string());
    }
    Ok(spans)
}

fn transition_len(from: f64, to: f64, rate: f64) -> f64 {
    // Smoothstep's steepest slope is 1.5 * Δgrade / length.
    let need = 1.5 * (to - from).abs() / rate;
    need.max(16.0)
}

fn check_profile(spans: &[Span], params: &TerrainParams, target: f64) -> Result<(), String> {
    let net = net_elevation_m(spans);
    if net.abs() > 1.0e-4 {
        return Err(format!("net elevation is {net} m"));
    }
    let (lo, hi) = elevation_bounds_m(spans);
    if lo < -0.05 {
        return Err(format!("elevation drops to {lo:.2} m"));
    }
    if hi > crate::spec::MAX_ELEVATION_M {
        return Err(format!("elevation reaches {hi:.1} m"));
    }
    let up = ascent_m(spans);
    let tol = crate::spec::ascent_tolerance_m(target);
    if target > 1.0 && (up - target).abs() > tol {
        return Err(format!(
            "ascent {up:.1} m is outside {target:.1} ± {tol:.1} m"
        ));
    }
    let up_lim = params.max_up_grade_pct + 1.0e-6;
    let down_lim = params.max_down_grade_pct + 1.0e-6;
    for span in spans {
        let over = match *span {
            Span::Hold { grade, .. } => grade > up_lim || grade < -down_lim,
            Span::Smooth { to, .. } => to > up_lim || to < -down_lim,
            Span::Wave { amplitude, .. } => amplitude.abs() > up_lim.min(down_lim),
        };
        if over {
            return Err("a grade span exceeds the terrain limit".to_string());
        }
    }
    if spans.first().is_none_or(|s| !is_zero_hold(s))
        || spans.last().is_none_or(|s| !is_zero_hold(s))
    {
        return Err("profile does not start and end on a 0% flat".to_string());
    }
    Ok(())
}

fn is_zero_hold(span: &Span) -> bool {
    matches!(
        span,
        Span::Hold { grade, .. } if grade.abs() < 1.0e-9
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_full_span_matches_average() {
        let gain = smooth_gain(200.0, 2.0, 8.0, 1.0);
        let avg = 200.0 / 100.0 * (2.0 + 8.0) / 2.0;
        assert!((gain - avg).abs() < 1e-9, "{gain} vs {avg}");
    }

    #[test]
    fn wave_returns_to_zero() {
        let spans = [Span::Wave {
            length: 400.0,
            amplitude: 3.0,
            cycles: 2,
        }];
        assert!(net_elevation_m(&spans).abs() < 1e-9);
        let up = ascent_m(&spans);
        let expect = 3.0 / 100.0 * 400.0 / PI;
        assert!((up - expect).abs() < 1e-9, "{up}");
    }
}
