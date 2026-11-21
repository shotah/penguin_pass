//! Closed horizontal plans: a rounded loop, or a switchback mountain.
//!
//! Every plan is a loop of lines and circular arcs whose signed turn is
//! exactly one full circle, so the heading closes. Position is closed by
//! solving one or two straight lengths. Uniform scaling later keeps both.

use std::f64::consts::{PI, TAU};

use crate::geom::{
    Role, Seg, angle_diff, closest_approach, closure_error, end_pose, flat_lengths, footprint,
    heading_at, horiz, min_radius, sample_xy, solve_line_for_x,
};
use crate::rng::Stream;
use crate::spec::{
    CENTERLINE_CLEARANCE_M, CLEARANCE_WINDOW_M, MAX_FOOTPRINT_FRACTION, MIN_FOOTPRINT_FRACTION,
    TerrainParams,
};

/// Which plan and grade pattern to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Gentle bends, tiny rolls.
    Flat,
    /// Moderate bends and short hills.
    Rolling,
    /// Switchback climb, ridge, wider descent.
    Mountain,
    /// Long straights and wide bends.
    Desert,
}

impl Style {
    /// Plan style for a terrain value.
    #[must_use]
    pub fn of(terrain: crate::spec::Terrain) -> Self {
        match terrain {
            crate::spec::Terrain::Preset(crate::spec::TerrainPreset::Flat) => Self::Flat,
            crate::spec::Terrain::Preset(crate::spec::TerrainPreset::Rolling) => Self::Rolling,
            crate::spec::Terrain::Preset(crate::spec::TerrainPreset::Mountain) => Self::Mountain,
            crate::spec::Terrain::Preset(crate::spec::TerrainPreset::Desert) => Self::Desert,
            crate::spec::Terrain::Custom(params) => {
                if params.switchbacks_per_10km >= 0.5 && params.max_up_grade_pct >= 5.0 {
                    Self::Mountain
                } else if params.twist <= 0.28 && params.min_radius_m >= 70.0 {
                    Self::Desert
                } else if params.max_up_grade_pct <= 2.5 {
                    Self::Flat
                } else {
                    Self::Rolling
                }
            }
        }
    }
}

/// A closed horizontal path.
pub struct Plan {
    /// Segments, in ride order.
    pub segs: Vec<Seg>,
    /// Heading at the start, in radians, before the final rotation.
    pub h0: f64,
}

/// Build a closed plan whose horizontal length is near `h_target` meters.
pub fn build(
    rng: &mut Stream,
    h_target: f64,
    params: &TerrainParams,
    style: Style,
) -> Result<Plan, String> {
    match style {
        Style::Mountain => mountain(rng, h_target, params),
        Style::Flat | Style::Rolling | Style::Desert => winding(rng, h_target, params, style),
    }
}

/// Reject a plan that is too tight, too sprawling, or too close to itself.
pub fn check_shape(segs: &[Seg], h0: f64, distance_m: f64, radius_floor_m: f64) -> Result<(), String> {
    let (gap, herr) = closure_error(segs, h0);
    if gap > 1.0e-4 || herr > 1.0e-6 {
        return Err(format!("plan does not close (gap {gap:.6} m, heading {herr:.3e} rad)"));
    }
    let radius = min_radius(segs);
    if radius + 1.0e-6 < radius_floor_m {
        return Err(format!("radius {radius:.2} m is under the floor {radius_floor_m:.2} m"));
    }
    let (flat1, flat2) = flat_lengths(segs);
    if flat1 + flat2 < 150.0 {
        return Err(format!("start straight is {:.1} m", flat1 + flat2));
    }
    let step = 4.0;
    let pts = sample_xy(segs, h0, step);
    let length = horiz(segs);
    let along_step = length / pts.len() as f64;
    let gap_m = closest_approach(&pts, along_step, length, CLEARANCE_WINDOW_M);
    // Sampled clearance is a little pessimistic, so the generator asks for
    // more than the 25 m the tests require.
    if gap_m < CENTERLINE_CLEARANCE_M + 6.0 {
        return Err(format!("centerlines approach within {gap_m:.1} m"));
    }
    let (east, north) = footprint(&pts);
    let long = east.max(north);
    let short = east.min(north);
    if long > MAX_FOOTPRINT_FRACTION * distance_m {
        return Err(format!("footprint {long:.0} m is too wide for {distance_m:.0} m"));
    }
    if long < MIN_FOOTPRINT_FRACTION * distance_m {
        return Err(format!("footprint {long:.0} m is a tight spiral"));
    }
    if short < 45.0 || long > short * 4.2 {
        return Err(format!("footprint {east:.0} by {north:.0} m is too skinny"));
    }
    Ok(())
}

/// True when every arc respects the ordinary-bend floor, or the hairpin
/// floor when the sweep is a switchback (more than about 126°).
pub fn radii_ok(segs: &[Seg], params: &TerrainParams) -> Result<(), String> {
    for seg in segs {
        let (Some(radius), Some(angle)) = (seg.radius(), seg.angle()) else {
            continue;
        };
        let floor = if angle.abs() > 2.2 {
            params.switchback_radius_m
        } else {
            params.min_radius_m
        };
        if radius + 1.0e-4 < floor {
            return Err(format!("arc radius {radius:.2} m is under {floor:.2} m"));
        }
    }
    Ok(())
}

fn winding(
    rng: &mut Stream,
    h_target: f64,
    params: &TerrainParams,
    style: Style,
) -> Result<Plan, String> {
    let sign = if rng.coin() { 1.0 } else { -1.0 };
    let n = corner_count(rng, h_target, params, style);
    let (min_a, max_a) = angle_bounds(style, params.twist);
    let angles = sample_angles(rng, n, min_a, max_a, TAU).ok_or("angles do not sum to a circle")?;
    let angles: Vec<f64> = angles.into_iter().map(|a| a * sign).collect();
    let radii = corner_radii(rng, &angles, h_target, params, style)?;
    let arc_len: f64 = radii
        .iter()
        .zip(&angles)
        .map(|(r, a)| r * a.abs())
        .sum();
    let line_budget = h_target * 0.96 - arc_len;
    let flat = (h_target * 0.045).clamp(220.0, 1_400.0);
    let min_line = (h_target * 0.015).clamp(40.0, 220.0);
    if line_budget < flat + min_line * (n.saturating_sub(1) as f64) {
        return Err("bends consume the route before the straights fit".to_string());
    }
    let contrast = match style {
        Style::Desert => 0.85,
        Style::Flat => 0.5,
        Style::Rolling => 0.7,
        Style::Mountain => 0.5,
    };
    let rest = sample_lengths(rng, n - 1, line_budget - flat, min_line, contrast)?;
    let mut lines = Vec::with_capacity(n);
    lines.push(flat);
    lines.extend(rest);
    let max_line = match style {
        Style::Desert => h_target * 0.42,
        _ => h_target * 0.34,
    };
    let mut segs = emit_loop(&lines, &angles, &radii);
    let h0 = 0.0;
    if !close_polygon(&mut segs, h0, n, min_line, max_line) {
        return Err("rounded loop did not close with usable straights".to_string());
    }
    let (gap, herr) = closure_error(&segs, h0);
    if gap > 1.0e-5 || herr > 1.0e-7 {
        return Err(format!("loop gap {gap} m, heading {herr}"));
    }
    Ok(Plan { segs, h0 })
}

fn corner_count(rng: &mut Stream, h_target: f64, params: &TerrainParams, style: Style) -> usize {
    let km = h_target / 1000.0;
    let base = match style {
        Style::Desert => 5.2 + params.twist * 3.0,
        Style::Flat => 5.4 + params.twist * 6.0,
        Style::Rolling => 6.2 + params.twist * 7.0,
        Style::Mountain => 6.0,
    };
    let grow = (km / 28.0).min(3.5);
    let n = (base + grow + rng.range(-0.45, 0.45)).round() as usize;
    let (lo, hi) = match style {
        Style::Desert => (5, 9),
        Style::Flat => (5, 11),
        Style::Rolling => (6, 14),
        Style::Mountain => (6, 12),
    };
    n.clamp(lo, hi)
}

fn angle_bounds(style: Style, twist: f64) -> (f64, f64) {
    match style {
        Style::Desert => (0.42, 1.15 + twist),
        Style::Flat => (0.30, 1.05 + twist),
        Style::Rolling => (0.26, 1.15 + twist * 0.7),
        Style::Mountain => (0.3, 1.4),
    }
}

fn sample_angles(rng: &mut Stream, n: usize, min_a: f64, max_a: f64, sum: f64) -> Option<Vec<f64>> {
    if n == 0 || (n as f64) * min_a > sum + 1.0e-9 || (n as f64) * max_a < sum - 1.0e-9 {
        return None;
    }
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = rng.index(i as u32 + 1) as usize;
        order.swap(i, j);
    }
    let mut angles = vec![min_a; n];
    let mut remain = sum - (n as f64) * min_a;
    let cap = max_a - min_a;
    for (k, &slot) in order.iter().enumerate() {
        let slots_left = n - k;
        let min_take = (remain - cap * (slots_left - 1) as f64).max(0.0);
        let max_take = remain.min(cap);
        let take = if slots_left == 1 {
            remain
        } else {
            rng.range(min_take, max_take.max(min_take))
        };
        angles[slot] += take;
        remain -= take;
    }
    if remain.abs() > 1.0e-6 {
        return None;
    }
    Some(angles)
}

fn corner_radii(
    rng: &mut Stream,
    angles: &[f64],
    h_target: f64,
    params: &TerrainParams,
    style: Style,
) -> Result<Vec<f64>, String> {
    let spread = match style {
        Style::Desert => 3.2,
        Style::Flat => 1.3,
        Style::Rolling => 1.9,
        Style::Mountain => 1.2,
    };
    let mut radii: Vec<f64> = angles
        .iter()
        .map(|_| params.min_radius_m * (1.12 + rng.f64() * spread))
        .collect();
    let arc_len = |r: &[f64]| {
        r.iter()
            .zip(angles)
            .map(|(radius, angle)| radius * angle.abs())
            .sum::<f64>()
    };
    let cap = h_target * 0.58;
    let now = arc_len(&radii);
    if now > cap {
        let k = cap / now;
        for radius in &mut radii {
            *radius = (*radius * k).max(params.min_radius_m * 1.04);
        }
    }
    if arc_len(&radii) > h_target * 0.72 {
        return Err("the bends are longer than the route".to_string());
    }
    Ok(radii)
}

fn sample_lengths(
    rng: &mut Stream,
    n: usize,
    total: f64,
    min_each: f64,
    contrast: f64,
) -> Result<Vec<f64>, String> {
    if n == 0 {
        return Ok(Vec::new());
    }
    if total < min_each * n as f64 {
        return Err("not enough length for the straights".to_string());
    }
    let mut weights: Vec<f64> = (0..n)
        .map(|_| {
            let u = rng.f64();
            (1.0 - contrast) + contrast * u * u * 3.2
        })
        .collect();
    if let Some(big) = weights.iter_mut().max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)) {
        *big *= 1.35;
    }
    let wsum: f64 = weights.iter().sum();
    let extra = total - min_each * n as f64;
    let mut lens: Vec<f64> = weights
        .iter()
        .map(|w| min_each + extra * w / wsum)
        .collect();
    let sum: f64 = lens.iter().sum();
    if let Some(last) = lens.last_mut() {
        *last += total - sum;
    }
    if lens.iter().any(|len| *len < min_each * 0.98) {
        return Err("a straight collapsed".to_string());
    }
    Ok(lens)
}

/// `line[0]` is the start/finish straight and is split across the seam.
fn emit_loop(lines: &[f64], angles: &[f64], radii: &[f64]) -> Vec<Seg> {
    let n = lines.len();
    let mut segs = Vec::with_capacity(n * 2 + 1);
    segs.push(Seg::line(lines[0] * 0.5, Role::Flat));
    for i in 0..n {
        segs.push(Seg::arc(radii[i], angles[i], Role::Valley));
        if i + 1 == n {
            segs.push(Seg::line(lines[0] * 0.5, Role::Flat));
        } else {
            segs.push(Seg::line(lines[i + 1], Role::Valley));
        }
    }
    segs
}

fn close_polygon(
    segs: &mut Vec<Seg>,
    h0: f64,
    n_lines: usize,
    min_len: f64,
    max_len: f64,
) -> bool {
    // Segment layout: [flat/2, arc0, line1, arc1, line2, ..., line_{n-1}, arc_{n-1}, flat/2]
    // Full straight k (k >= 1) is at index 2k.
    let mut pairs = Vec::new();
    for i in 1..n_lines {
        for j in (i + 1)..n_lines {
            let ia = 2 * i;
            let ib = 2 * j;
            let ha = heading_at(segs, ia, h0);
            let hb = heading_at(segs, ib, h0);
            let det = ha.cos() * hb.sin() - hb.cos() * ha.sin();
            pairs.push((det.abs(), ia, ib));
        }
    }
    pairs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, ia, ib) in pairs.into_iter().take(8) {
        if close_two(segs, h0, ia, ib, min_len, max_len) {
            return true;
        }
    }
    false
}

fn close_two(
    segs: &mut [Seg],
    h0: f64,
    ia: usize,
    ib: usize,
    min_len: f64,
    max_len: f64,
) -> bool {
    let (Some(old_a), Some(old_b)) = (segs[ia].line_length(), segs[ib].line_length()) else {
        return false;
    };
    segs[ia].set_line_length(0.0);
    segs[ib].set_line_length(0.0);
    let (x, z, _) = end_pose(segs, h0);
    let ha = heading_at(segs, ia, h0);
    let hb = heading_at(segs, ib, h0);
    let det = ha.cos() * hb.sin() - hb.cos() * ha.sin();
    if det.abs() < 0.22 {
        segs[ia].set_line_length(old_a);
        segs[ib].set_line_length(old_b);
        return false;
    }
    // la * u(ha) + lb * u(hb) = -end, with those two straights removed.
    let solved_a = (-x * hb.sin() + z * hb.cos()) / det;
    let solved_b = (-z * ha.cos() + x * ha.sin()) / det;
    if !(solved_a.is_finite()
        && solved_b.is_finite()
        && (min_len..=max_len).contains(&solved_a)
        && (min_len..=max_len).contains(&solved_b))
    {
        segs[ia].set_line_length(old_a);
        segs[ib].set_line_length(old_b);
        return false;
    }
    segs[ia].set_line_length(solved_a);
    segs[ib].set_line_length(solved_b);
    let (gap, herr) = closure_error(segs, h0);
    if gap > 1.0e-4 || herr > 1.0e-6 {
        segs[ia].set_line_length(old_a);
        segs[ib].set_line_length(old_b);
        return false;
    }
    true
}

struct Mtn {
    dir: f64,
    h0: f64,
    flat1: f64,
    flat2: f64,
    leg_c: Vec<f64>,
    leg_d: Vec<f64>,
    r_c: f64,
    r_bow: f64,
    bow: f64,
    bow_len: f64,
    h_target: f64,
}

fn mountain(rng: &mut Stream, h_target: f64, params: &TerrainParams) -> Result<Plan, String> {
    let dir = if rng.coin() { 1.0 } else { -1.0 };
    let mut hairpins = ((h_target / 10_000.0) * params.switchbacks_per_10km)
        .round()
        .clamp(2.0, 16.0) as i32;
    if hairpins % 2 == 1 {
        hairpins += 1;
    }
    let n_c = (hairpins + 1) as usize;
    let n_d = descent_legs(n_c);
    let h = h_target * 0.94;
    let flat = (h * 0.04).clamp(220.0, 1_200.0);
    let bow = rng.range(0.40, 0.78);
    let bow_len = (h * rng.range(0.045, 0.08)).clamp(120.0, 700.0);
    let r_c = params.switchback_radius_m * rng.range(1.12, 1.45);
    let mut r_bow = params.min_radius_m * rng.range(2.4, 4.2);
    let bow_arc = 2.0 * r_bow * bow;
    if bow_arc > h * 0.18 {
        r_bow = (h * 0.18) / (2.0 * bow).max(0.2);
        r_bow = r_bow.max(params.min_radius_m * 1.08);
    }
    let r_d_guess = r_c * 1.75;
    let climb_arcs = PI * r_c * n_c as f64;
    let desc_arcs = PI * r_d_guess * n_d as f64;
    let ridge_guess = (h * 0.11).clamp(140.0, h * 0.18);
    let used = flat + bow_arc + bow_len + climb_arcs + desc_arcs + ridge_guess;
    let remain = (h - used).max(h * 0.28);
    let leg_c = vary_legs(rng, n_c, remain * 0.47 / n_c as f64);
    let leg_d = vary_legs(rng, n_d, remain * 0.53 / n_d as f64);
    if leg_c.iter().any(|l| *l < 90.0) || leg_d.iter().any(|l| *l < 90.0) {
        return Err("switchback legs are shorter than 90 m".to_string());
    }
    let spec = Mtn {
        dir,
        h0: PI,
        flat1: flat * 0.5,
        flat2: flat * 0.5,
        leg_c,
        leg_d,
        r_c,
        r_bow,
        bow,
        bow_len,
        h_target,
    };
    let (segs, _r_d) = close_mountain(&spec, params)?;
    let (gap, herr) = closure_error(&segs, spec.h0);
    if gap > 1.0e-4 || herr > 1.0e-6 {
        return Err(format!("mountain gap {gap} m, heading {herr}"));
    }
    Ok(Plan { segs, h0: spec.h0 })
}

fn descent_legs(n_c: usize) -> usize {
    let n = ((n_c as f64) * 0.62).round() as usize;
    let n = n.max(3);
    if n % 2 == 0 { n + 1 } else { n }.min(n_c)
}

fn vary_legs(rng: &mut Stream, n: usize, mean: f64) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    let mut legs: Vec<f64> = (0..n).map(|_| mean * rng.range(0.78, 1.24)).collect();
    let sum: f64 = legs.iter().sum();
    let target = mean * n as f64;
    if sum > 0.0 {
        for leg in &mut legs {
            *leg *= target / sum;
        }
    }
    legs
}

fn close_mountain(spec: &Mtn, params: &TerrainParams) -> Result<(Vec<Seg>, f64), String> {
    let lo = params.switchback_radius_m * 1.2;
    let mut hi = (spec.r_c * 4.5).max(lo * 1.8);
    let z_lo = z_after_x(spec, lo)?;
    let mut z_hi = match z_after_x(spec, hi) {
        Ok(z) => z,
        Err(_) => {
            hi = lo * 2.5;
            z_after_x(spec, hi)?
        }
    };
    if z_lo * z_hi > 0.0 {
        hi *= 1.7;
        z_hi = z_after_x(spec, hi)?;
    }
    if z_lo * z_hi > 0.0 {
        return Err("descent radius cannot cancel the northing error".to_string());
    }
    let mut a = lo;
    let mut b = hi;
    let mut za = z_lo;
    for _ in 0..28 {
        let mid = 0.5 * (a + b);
        let zm = z_after_x(spec, mid)?;
        if za * zm <= 0.0 {
            b = mid;
        } else {
            a = mid;
            za = zm;
        }
    }
    let r_d = 0.5 * (a + b);
    let segs = assemble_closed(spec, r_d)?;
    let (_, z, h) = end_pose(&segs, spec.h0);
    if z.abs() > 1.0e-3 || angle_diff(h, spec.h0).abs() > 1.0e-6 {
        return Err(format!("mountain residual z {z} m"));
    }
    Ok((segs, r_d))
}

fn z_after_x(spec: &Mtn, r_d: f64) -> Result<f64, String> {
    let segs = assemble_closed(spec, r_d)?;
    let (x, z, h) = end_pose(&segs, spec.h0);
    if x.abs() > 1.0e-3 || angle_diff(h, spec.h0).abs() > 1.0e-5 {
        return Err("x close missed".to_string());
    }
    Ok(z)
}

fn assemble_closed(spec: &Mtn, r_d: f64) -> Result<Vec<Seg>, String> {
    let (segs, ridge_at, candidates) = assemble(spec, r_d, spec.h_target * 0.12);
    let min_ridge = 100.0;
    let max_ridge = spec.h_target * 0.32;
    let min_leg = 80.0;
    let max_leg = spec.h_target * 0.28;
    if let Some(closed) = solve_line_for_x(&segs, spec.h0, &[ridge_at], min_ridge, max_ridge) {
        return Ok(closed);
    }
    for idx in candidates {
        if idx == ridge_at {
            continue;
        }
        if let Some(closed) = solve_line_for_x(&segs, spec.h0, &[idx], min_leg, max_leg) {
            return Ok(closed);
        }
    }
    Err("no straight could absorb the easting error".to_string())
}

fn assemble(spec: &Mtn, r_d: f64, ridge: f64) -> (Vec<Seg>, usize, Vec<usize>) {
    let mut segs = Vec::new();
    let mut candidates = Vec::new();
    let d = spec.dir;
    segs.push(Seg::line(spec.flat1, Role::Flat));
    segs.push(Seg::arc(spec.r_c, -PI * d, Role::Climb));
    push_ladder(&mut segs, &mut candidates, &spec.leg_c, spec.r_c, PI * d, Role::Climb);
    segs.push(Seg::arc(spec.r_bow, spec.bow * d, Role::Crest));
    segs.push(Seg::line(spec.bow_len, Role::Crest));
    segs.push(Seg::arc(spec.r_bow, -spec.bow * d, Role::Crest));
    let ridge_at = segs.len();
    segs.push(Seg::line(ridge, Role::Crest));
    candidates.push(ridge_at);
    segs.push(Seg::arc(r_d, -PI * d, Role::Descent));
    push_ladder(&mut segs, &mut candidates, &spec.leg_d, r_d, PI * d, Role::Descent);
    segs.push(Seg::line(spec.flat2, Role::Flat));
    (segs, ridge_at, candidates)
}

fn push_ladder(
    segs: &mut Vec<Seg>,
    candidates: &mut Vec<usize>,
    legs: &[f64],
    radius: f64,
    first_turn: f64,
    role: Role,
) {
    let mut turn = first_turn;
    for (i, &leg) in legs.iter().enumerate() {
        candidates.push(segs.len());
        segs.push(Seg::line(leg, role));
        if i + 1 == legs.len() {
            break;
        }
        segs.push(Seg::arc(radius, turn, role));
        turn = -turn;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::TerrainPreset;

    #[test]
    fn square_angles_sum_and_solver_closes() {
        let mut rng = Stream::new(3);
        let params = TerrainPreset::Rolling.params();
        let plan = winding(&mut rng, 8_000.0, &params, Style::Rolling).expect("loop");
        let (gap, herr) = closure_error(&plan.segs, plan.h0);
        assert!(gap < 1e-4, "gap {gap}");
        assert!(herr < 1e-6, "heading {herr}");
    }

    #[test]
    fn mountain_closes_for_a_fixed_seed() {
        let mut rng = Stream::new(11);
        let params = TerrainPreset::Mountain.params();
        let plan = mountain(&mut rng, 10_000.0, &params).expect("mountain");
        let (gap, herr) = closure_error(&plan.segs, plan.h0);
        assert!(gap < 1e-4, "gap {gap}");
        assert!(herr < 1e-6, "heading {herr}");
        let (flat1, flat2) = flat_lengths(&plan.segs);
        assert!(flat1 + flat2 >= 150.0, "{} + {}", flat1, flat2);
    }
}
