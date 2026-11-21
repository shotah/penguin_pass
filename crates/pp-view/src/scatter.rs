//! Scatter placement. Chunk coordinates and a ground sample in, instances out.
//!
//! The Godot side supplies the height and the road distance. The rules here
//! are what the checksum covers.

use crate::theme::ScatterLayer;

/// What the world knows about one ground point.
#[derive(Clone, Copy, Debug)]
pub struct Ground {
    /// Metres to the road centreline.
    pub road_m: f32,
    /// Metres of elevation.
    pub height: f32,
    /// 0 is flat, 1 is vertical.
    pub slope: f32,
}

/// One placed prop, in metres.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    /// World X.
    pub x: f32,
    /// World Y.
    pub y: f32,
    /// World Z.
    pub z: f32,
    /// Radians around Y.
    pub yaw: f32,
    /// Uniform scale.
    pub scale: f32,
}

const CHUNK_M: f32 = 128.0;

/// Instances for one layer in one chunk.
pub fn plan_chunk(
    theme_seed: u32,
    layer: &ScatterLayer,
    cx: i32,
    cz: i32,
    mut sample: impl FnMut(f32, f32) -> Ground,
) -> Vec<Placed> {
    if layer.density_per_ha <= 0.0 {
        return Vec::new();
    }
    let spacing = (10_000.0 / layer.density_per_ha).sqrt().clamp(1.2, 40.0);
    let origin_x = cx as f32 * CHUNK_M;
    let origin_z = cz as f32 * CHUNK_M;
    let steps = (CHUNK_M / spacing).floor() as i32;
    let mut placed = Vec::new();
    for iz in 0..steps.max(1) {
        for ix in 0..steps.max(1) {
            let jitter = hash32(mix_seed(theme_seed, layer.seed, cx, cz, ix, iz));
            let x = origin_x + (ix as f32 + jitter.0) * spacing;
            let z = origin_z + (iz as f32 + jitter.1) * spacing;
            if x < origin_x || z < origin_z || x >= origin_x + CHUNK_M || z >= origin_z + CHUNK_M {
                continue;
            }
            let ground = sample(x, z);
            if ground.road_m < layer.min_road_m || ground.road_m > layer.max_road_m {
                continue;
            }
            if ground.height < layer.altitude[0] || ground.height > layer.altitude[1] {
                continue;
            }
            if ground.slope > layer.max_slope {
                continue;
            }
            let cluster_n = detail_cluster(x, z, layer.cluster_frequency);
            if cluster_n < layer.cluster {
                continue;
            }
            let scale = layer.scale[0] + (layer.scale[1] - layer.scale[0]) * jitter.0;
            let yaw = if layer.random_yaw {
                jitter.1 * std::f32::consts::TAU
            } else {
                0.0
            };
            placed.push(Placed {
                x,
                y: ground.height,
                z,
                yaw,
                scale,
            });
        }
    }
    placed
}

/// Which library mesh an instance uses. Stable for a position.
pub fn variant_index(x: f32, z: f32, count: usize) -> usize {
    let bits = x.to_bits() ^ z.to_bits().rotate_left(7);
    (bits as usize) % count.max(1)
}

/// Order-independent mix of one instance. The smoke checksum adds these up.
pub fn instance_hash(kind: &str, x: f32, y: f32, z: f32) -> i64 {
    let x = (x * 100.0) as i64;
    let y = (y * 100.0) as i64;
    let z = (z * 100.0) as i64;
    let kind = kind.bytes().fold(0_i64, |acc, byte| {
        acc.wrapping_mul(131).wrapping_add(i64::from(byte))
    });
    kind.wrapping_mul(1_000_003)
        .wrapping_add(x.wrapping_mul(1_000_033))
        .wrapping_add(y.wrapping_mul(1_000_037))
        .wrapping_add(z)
}

fn detail_cluster(x: f32, z: f32, frequency: f32) -> f32 {
    let scale = frequency.max(0.001);
    let ix = (x * scale).floor();
    let iz = (z * scale).floor();
    let n = ix.to_bits().wrapping_mul(1_597_334_677) ^ iz.to_bits().wrapping_mul(3_812_015_801);
    (n >> 8) as f32 / 16_777_216.0
}

fn hash32(seed: u32) -> (f32, f32) {
    let a = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let b = a.wrapping_mul(2_246_822_519).wrapping_add(3_266_489_917);
    (
        (a >> 8) as f32 / 16_777_216.0,
        (b >> 8) as f32 / 16_777_216.0,
    )
}

fn mix_seed(theme: u32, layer: u32, cx: i32, cz: i32, ix: i32, iz: i32) -> u32 {
    theme
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(layer)
        .wrapping_mul(0x85EB_CA77)
        .wrapping_add(cx as u32)
        .wrapping_mul(0xC2B2_AE3D)
        .wrapping_add(cz as u32)
        .wrapping_add((ix as u32) << 16 | iz as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn open_ground(_: f32, _: f32) -> Ground {
        Ground {
            road_m: 20.0,
            height: 40.0,
            slope: 0.1,
        }
    }

    #[test]
    fn the_same_chunk_places_the_same_props_and_keeps_off_the_road() {
        let theme = Theme::builtin();
        let layer = &theme.scatter[0];
        let once = plan_chunk(theme.seed, layer, 1, -2, open_ground);
        let twice = plan_chunk(theme.seed, layer, 1, -2, open_ground);
        assert_eq!(once.len(), twice.len());
        assert!(
            once.iter()
                .all(|prop| prop.x.is_finite() && prop.scale > 0.0)
        );
        let blocked = plan_chunk(theme.seed, layer, 1, -2, |_, _| Ground {
            road_m: 1.0,
            height: 40.0,
            slope: 0.1,
        });
        assert!(blocked.is_empty());
    }

    #[test]
    fn variant_and_hash_are_stable() {
        assert_eq!(variant_index(1.5, 2.5, 3), variant_index(1.5, 2.5, 3));
        assert!(variant_index(1.5, 2.5, 3) < 3);
        assert_eq!(variant_index(1.0, 2.0, 0), 0);
        assert_eq!(
            instance_hash("tree", 1.0, 2.0, 3.0),
            instance_hash("tree", 1.0, 2.0, 3.0)
        );
        assert_ne!(
            instance_hash("tree", 1.0, 2.0, 3.0),
            instance_hash("rock", 1.0, 2.0, 3.0)
        );
    }
}
