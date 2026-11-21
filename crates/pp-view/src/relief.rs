//! A road cut into flowing ground.
//!
//! The asphalt, the verge, and a short shoulder stay at the road's own
//! height. From the shoulder the ground becomes the natural surface: a
//! smooth blend of the nearby road elevations, plus a low-frequency
//! sidehill, plus the big range further out. The bank leaving the shoulder
//! is capped at a slope grass can hold, so it cannot stand up as a wall.

use crate::roadbed::RoadIndex;

/// Half-width of the flat bed: asphalt, verge, and about a metre of shoulder.
pub const FLAT_M: f32 = 6.2;
/// Steepest bank, about 32 degrees. `tan` of the angle.
pub const BANK_TAN: f32 = 0.62;

/// How the ground grows into a range. Distances are metres.
#[derive(Clone, Debug)]
pub struct Relief {
    /// Height of the distant ridges, metres.
    pub amplitude_m: f32,
    /// Base wavelength of those ridges, metres.
    pub wavelength_m: f32,
    /// Octaves of the distant ridged noise. Few, so the forms stay broad.
    pub octaves: u32,
    /// Altitude above which the theme paints snow, metres.
    pub snow_line_m: f32,
    /// Snap the distant relief to flat steps. Desert mesas. Never alpine.
    pub terraced: bool,
    /// Step height when `terraced` is set, metres.
    pub terrace_m: f32,
    /// Kept so an older caller still compiles. The bank no longer waits for it.
    pub rise_near_m: f32,
    /// Distance where the distant ridges are at full strength, metres.
    pub rise_far_m: f32,
    /// Sidehill amplitude, metres. Tens of metres, wavelength of several hundred.
    pub side_m: f32,
}

impl Default for Relief {
    fn default() -> Self {
        Self {
            amplitude_m: 260.0,
            wavelength_m: 1_500.0,
            octaves: 3,
            snow_line_m: 150.0,
            terraced: false,
            terrace_m: 40.0,
            rise_near_m: 40.0,
            rise_far_m: 1_200.0,
            side_m: 26.0,
        }
    }
}

/// Ground height at `(x, z)`.
///
/// Inside [`FLAT_M`] of a road this is that road's height. Further out it is
/// the natural surface, pulled back so the slope away from the nearest road
/// stays under [`BANK_TAN`].
#[must_use]
pub fn land_height(x: f32, z: f32, roads: &RoadIndex, relief: &Relief, seed: u32) -> f32 {
    let (dist, road_y, field_y) = road_field(roads, x, z);
    compose(x, z, dist, road_y, field_y, relief, seed)
}

/// The bank, given the nearest road and the blended hillside already found.
#[must_use]
pub fn compose(
    x: f32,
    z: f32,
    dist: f32,
    road_y: f32,
    field_y: f32,
    relief: &Relief,
    seed: u32,
) -> f32 {
    if dist <= FLAT_M {
        return road_y;
    }
    let (wx, wz) = warp(x, z, seed);
    let side = sidehill(wx, wz, seed) * relief.side_m;
    let far_mix = smoothstep(120.0, relief.rise_far_m.max(400.0), dist);
    let mut far = (ridged(
        wx,
        wz,
        relief.wavelength_m.max(400.0),
        relief.octaves.clamp(1, 4),
        seed,
    ) - 0.38)
        * relief.amplitude_m
        * far_mix;
    if relief.terraced {
        let stepped = terrace(field_y + side + far, relief.terrace_m.max(8.0));
        let mesa = smoothstep(80.0, 240.0, dist);
        far = (field_y + side + far) * (1.0 - mesa) + stepped * mesa - field_y - side;
    }
    let detail = value_noise(x / 70.0, z / 70.0, seed.wrapping_add(41)) * 1.3;
    // One smooth surface. The run of the bank grows with the height
    // difference, so the slope stays under the cap and never steps.
    let natural = field_y + side + far + detail;
    if dist <= FLAT_M {
        return road_y;
    }
    let delta = natural - road_y;
    let run = FLAT_M + 48.0 + delta.abs() / BANK_TAN;
    let t = smoothstep(FLAT_M, run, dist);
    road_y + delta * t
}

/// A fine vertex on a coarse edge takes the coarse edge's straight height.
#[must_use]
pub fn snapped_edge_height(height_a: f32, height_b: f32, along: f32, a: f32, b: f32) -> f32 {
    let span = b - a;
    let u = if span.abs() < 1.0e-4 {
        0.0
    } else {
        ((along - a) / span).clamp(0.0, 1.0)
    };
    height_a + (height_b - height_a) * u
}

/// Nearest-road height for the bed, and a wider blend for the hillside.
///
/// Both heights are Gaussians over every sample, so the ground between two
/// hairpin legs is the slope between them and does not jump.
fn road_field(roads: &RoadIndex, x: f32, z: f32) -> (f32, f32, f32) {
    let Some((dist, tight, wide)) = roads.blend_heights(x, z) else {
        return (3_000.0, 0.0, 0.0);
    };
    let mix = smoothstep(6.0, 30.0, dist);
    let road_y = tight * (1.0 - mix) + wide * mix;
    (dist, road_y, wide)
}

fn sidehill(x: f32, z: f32, seed: u32) -> f32 {
    // Two long octaves. A road crossing this is a sidehill: up one side, down
    // the other, and the high side changes over a few hundred metres.
    let broad = value_noise(x / 640.0, z / 640.0, seed.wrapping_add(5));
    let mid = value_noise(x / 280.0, z / 280.0, seed.wrapping_add(19));
    broad * 0.78 + mid * 0.22
}

fn warp(x: f32, z: f32, seed: u32) -> (f32, f32) {
    let scale = 1.0 / 900.0;
    let wx = value_noise(x * scale, z * scale, seed.wrapping_add(3)) * 260.0;
    let wz = value_noise(x * scale + 30.0, z * scale, seed.wrapping_add(11)) * 260.0;
    (x + wx, z + wz)
}

/// 0 at and below `edge0`, 1 at and above `edge1`.
#[must_use]
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn ridged(x: f32, z: f32, wavelength: f32, octaves: u32, seed: u32) -> f32 {
    let mut amplitude = 1.0_f32;
    let mut frequency = 1.0 / wavelength;
    let mut sum = 0.0_f32;
    let mut norm = 0.0_f32;
    for octave in 0..octaves {
        let n = value_noise(
            x * frequency,
            z * frequency,
            seed.wrapping_add(octave.wrapping_mul(17)),
        );
        let ridge = 1.0 - n.abs();
        sum += ridge * amplitude;
        norm += amplitude;
        amplitude *= 0.45;
        frequency *= 2.0;
    }
    if norm <= 0.0 {
        0.0
    } else {
        (sum / norm).clamp(0.0, 1.0)
    }
}

fn value_noise(x: f32, z: f32, seed: u32) -> f32 {
    let x0 = x.floor() as i32;
    let z0 = z.floor() as i32;
    let fx = x - x0 as f32;
    let fz = z - z0 as f32;
    let ux = smoothstep(0.0, 1.0, fx);
    let uz = smoothstep(0.0, 1.0, fz);
    let a = hash2(x0, z0, seed);
    let b = hash2(x0 + 1, z0, seed);
    let c = hash2(x0, z0 + 1, seed);
    let d = hash2(x0 + 1, z0 + 1, seed);
    let ab = a + (b - a) * ux;
    let cd = c + (d - c) * ux;
    ab + (cd - ab) * uz
}

fn hash2(ix: i32, iz: i32, seed: u32) -> f32 {
    let mut n = seed
        .wrapping_mul(374_761_393)
        .wrapping_add((ix as u32).wrapping_mul(668_265_263))
        .wrapping_add((iz as u32).wrapping_mul(2_146_121_005));
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    n ^= n >> 16;
    (n as f32) / (u32::MAX as f32) * 2.0 - 1.0
}

fn terrace(height: f32, step: f32) -> f32 {
    let u = height / step;
    let q = u.floor();
    let along = u - q;
    let climb = smoothstep(0.82, 1.0, along);
    (q + climb) * step
}

#[cfg(test)]
mod tests {
    use super::*;
    use pp_routegen::{RouteSpec, Terrain, TerrainPreset, generate};
    use pp_session::Course;

    fn samples_of(course: &Course) -> Vec<[f32; 3]> {
        let mut out = Vec::new();
        let mut distance = 0.0;
        let length = course.length_m();
        while distance < length {
            let p = course.position_at(distance);
            out.push([p[0], p[1], p[2]]);
            distance += 2.0;
        }
        out
    }

    fn assert_bed_and_bank(course: &Course, relief: &Relief) {
        let samples = samples_of(course);
        let roads = RoadIndex::build(&samples);
        let mut distance = 0.0;
        let length = course.length_m();
        while distance < length {
            let p = course.position_at(distance);
            let tangent = course.tangent_at(distance);
            let mut right_x = -tangent[2];
            let mut right_z = tangent[0];
            let span = (right_x * right_x + right_z * right_z).sqrt();
            if span > 1.0e-4 {
                right_x /= span;
                right_z /= span;
            }
            for offset in [-4.0_f32, -2.0, 0.0, 2.0, 4.0] {
                let x = p[0] + right_x * offset;
                let z = p[2] + right_z * offset;
                let h = land_height(x, z, &roads, relief, 7);
                let (dist, road_y, _) = road_field(&roads, x, z);
                if dist <= 4.5 {
                    assert!(
                        (h - road_y).abs() < 0.04,
                        "bed at {distance:.0} m offset {offset} is {h}, road {road_y}"
                    );
                }
            }
            for step in 0..=16 {
                let offset = 6.2 + step as f32 * 2.5;
                let x = p[0] + right_x * offset;
                let z = p[2] + right_z * offset;
                let h = land_height(x, z, &roads, relief, 7);
                let (dist, road_y, _) = road_field(&roads, x, z);
                let allowance = (dist - FLAT_M).max(0.0) * BANK_TAN + 0.05;
                assert!(
                    (h - road_y).abs() <= allowance,
                    "bank at {distance:.0} m offset {offset} rises {} over {dist:.1} m (cap {allowance:.1})",
                    (h - road_y).abs()
                );
            }
            distance += 40.0;
        }
    }

    #[test]
    fn the_built_in_loop_keeps_its_bed_and_its_bank() {
        assert_bed_and_bank(&Course::penguin_pass(), &Relief::default());
    }

    #[test]
    fn a_generated_mountain_keeps_its_bed_and_its_bank() {
        let spec = RouteSpec::new(
            "mountain",
            11,
            10_000.0,
            Terrain::Preset(TerrainPreset::Mountain),
        );
        let course = generate(&spec).expect("mountain").course().clone();
        assert_bed_and_bank(&course, &Relief::default());
    }

    fn assert_no_steps(course: &Course, relief: &Relief) {
        let samples = samples_of(course);
        let roads = RoadIndex::build(&samples);
        let origin = course.position_at(course.length_m() * 0.25);
        let lines = [
            (origin[0], origin[2], 1.0_f32, 0.0_f32),
            (origin[0], origin[2], 0.0, 1.0),
            (origin[0] - 400.0, origin[2] - 400.0, 0.7, 0.7),
        ];
        for (x0, z0, dx, dz) in lines {
            let mut previous = land_height(x0, z0, &roads, relief, 7);
            for step in 1..700 {
                let x = x0 + dx * step as f32;
                let z = z0 + dz * step as f32;
                let h = land_height(x, z, &roads, relief, 7);
                let jump = (h - previous).abs();
                assert!(jump <= 2.5, "step of {jump:.2} m at ({x:.0}, {z:.0})");
                previous = h;
            }
        }
    }

    #[test]
    fn the_surface_has_no_steps_across_either_course() {
        assert_no_steps(&Course::penguin_pass(), &Relief::default());
        let spec = RouteSpec::new(
            "mountain",
            11,
            10_000.0,
            Terrain::Preset(TerrainPreset::Mountain),
        );
        let course = generate(&spec).expect("mountain").course().clone();
        assert_no_steps(&course, &Relief::default());
    }

    #[test]
    fn a_fine_border_vertex_lies_on_the_coarse_edge() {
        let coarse = 64.0_f32;
        let h0 = 12.0_f32;
        let h1 = 28.0_f32;
        for step in 0..=8 {
            let along = step as f32 * 8.0;
            let snapped = snapped_edge_height(h0, h1, along, 0.0, coarse);
            let u = along / coarse;
            let on_edge = h0 + (h1 - h0) * u;
            assert!((snapped - on_edge).abs() < 0.01);
        }
    }

    #[test]
    fn opposite_shoulders_are_not_the_same_flat_apron() {
        let course = Course::penguin_pass();
        let samples = samples_of(&course);
        let roads = RoadIndex::build(&samples);
        let relief = Relief::default();
        let mut differ = 0;
        let mut distance = 200.0;
        while distance < course.length_m() {
            let p = course.position_at(distance);
            let tangent = course.tangent_at(distance);
            let rx = -tangent[2];
            let rz = tangent[0];
            let span = (rx * rx + rz * rz).sqrt().max(1.0e-4);
            let left = land_height(
                p[0] - rx / span * 28.0,
                p[2] - rz / span * 28.0,
                &roads,
                &relief,
                7,
            );
            let right = land_height(
                p[0] + rx / span * 28.0,
                p[2] + rz / span * 28.0,
                &roads,
                &relief,
                7,
            );
            if (left - right).abs() > 1.5 {
                differ += 1;
            }
            distance += 80.0;
        }
        assert!(differ > 8, "shoulders stayed level, only {differ} differed");
    }
}
