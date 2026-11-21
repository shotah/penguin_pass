//! Distant skyline. Rings of ridged (or mesa) silhouettes, lighter with distance.

use crate::theme::Backdrop;

/// One corner of a backdrop triangle, in metres, with a flat colour.
#[derive(Clone, Copy, Debug)]
pub struct Vert {
    /// World X.
    pub x: f32,
    /// World Y.
    pub y: f32,
    /// World Z.
    pub z: f32,
    /// Red, 0..1.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
}

/// One flat-shaded facet.
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    /// Counter-clockwise when seen from the origin.
    pub v: [Vert; 3],
}

/// Build every ring. `reach_m` is the furthest road sample from the origin.
pub fn build(spec: &Backdrop, reach_m: f32, seed: u32) -> Vec<Tri> {
    let mut tris = Vec::new();
    let mesa = spec.style == "mesas";
    for ring in 0..spec.rings {
        let radius = reach_m + spec.inner_margin_m + ring as f32 * spec.ring_gap_m;
        let fade = ring as f32 / spec.rings.max(1) as f32;
        push_ring(&mut tris, spec, radius, fade, seed.wrapping_add(ring * 17), mesa);
    }
    tris
}

fn push_ring(out: &mut Vec<Tri>, spec: &Backdrop, radius: f32, fade: f32, seed: u32, mesa: bool) {
    let n = spec.segments.max(3);
    let mut crest = Vec::with_capacity(n as usize);
    for index in 0..n {
        let angle = index as f32 / n as f32 * std::f32::consts::TAU;
        let noise = noise1(angle * 3.0 + seed as f32 * 0.01, seed);
        let shaped = if mesa {
            (noise * 3.0).floor() / 2.0
        } else {
            noise
        };
        let height = spec.height_m[0] + (spec.height_m[1] - spec.height_m[0]) * shaped;
        let height = height * (1.0 - fade * 0.28);
        let shoulder = radius + if mesa { 80.0 + noise * 40.0 } else { noise * 70.0 - 20.0 };
        crest.push((angle, height.max(40.0), shoulder));
    }
    let rock = lerp_rgb([0.38, 0.42, 0.48], [0.62, 0.70, 0.82], fade);
    let snow = lerp_rgb([0.90, 0.92, 0.94], [0.78, 0.84, 0.92], fade);
    let base_y = -spec.base_drop_m;
    for index in 0..n as usize {
        let next = (index + 1) % n as usize;
        let (a0, h0, s0) = crest[index];
        let (a1, h1, s1) = crest[next];
        let snowline = spec.snow_fraction * h0.max(h1) * (0.85 + 0.2 * noise1(a0 * 2.0, seed));
        let p0 = polar(radius * 0.92, base_y, a0, rock);
        let p1 = polar(radius * 0.92, base_y, a1, rock);
        let top0 = peak(s0, h0, a0, snowline, rock, snow);
        let top1 = peak(s1, h1, a1, snowline, rock, snow);
        out.push(Tri { v: [p0, p1, top1] });
        out.push(Tri { v: [p0, top1, top0] });
    }
}

fn peak(radius: f32, height: f32, angle: f32, snowline: f32, rock: [f32; 3], snow: [f32; 3]) -> Vert {
    let colour = if height >= snowline { snow } else { rock };
    polar(radius, height, angle, colour)
}

fn polar(radius: f32, y: f32, angle: f32, colour: [f32; 3]) -> Vert {
    Vert {
        x: angle.cos() * radius,
        y,
        z: angle.sin() * radius,
        r: colour[0],
        g: colour[1],
        b: colour[2],
    }
}

fn lerp_rgb(near: [f32; 3], far: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        near[0] + (far[0] - near[0]) * t,
        near[1] + (far[1] - near[1]) * t,
        near[2] + (far[2] - near[2]) * t,
    ]
}

fn noise1(x: f32, seed: u32) -> f32 {
    let i = x.floor();
    let f = x - i;
    let smooth = f * f * (3.0 - 2.0 * f);
    let a = hash(i as i32, seed);
    let b = hash(i as i32 + 1, seed);
    a + (b - a) * smooth
}

fn hash(cell: i32, seed: u32) -> f32 {
    let n = (cell as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(seed)
        .wrapping_mul(0x85EB_CA6B);
    (n >> 8) as f32 / 16_777_216.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Backdrop;

    #[test]
    fn rings_sit_outside_the_road_and_the_far_ones_are_bluer() {
        let spec = Backdrop::default();
        let tris = build(&spec, 800.0, 7);
        assert!(tris.len() > 100);
        let near = tris[0].v[0];
        let far_start = tris.len() / spec.rings as usize;
        let far = tris[far_start].v[2];
        let near_r = (near.x * near.x + near.z * near.z).sqrt();
        let far_r = (far.x * far.x + far.z * far.z).sqrt();
        assert!(near_r > 800.0);
        assert!(far_r > near_r);
        assert!(far.b + 0.02 >= near.b);
        assert!(tris.iter().all(|tri| tri.v.iter().all(|v| v.y.is_finite())));
    }

    #[test]
    fn mesas_reuse_heights_and_zero_rings_draw_nothing() {
        let mut spec = Backdrop::default();
        spec.rings = 0;
        assert!(build(&spec, 100.0, 1).is_empty());
        spec.rings = 1;
        spec.style = "mesas".to_owned();
        spec.segments = 24;
        let tris = build(&spec, 100.0, 2);
        let mut heights: Vec<i32> = tris
            .iter()
            .flat_map(|tri| tri.v.iter().map(|v| (v.y * 2.0).round() as i32))
            .filter(|y| *y > 0)
            .collect();
        heights.sort_unstable();
        heights.dedup();
        assert!(heights.len() <= 8, "mesa heights {}", heights.len());
    }
}
