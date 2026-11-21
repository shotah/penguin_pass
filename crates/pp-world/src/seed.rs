//! Stable per-chunk seeds.

use crate::ChunkCoord;

/// Golden-ratio increment used by SplitMix64.
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
/// First SplitMix64 finalizer constant (Stafford / Vigna).
const MIX_1: u64 = 0xBF58_476D_1CE4_E5B9;
/// Second SplitMix64 finalizer constant (Stafford / Vigna).
const MIX_2: u64 = 0x94D0_49BB_1331_11EB;
/// Odd wyhash constant mixed into the chunk x index.
const M_X: u64 = 0xD1B5_4A32_D192_ED03;
/// Odd wyhash constant mixed into the chunk z index.
const M_Z: u64 = 0xA076_1D64_78BD_642F;
/// Odd wyhash constant mixed into the layer.
const M_L: u64 = 0xE703_7ED1_A0B4_28DB;

/// SplitMix64 finalizer. Stafford's variant of Vigna's mixer.
///
/// This is a fixed public function. Every step is wrapping arithmetic on
/// `u64`, so the result does not depend on the platform.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(MIX_1);
    z = (z ^ (z >> 27)).wrapping_mul(MIX_2);
    z ^ (z >> 31)
}

/// Deterministic seed for one chunk layer.
///
/// `world_seed` selects the world. `coord` selects the chunk. `layer`
/// selects an independent stream inside that chunk (for example `0` for the
/// terrain heightfield and `1` for scenery). The result is a `u64` suitable
/// as the seed of a generator owned by the caller.
///
/// # Stability
///
/// This function is a compatibility promise. Saved games and shared seeds
/// depend on the exact bits. The mix will not change in a future version of
/// this crate; a different mix would be a different function.
///
/// The steps below are the whole definition. Arithmetic is wrapping mod 2^64.
/// `x` and `z` are sign-extended from `i32` to `u64` (the `i32` bit pattern,
/// not the absolute value). The finalizer is the SplitMix64 mixer published
/// by Vigna (2015) with Stafford's constants. The odd multipliers are the
/// wyhash constants, used so that neighbouring coordinates and layers enter
/// the mixer as unrelated values.
///
/// ```text
/// h = world_seed
/// h = mix(h + GOLDEN xor sext(x) * M_X)
/// h = mix(h + GOLDEN xor sext(z) * M_Z)
/// h = mix(h + GOLDEN xor layer * M_L)
/// ```
///
/// `GOLDEN` is `0x9E3779B97F4A7C15`. `M_X`, `M_Z`, and `M_L` are
/// `0xD1B54A32D192ED03`, `0xA0761D6478BD642F`, and `0xE7037ED1A0B428DB`.
#[must_use]
pub fn chunk_seed(world_seed: u64, coord: ChunkCoord, layer: u32) -> u64 {
    let x = i64::from(coord.x) as u64;
    let z = i64::from(coord.z) as u64;
    let mut h = world_seed;
    h = mix(h.wrapping_add(GOLDEN) ^ x.wrapping_mul(M_X));
    h = mix(h.wrapping_add(GOLDEN) ^ z.wrapping_mul(M_Z));
    mix(h.wrapping_add(GOLDEN) ^ u64::from(layer).wrapping_mul(M_L))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hamming(left: u64, right: u64) -> u32 {
        (left ^ right).count_ones()
    }

    #[test]
    fn chunk_seed_matches_hard_coded_vectors() {
        // Locked bit patterns. A change here is a broken compatibility promise.
        let cases = [
            (0, 0, 0, 0, 0x2382_75BC_38FC_BE91),
            (1, 0, 0, 0, 0xB18A_02F4_6D8D_86C3),
            (1, 1, 0, 0, 0x4B9E_F2DC_EA6D_95CD),
            (1, 0, 1, 0, 0x73E0_56DD_A999_5ED0),
            (1, 0, 0, 1, 0xD24D_7834_ED6B_E890),
            (1, -1, -2, 3, 0xC3A4_5C36_70E1_73E2),
            (
                u64::MAX,
                i32::MIN,
                i32::MAX,
                u32::MAX,
                0xB77C_0A87_CA9D_432D,
            ),
            (0x00C0_FFEE, 4, -7, 2, 0x947B_20CF_F1D9_84D3),
        ];
        for (world, x, z, layer, expected) in cases {
            let got = chunk_seed(world, ChunkCoord::new(x, z), layer);
            assert_eq!(got, expected, "seed({world}, {x}, {z}, {layer})");
        }
    }

    #[test]
    fn chunk_seed_neighbors_and_layers_are_decorrelated() {
        let origin = ChunkCoord::new(0, 0);
        let base = chunk_seed(1, origin, 0);
        let neighbors = [
            ChunkCoord::new(1, 0),
            ChunkCoord::new(0, 1),
            ChunkCoord::new(-1, 0),
            ChunkCoord::new(0, -1),
            ChunkCoord::new(1, 1),
            ChunkCoord::new(2, 3),
            ChunkCoord::new(-8, 5),
        ];
        for coord in neighbors {
            let seed = chunk_seed(1, coord, 0);
            assert_ne!(seed, base);
            let bits = hamming(seed, base);
            assert!(bits >= 20, "{coord:?} shares too many bits ({bits})");
        }
        for layer in [1, 2, 9] {
            let seed = chunk_seed(1, origin, layer);
            assert_ne!(seed, base);
            let bits = hamming(seed, base);
            assert!(bits >= 20, "layer {layer} shares too many bits ({bits})");
        }

        // A small neighbourhood at another world seed is unique.
        let mut seen = std::collections::BTreeSet::new();
        for x in -2..=2 {
            for z in -2..=2 {
                for layer in [0, 1] {
                    assert!(seen.insert(chunk_seed(42, ChunkCoord::new(x, z), layer)));
                }
            }
        }
    }
}
