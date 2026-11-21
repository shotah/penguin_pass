//! Square chunk grid on the X/Z ground plane.

use crate::WorldError;

/// Column and row of a chunk on the ground plane.
///
/// `x` increases east and `z` increases north, in chunk-sized steps. Ordered
/// by `x`, then `z`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ChunkCoord {
    /// East-west index. Chunk `0` starts at world x = 0.
    pub x: i32,
    /// North-south index. Chunk `0` starts at world z = 0.
    pub z: i32,
}

impl ChunkCoord {
    /// Chunk at column `x` and row `z`.
    #[must_use]
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// Closed-open square occupied by one chunk, in meters.
///
/// [`ChunkGrid::coord_at`] assigns a point to the chunk whose bounds contain
/// it in the half-open square `[min, max)`. Intersection queries treat the
/// same square as closed, so a shape that only touches a shared edge hits
/// both chunks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkBounds {
    /// Minimum world x, in meters. Inclusive.
    pub min_x_m: f64,
    /// Minimum world z, in meters. Inclusive.
    pub min_z_m: f64,
    /// Maximum world x, in meters. Exclusive for [`ChunkGrid::coord_at`].
    pub max_x_m: f64,
    /// Maximum world z, in meters. Exclusive for [`ChunkGrid::coord_at`].
    pub max_z_m: f64,
}

/// Square tiling of the X/Z plane.
///
/// The edge length is in meters. World x and z map to a [`ChunkCoord`] by
/// floor division, which is well-defined for negative coordinates: the chunk
/// contains the half-open square `[index * edge, (index + 1) * edge)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkGrid {
    edge_m: f64,
}

impl Default for ChunkGrid {
    /// A 128 m grid.
    fn default() -> Self {
        Self {
            edge_m: Self::DEFAULT_EDGE_M,
        }
    }
}

impl ChunkGrid {
    /// Default chunk edge, in meters.
    pub const DEFAULT_EDGE_M: f64 = 128.0;

    /// Grid with `edge_m` meters on a side.
    ///
    /// Returns [`WorldError::ChunkEdge`] when `edge_m` is not finite or is
    /// not greater than zero.
    pub fn try_new(edge_m: f64) -> Result<Self, WorldError> {
        if edge_m.is_finite() && edge_m > 0.0 {
            Ok(Self { edge_m })
        } else {
            Err(WorldError::ChunkEdge)
        }
    }

    /// Chunk edge length, in meters.
    #[must_use]
    pub const fn edge_m(&self) -> f64 {
        self.edge_m
    }

    /// Chunk that owns the world point `(x_m, z_m)`.
    ///
    /// Uses floor division. A point exactly on the maximum edge of a chunk
    /// belongs to the next chunk. Non-finite inputs map to index `0` on that
    /// axis. Indices saturate at [`i32::MIN`] and [`i32::MAX`].
    ///
    /// With the default 128 m edge the full `i32` index range covers world
    /// coordinates of about ±2.749×10^11 m. `f64` can represent every meter
    /// in that range exactly. `f32` cannot: its spacing is 1 m at about
    /// ±8.39×10^6 m and reaches the chunk edge at about ±1.07×10^9 m. A
    /// 200 km ride is safe in `f32` (spacing about 0.03 m).
    #[must_use]
    pub fn coord_at(&self, x_m: f64, z_m: f64) -> ChunkCoord {
        ChunkCoord {
            x: self.coord_component(x_m),
            z: self.coord_component(z_m),
        }
    }

    /// Minimum corner of `coord`, in meters: `[x, z]`.
    #[must_use]
    pub fn origin_m(&self, coord: ChunkCoord) -> [f64; 2] {
        [
            f64::from(coord.x) * self.edge_m,
            f64::from(coord.z) * self.edge_m,
        ]
    }

    /// Center of `coord`, in meters: `[x, z]`.
    #[must_use]
    pub fn center_m(&self, coord: ChunkCoord) -> [f64; 2] {
        let [x, z] = self.origin_m(coord);
        [x + self.edge_m * 0.5, z + self.edge_m * 0.5]
    }

    /// Bounds of `coord`, in meters.
    #[must_use]
    pub fn bounds(&self, coord: ChunkCoord) -> ChunkBounds {
        let [min_x_m, min_z_m] = self.origin_m(coord);
        ChunkBounds {
            min_x_m,
            min_z_m,
            max_x_m: min_x_m + self.edge_m,
            max_z_m: min_z_m + self.edge_m,
        }
    }

    /// Chunks whose closed square intersects the circle.
    ///
    /// `radius_m` is meters. A negative or non-finite radius, or a
    /// non-finite center, yields an empty list. The list is sorted by
    /// [`ChunkCoord`] and contains each chunk once.
    #[must_use]
    pub fn chunks_in_circle(&self, x_m: f64, z_m: f64, radius_m: f64) -> Vec<ChunkCoord> {
        let mut out = Vec::new();
        self.visit_circle(x_m, z_m, radius_m, |coord| out.push(coord));
        out
    }

    /// Chunks whose closed square intersects a capsule.
    ///
    /// The capsule is the line segment from `(ax_m, az_m)` to `(bx_m, bz_m)`
    /// expanded by `half_width_m` meters. A negative or non-finite half-width,
    /// or a non-finite endpoint, yields an empty list. The list is sorted by
    /// [`ChunkCoord`] and contains each chunk once.
    #[must_use]
    pub fn chunks_in_capsule(
        &self,
        ax_m: f64,
        az_m: f64,
        bx_m: f64,
        bz_m: f64,
        half_width_m: f64,
    ) -> Vec<ChunkCoord> {
        let mut out = Vec::new();
        self.visit_capsule(ax_m, az_m, bx_m, bz_m, half_width_m, |coord| {
            out.push(coord);
        });
        out
    }

    pub(crate) fn visit_circle(
        &self,
        x_m: f64,
        z_m: f64,
        radius_m: f64,
        visit: impl FnMut(ChunkCoord),
    ) {
        if !x_m.is_finite() || !z_m.is_finite() || !radius_m.is_finite() || radius_m < 0.0 {
            return;
        }
        let min_x = x_m - radius_m;
        let max_x = x_m + radius_m;
        let min_z = z_m - radius_m;
        let max_z = z_m + radius_m;
        self.visit_rect(
            min_x,
            min_z,
            max_x,
            max_z,
            |bounds| point_aabb_distance_sq(x_m, z_m, bounds) <= radius_m * radius_m,
            visit,
        );
    }

    pub(crate) fn visit_capsule(
        &self,
        ax_m: f64,
        az_m: f64,
        bx_m: f64,
        bz_m: f64,
        half_width_m: f64,
        visit: impl FnMut(ChunkCoord),
    ) {
        if !half_width_m.is_finite()
            || half_width_m < 0.0
            || ![ax_m, az_m, bx_m, bz_m]
                .iter()
                .all(|value| value.is_finite())
        {
            return;
        }
        let min_x = ax_m.min(bx_m) - half_width_m;
        let max_x = ax_m.max(bx_m) + half_width_m;
        let min_z = az_m.min(bz_m) - half_width_m;
        let max_z = az_m.max(bz_m) + half_width_m;
        let limit = half_width_m * half_width_m;
        self.visit_rect(
            min_x,
            min_z,
            max_x,
            max_z,
            |bounds| segment_aabb_distance_sq(ax_m, az_m, bx_m, bz_m, bounds) <= limit,
            visit,
        );
    }

    fn visit_rect(
        &self,
        min_x: f64,
        min_z: f64,
        max_x: f64,
        max_z: f64,
        mut hit: impl FnMut(ChunkBounds) -> bool,
        mut visit: impl FnMut(ChunkCoord),
    ) {
        if ![min_x, min_z, max_x, max_z]
            .iter()
            .all(|value| value.is_finite())
        {
            return;
        }
        // Floor division assigns a point on a grid line to the higher chunk.
        // The previous chunk's closed max edge still contains that line, so
        // the scan starts one chunk early and the predicate drops misses.
        let x0 = self.coord_component(min_x).saturating_sub(1);
        let x1 = self.coord_component(max_x);
        let z0 = self.coord_component(min_z).saturating_sub(1);
        let z1 = self.coord_component(max_z);
        for x in x0..=x1 {
            for z in z0..=z1 {
                let coord = ChunkCoord::new(x, z);
                if hit(self.bounds(coord)) {
                    visit(coord);
                }
            }
        }
    }

    fn coord_component(&self, value: f64) -> i32 {
        if !value.is_finite() {
            return 0;
        }
        let quotient = (value / self.edge_m).floor();
        if quotient >= f64::from(i32::MAX) {
            i32::MAX
        } else if quotient <= f64::from(i32::MIN) {
            i32::MIN
        } else {
            quotient as i32
        }
    }
}

fn point_aabb_distance_sq(x: f64, z: f64, bounds: ChunkBounds) -> f64 {
    let dx = axis_distance(x, bounds.min_x_m, bounds.max_x_m);
    let dz = axis_distance(z, bounds.min_z_m, bounds.max_z_m);
    dx * dx + dz * dz
}

fn axis_distance(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min - value
    } else if value > max {
        value - max
    } else {
        0.0
    }
}

fn segment_aabb_distance_sq(ax: f64, az: f64, bx: f64, bz: f64, bounds: ChunkBounds) -> f64 {
    if segment_intersects_aabb(ax, az, bx, bz, bounds) {
        return 0.0;
    }
    let mut best =
        point_aabb_distance_sq(ax, az, bounds).min(point_aabb_distance_sq(bx, bz, bounds));
    let corners = [
        (bounds.min_x_m, bounds.min_z_m),
        (bounds.min_x_m, bounds.max_z_m),
        (bounds.max_x_m, bounds.min_z_m),
        (bounds.max_x_m, bounds.max_z_m),
    ];
    for (cx, cz) in corners {
        best = best.min(point_segment_distance_sq(cx, cz, ax, az, bx, bz));
    }
    best
}

/// Liang–Barsky clip of the segment against the closed rectangle.
fn segment_intersects_aabb(ax: f64, az: f64, bx: f64, bz: f64, bounds: ChunkBounds) -> bool {
    let mut t0: f64 = 0.0;
    let mut t1: f64 = 1.0;
    let dx = bx - ax;
    let dz = bz - az;
    let mins = [bounds.min_x_m, bounds.min_z_m];
    let maxs = [bounds.max_x_m, bounds.max_z_m];
    let origin = [ax, az];
    let delta = [dx, dz];
    for axis in 0..2 {
        if delta[axis] == 0.0 {
            if origin[axis] < mins[axis] || origin[axis] > maxs[axis] {
                return false;
            }
            continue;
        }
        let inv = 1.0 / delta[axis];
        let mut near = (mins[axis] - origin[axis]) * inv;
        let mut far = (maxs[axis] - origin[axis]) * inv;
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 > t1 {
            return false;
        }
    }
    true
}

fn point_segment_distance_sq(px: f64, pz: f64, ax: f64, az: f64, bx: f64, bz: f64) -> f64 {
    let dx = bx - ax;
    let dz = bz - az;
    let len_sq = dx * dx + dz * dz;
    if len_sq == 0.0 {
        let ox = px - ax;
        let oz = pz - az;
        return ox * ox + oz * oz;
    }
    let t = (((px - ax) * dx + (pz - az) * dz) / len_sq).clamp(0.0, 1.0);
    let cx = ax + dx * t;
    let cz = az + dz * t;
    let ox = px - cx;
    let oz = pz - cz;
    ox * ox + oz * oz
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn set_of(coords: Vec<ChunkCoord>) -> BTreeSet<ChunkCoord> {
        coords.into_iter().collect()
    }

    #[test]
    fn coord_at_uses_floor_including_negatives_and_exact_edges() {
        let grid = ChunkGrid::default();
        assert_eq!(grid.edge_m(), 128.0);
        assert_eq!(grid.coord_at(0.0, 0.0), ChunkCoord::new(0, 0));
        assert_eq!(grid.coord_at(127.999, 0.0), ChunkCoord::new(0, 0));
        assert_eq!(grid.coord_at(128.0, -0.0), ChunkCoord::new(1, 0));
        assert_eq!(grid.coord_at(-0.001, 0.0), ChunkCoord::new(-1, 0));
        assert_eq!(grid.coord_at(-128.0, 10.0), ChunkCoord::new(-1, 0));
        assert_eq!(grid.coord_at(-128.5, -128.0), ChunkCoord::new(-2, -1));
        assert_eq!(grid.coord_at(f64::NAN, 64.0), ChunkCoord::new(0, 0));
        assert_eq!(grid.coord_at(10.0, f64::INFINITY), ChunkCoord::new(0, 0));

        let tens = ChunkGrid::try_new(10.0).unwrap();
        assert_eq!(tens.coord_at(-25.0, 19.999), ChunkCoord::new(-3, 1));
        assert_eq!(tens.coord_at(-20.0, 20.0), ChunkCoord::new(-2, 2));
    }

    #[test]
    fn bounds_origin_and_center_follow_the_edge() {
        let grid = ChunkGrid::default();
        let coord = ChunkCoord::new(-1, 2);
        assert_eq!(grid.origin_m(coord), [-128.0, 256.0]);
        assert_eq!(grid.center_m(coord), [-64.0, 320.0]);
        let bounds = grid.bounds(coord);
        assert_eq!(bounds.min_x_m, -128.0);
        assert_eq!(bounds.max_x_m, 0.0);
        assert_eq!(bounds.min_z_m, 256.0);
        assert_eq!(bounds.max_z_m, 384.0);
        assert_eq!(grid.coord_at(bounds.min_x_m, bounds.min_z_m), coord);
        assert_eq!(
            grid.coord_at(bounds.max_x_m, bounds.max_z_m),
            ChunkCoord::new(0, 3)
        );
    }

    #[test]
    fn zero_radius_circle_on_a_corner_includes_every_closed_neighbor() {
        let grid = ChunkGrid::default();
        assert_eq!(
            set_of(grid.chunks_in_circle(64.0, 64.0, 0.0)),
            BTreeSet::from([ChunkCoord::new(0, 0)])
        );
        assert_eq!(
            set_of(grid.chunks_in_circle(128.0, 64.0, 0.0)),
            BTreeSet::from([ChunkCoord::new(0, 0), ChunkCoord::new(1, 0)])
        );
        assert_eq!(
            set_of(grid.chunks_in_circle(128.0, 128.0, 0.0)),
            BTreeSet::from([
                ChunkCoord::new(0, 0),
                ChunkCoord::new(0, 1),
                ChunkCoord::new(1, 0),
                ChunkCoord::new(1, 1),
            ])
        );
        assert_eq!(
            set_of(grid.chunks_in_circle(-128.0, -128.0, 0.0)),
            BTreeSet::from([
                ChunkCoord::new(-2, -2),
                ChunkCoord::new(-2, -1),
                ChunkCoord::new(-1, -2),
                ChunkCoord::new(-1, -1),
            ])
        );
    }

    #[test]
    fn circle_matches_brute_force() {
        let edges = [128.0, 50.0, 100.0];
        let centers = [
            (0.0, 0.0, 0.0),
            (128.0, -64.0, 0.0),
            (-300.0, -50.0, 80.0),
            (40.0, 40.0, 200.0),
            (-15.5, 90.0, 1.0),
            (500.0, -500.0, 140.0),
        ];
        let mut state = 0x1234_5678_u64;
        let mut random_centers = Vec::new();
        for _ in 0..24 {
            random_centers.push((
                unit(&mut state) * 500.0 - 250.0,
                unit(&mut state) * 500.0 - 250.0,
                unit(&mut state) * 180.0,
            ));
        }
        for edge in edges {
            let grid = ChunkGrid::try_new(edge).unwrap();
            for &(x, z, radius) in centers.iter().chain(random_centers.iter()) {
                let got = set_of(grid.chunks_in_circle(x, z, radius));
                let mut expected = BTreeSet::new();
                for cx in -16..=16 {
                    for cz in -16..=16 {
                        let coord = ChunkCoord::new(cx, cz);
                        let bounds = grid.bounds(coord);
                        let dx = axis_distance(x, bounds.min_x_m, bounds.max_x_m);
                        let dz = axis_distance(z, bounds.min_z_m, bounds.max_z_m);
                        if dx * dx + dz * dz <= radius * radius {
                            expected.insert(coord);
                        }
                    }
                }
                assert_eq!(got, expected, "edge {edge} center ({x}, {z}) r {radius}");
            }
        }
    }

    #[test]
    fn capsule_matches_hand_cases_and_brute_force() {
        let grid = ChunkGrid::default();
        assert_eq!(
            set_of(grid.chunks_in_capsule(10.0, 10.0, 50.0, 40.0, 5.0)),
            BTreeSet::from([ChunkCoord::new(0, 0)])
        );
        assert_eq!(
            set_of(grid.chunks_in_capsule(0.0, 64.0, 500.0, 64.0, 10.0)),
            BTreeSet::from([
                ChunkCoord::new(-1, 0),
                ChunkCoord::new(0, 0),
                ChunkCoord::new(1, 0),
                ChunkCoord::new(2, 0),
                ChunkCoord::new(3, 0),
            ])
        );
        assert_eq!(
            set_of(grid.chunks_in_capsule(0.0, 0.0, 100.0, 0.0, 0.0)),
            BTreeSet::from([
                ChunkCoord::new(-1, -1),
                ChunkCoord::new(-1, 0),
                ChunkCoord::new(0, -1),
                ChunkCoord::new(0, 0),
            ])
        );
        assert_eq!(
            set_of(grid.chunks_in_capsule(-500.0, -300.0, -200.0, -300.0, 10.0)),
            BTreeSet::from([
                ChunkCoord::new(-4, -3),
                ChunkCoord::new(-3, -3),
                ChunkCoord::new(-2, -3),
            ])
        );

        let mut state = 0xABCDEF_u64;
        for _ in 0..30 {
            let ax = unit(&mut state) * 600.0 - 300.0;
            let az = unit(&mut state) * 600.0 - 300.0;
            let bx = ax + unit(&mut state) * 400.0 - 200.0;
            let bz = az + unit(&mut state) * 400.0 - 200.0;
            let half = unit(&mut state) * 80.0;
            let got = set_of(grid.chunks_in_capsule(ax, az, bx, bz, half));
            for x in -8..=8 {
                for z in -8..=8 {
                    let coord = ChunkCoord::new(x, z);
                    let dist =
                        reference_capsule_distance(ax, az, bx, bz, grid.bounds(coord)).sqrt();
                    let included = got.contains(&coord);
                    if dist + 0.2 < half {
                        assert!(included, "{coord:?} dist {dist} half {half}");
                    } else if dist - 0.2 > half {
                        assert!(!included, "{coord:?} dist {dist} half {half}");
                    }
                }
            }
        }
    }

    #[test]
    fn queries_reject_non_finite_and_negative_radius() {
        let grid = ChunkGrid::default();
        assert!(grid.chunks_in_circle(0.0, 0.0, -1.0).is_empty());
        assert!(grid.chunks_in_circle(f64::NAN, 0.0, 10.0).is_empty());
        assert!(grid.chunks_in_circle(0.0, 0.0, f64::INFINITY).is_empty());
        assert!(grid.chunks_in_capsule(0.0, 0.0, 10.0, 0.0, -0.1).is_empty());
        assert!(
            grid.chunks_in_capsule(f64::NAN, 0.0, 1.0, 1.0, 1.0)
                .is_empty()
        );
    }

    #[test]
    fn try_new_rejects_non_positive_edge() {
        assert!(ChunkGrid::try_new(0.0).is_err());
        assert!(ChunkGrid::try_new(-10.0).is_err());
        assert!(ChunkGrid::try_new(f64::NAN).is_err());
        assert!(ChunkGrid::try_new(f64::INFINITY).is_err());
        assert!(ChunkGrid::try_new(1.0).is_ok());
    }

    #[test]
    fn coord_at_saturates_at_i32_without_panic() {
        let grid = ChunkGrid::default();
        let edge = grid.edge_m();
        let max_world = f64::from(i32::MAX) * edge;
        assert_eq!(grid.coord_at(max_world, 0.0).x, i32::MAX);
        assert_eq!(grid.coord_at(max_world + edge, -edge).x, i32::MAX);
        let min_world = f64::from(i32::MIN) * edge;
        assert_eq!(grid.coord_at(min_world, 0.0).x, i32::MIN);
        assert_eq!(grid.coord_at(min_world - edge, 0.0).x, i32::MIN);
    }

    fn unit(state: &mut u64) -> f64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        (*state >> 33) as f64 / f64::from(1_u32 << 31)
    }

    fn reference_capsule_distance(ax: f64, az: f64, bx: f64, bz: f64, bounds: ChunkBounds) -> f64 {
        let mut best = point_aabb_distance_sq(ax, az, bounds);
        let steps = 64;
        for step in 0..=steps {
            let t = f64::from(step) / f64::from(steps);
            let x = ax + (bx - ax) * t;
            let z = az + (bz - az) * t;
            best = best.min(point_aabb_distance_sq(x, z, bounds));
        }
        let corners = [
            (bounds.min_x_m, bounds.min_z_m),
            (bounds.min_x_m, bounds.max_z_m),
            (bounds.max_x_m, bounds.min_z_m),
            (bounds.max_x_m, bounds.max_z_m),
        ];
        for (cx, cz) in corners {
            best = best.min(point_segment_distance_sq(cx, cz, ax, az, bx, bz));
        }
        best
    }
}
