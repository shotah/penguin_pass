//! Mesh checks shared by the unit tests.

use std::collections::BTreeMap;

use glam::Vec3;

use crate::mesh::{Part, Surface};

/// A connected piece of a part, after welding equal positions.
pub(crate) struct Shell {
    /// Every edge is used by exactly two triangles, in opposite directions.
    pub closed: bool,
    /// Signed volume, in cubic meters. Positive when faces point outward.
    pub volume_m3: f32,
    /// Number of triangles in the shell.
    pub triangles: usize,
    /// A point on the shell, to find it when a test fails.
    pub sample: Vec3,
}

fn triangle(surface: &Surface, t: usize) -> [Vec3; 3] {
    let at = |c: usize| Vec3::from_array(surface.positions[surface.indices[t * 3 + c] as usize]);
    [at(0), at(1), at(2)]
}

/// Panic unless every surface of `part` is a well-formed mesh.
pub(crate) fn assert_part_valid(part: &Part) {
    let name = part.name();
    for (s, surface) in part.surfaces().iter().enumerate() {
        assert_eq!(
            surface.positions.len(),
            surface.normals.len(),
            "{name}[{s}]: one normal per position"
        );
        assert_eq!(surface.indices.len() % 3, 0, "{name}[{s}]: whole triangles");
        assert!(!surface.indices.is_empty(), "{name}[{s}]: empty surface");
        for p in &surface.positions {
            assert!(p.iter().all(|c| c.is_finite()), "{name}[{s}]: bad position");
        }
        for n in &surface.normals {
            assert!(n.iter().all(|c| c.is_finite()), "{name}[{s}]: bad normal");
            let length = Vec3::from_array(*n).length();
            assert!(
                (length - 1.0).abs() < 1.0e-4,
                "{name}[{s}]: normal length {length}"
            );
        }
        for &i in &surface.indices {
            assert!(
                (i as usize) < surface.positions.len(),
                "{name}[{s}]: index {i} out of range"
            );
        }
        let m = surface.material;
        assert!(m.base_color.iter().all(|c| (0.0..=1.0).contains(c)));
        assert!((0.0..=1.0).contains(&m.roughness));
        assert!((0.0..=1.0).contains(&m.metallic));

        for t in 0..surface.triangle_count() {
            let [a, b, c] = triangle(surface, t);
            let cross = (b - a).cross(c - a);
            let area = 0.5 * cross.length();
            assert!(area > 1.0e-11, "{name}[{s}]: degenerate triangle at {a:?}");
            let face = cross.normalize();
            let mut mean = Vec3::ZERO;
            for corner in 0..3 {
                let n = Vec3::from_array(surface.normals[surface.indices[t * 3 + corner] as usize]);
                assert!(
                    n.dot(face) > 0.0,
                    "{name}[{s}]: normal {n:?} opposes its face {face:?} at {a:?}"
                );
                mean += n;
            }
            assert!(
                mean.normalize().dot(face) > 0.3,
                "{name}[{s}]: normals lean away from the face at {a:?}"
            );
        }
    }
}

/// Split a part into connected shells and measure each one.
pub(crate) fn shells(part: &Part) -> Vec<Shell> {
    // Weld by exact position. Split normals and split materials duplicate a
    // vertex bit for bit, so exact matching rejoins them.
    let mut ids: BTreeMap<[u32; 3], usize> = BTreeMap::new();
    let mut points: Vec<Vec3> = Vec::new();
    let mut tris: Vec<[usize; 3]> = Vec::new();
    for surface in part.surfaces() {
        for t in 0..surface.triangle_count() {
            let corners = triangle(surface, t).map(|p| {
                // Fold -0.0 into 0.0 so the two weld together.
                let key = (p + Vec3::ZERO).to_array().map(f32::to_bits);
                *ids.entry(key).or_insert_with(|| {
                    points.push(p);
                    points.len() - 1
                })
            });
            tris.push(corners);
        }
    }

    // Union-find over welded vertices.
    let mut parent: Vec<usize> = (0..points.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for t in &tris {
        let a = find(&mut parent, t[0]);
        for &v in &t[1..] {
            let b = find(&mut parent, v);
            parent[b] = a;
        }
    }

    struct Acc {
        volume: f32,
        triangles: usize,
        sample: Vec3,
        edges: BTreeMap<(usize, usize), i32>,
    }
    let mut by_root: BTreeMap<usize, Acc> = BTreeMap::new();
    for t in &tris {
        let root = find(&mut parent, t[0]);
        let acc = by_root.entry(root).or_insert_with(|| Acc {
            volume: 0.0,
            triangles: 0,
            sample: points[t[0]],
            edges: BTreeMap::new(),
        });
        let [a, b, c] = t.map(|i| points[i]);
        acc.volume += a.dot(b.cross(c)) / 6.0;
        acc.triangles += 1;
        for (from, to) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            // +1 for the forward direction, -1 for the reverse: a closed,
            // consistently wound shell cancels to zero on every edge.
            let (key, sign) = if from < to {
                ((from, to), 1)
            } else {
                ((to, from), -1)
            };
            *acc.edges.entry(key).or_insert(0) += sign;
        }
    }
    by_root
        .into_values()
        .map(|acc| Shell {
            closed: acc.edges.values().all(|&count| count == 0),
            volume_m3: acc.volume,
            triangles: acc.triangles,
            sample: acc.sample,
        })
        .collect()
}
