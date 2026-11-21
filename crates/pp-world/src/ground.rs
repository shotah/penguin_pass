//! One far ground mesh plus the corridor chunks, without Godot.
//!
//! The far mesh is a 64 m grid around the course and a 256 m grid beyond it.
//! Cells that sit wholly inside a corridor chunk are left out. Where the two
//! grids meet, the in-between vertices lie on the coarse edge. Every open
//! edge grows a skirt so a gap cannot show sky.

use std::collections::{HashMap, HashSet};

use crate::heightfield::Field;

/// How far a skirt drops, in meters.
pub const SKIRT_M: f32 = 40.0;
/// Near far-mesh cell, in meters.
pub const NEAR_CELL_M: f32 = 64.0;
/// Far far-mesh cell, in meters.
pub const FAR_CELL_M: f32 = 256.0;
const NEAR_REACH_M: f32 = 1_200.0;
const FAR_REACH_M: f32 = 5_600.0;
const CHUNK_M: f32 = 128.0;

/// One corner of a ground triangle.
#[derive(Clone, Copy, Debug)]
pub struct GroundVertex {
    /// World x, meters.
    pub x: f32,
    /// World y, meters.
    pub y: f32,
    /// World z, meters.
    pub z: f32,
    /// `true` when `y` is [`Field::height`](crate::heightfield::Field::height) at this point.
    pub sampled: bool,
}

/// A skirt along one open edge. The hem is [`SKIRT_M`] below `y0` and `y1`.
#[derive(Clone, Copy, Debug)]
pub struct Skirt {
    /// First top corner.
    pub x0: f32,
    /// First top height.
    pub y0: f32,
    /// First top z.
    pub z0: f32,
    /// Second top corner.
    pub x1: f32,
    /// Second top height.
    pub y1: f32,
    /// Second top z.
    pub z1: f32,
}

/// Two polylines that claim the same border. Each is ordered along the edge.
#[derive(Clone, Debug)]
pub struct Border {
    /// One side, including its in-between vertices.
    pub left: Vec<[f32; 3]>,
    /// The other side.
    pub right: Vec<[f32; 3]>,
}

/// One corridor chunk grid.
#[derive(Clone, Debug)]
pub struct PlannedChunk {
    /// Chunk x index.
    pub cx: i32,
    /// Chunk z index.
    pub cz: i32,
    /// Cell size, meters.
    pub cell: f32,
    /// Cells on a side.
    pub steps: i32,
    /// Row-major heights, `(steps + 1)` squared.
    pub heights: Vec<f32>,
    /// Parallel to `heights`. `false` where the vertex was snapped to a coarse edge.
    pub sampled: Vec<bool>,
    /// Open edges of this chunk.
    pub skirts: Vec<Skirt>,
}

/// The single mesh outside the corridor chunks.
#[derive(Clone, Debug)]
pub struct PlannedFar {
    /// Top vertices.
    pub vertices: Vec<GroundVertex>,
    /// Clockwise triangles, indices into `vertices`.
    pub triangles: Vec<[u32; 3]>,
    /// Open edges, including the outer perimeter.
    pub skirts: Vec<Skirt>,
}

/// Everything the Godot uploader needs, and everything the invariant test checks.
#[derive(Clone, Debug)]
pub struct GroundPlan {
    /// Corridor chunks.
    pub chunks: Vec<PlannedChunk>,
    /// The one far mesh.
    pub far: PlannedFar,
    /// Shared borders, chunk to chunk and chunk to far and 64 m to 256 m.
    pub borders: Vec<Border>,
}

/// Build the corridor grids and the one far mesh for `field`.
#[must_use]
pub fn plan(field: &Field) -> GroundPlan {
    let occupied: HashSet<(i32, i32)> = field.corridor().iter().copied().collect();
    let mut chunks = Vec::with_capacity(occupied.len());
    for &(cx, cz) in field.corridor() {
        chunks.push(plan_chunk(field, cx, cz, &occupied));
    }
    let (span_min_x, span_max_x, span_min_z, span_max_z) = field.spans();
    let far = plan_far(
        field, span_min_x, span_max_x, span_min_z, span_max_z, &occupied,
    );
    let borders = collect_borders(&chunks, &far, &occupied);
    GroundPlan {
        chunks,
        far,
        borders,
    }
}

fn plan_chunk(field: &Field, cx: i32, cz: i32, occupied: &HashSet<(i32, i32)>) -> PlannedChunk {
    let cell = field.chunk_cell(cx, cz);
    let steps = (CHUNK_M / cell).round() as i32;
    let n = steps + 1;
    let origin_x = cx as f32 * CHUNK_M;
    let origin_z = cz as f32 * CHUNK_M;
    let mut heights = vec![0.0_f32; (n * n) as usize];
    let mut sampled = vec![true; (n * n) as usize];
    for iz in 0..n {
        for ix in 0..n {
            let x = origin_x + ix as f32 * cell;
            let z = origin_z + iz as f32 * cell;
            let open_west = ix == 0 && !occupied.contains(&(cx - 1, cz));
            let open_east = ix == n - 1 && !occupied.contains(&(cx + 1, cz));
            let open_south = iz == 0 && !occupied.contains(&(cx, cz - 1));
            let open_north = iz == n - 1 && !occupied.contains(&(cx, cz + 1));
            let (spacing, along_x) = if open_west || open_east || open_south || open_north {
                (Some(NEAR_CELL_M), open_south || open_north)
            } else if ix == 0 {
                let other = field.chunk_cell(cx - 1, cz);
                ((other > cell).then_some(other), false)
            } else if ix == n - 1 {
                let other = field.chunk_cell(cx + 1, cz);
                ((other > cell).then_some(other), false)
            } else if iz == 0 {
                let other = field.chunk_cell(cx, cz - 1);
                ((other > cell).then_some(other), true)
            } else if iz == n - 1 {
                let other = field.chunk_cell(cx, cz + 1);
                ((other > cell).then_some(other), true)
            } else {
                (None, false)
            };
            let index = (iz * n + ix) as usize;
            if let Some(spacing) = spacing {
                let (y, is_sample) =
                    seam_or_sample(x, z, along_x, spacing, |px, pz| field.height(px, pz));
                heights[index] = y;
                sampled[index] = is_sample;
            } else {
                heights[index] = field.height(x, z);
            }
        }
    }
    let mut skirts = Vec::new();
    if !occupied.contains(&(cx - 1, cz)) {
        skirts.extend(edge_skirts(
            &heights,
            steps,
            cell,
            origin_x,
            origin_z,
            Edge::West,
        ));
    }
    if !occupied.contains(&(cx + 1, cz)) {
        skirts.extend(edge_skirts(
            &heights,
            steps,
            cell,
            origin_x,
            origin_z,
            Edge::East,
        ));
    }
    if !occupied.contains(&(cx, cz - 1)) {
        skirts.extend(edge_skirts(
            &heights,
            steps,
            cell,
            origin_x,
            origin_z,
            Edge::South,
        ));
    }
    if !occupied.contains(&(cx, cz + 1)) {
        skirts.extend(edge_skirts(
            &heights,
            steps,
            cell,
            origin_x,
            origin_z,
            Edge::North,
        ));
    }
    PlannedChunk {
        cx,
        cz,
        cell,
        steps,
        heights,
        sampled,
        skirts,
    }
}

#[derive(Clone, Copy)]
enum Edge {
    West,
    East,
    South,
    North,
}

fn edge_skirts(
    heights: &[f32],
    steps: i32,
    cell: f32,
    origin_x: f32,
    origin_z: f32,
    edge: Edge,
) -> Vec<Skirt> {
    let n = steps + 1;
    let mut skirts = Vec::with_capacity(steps as usize);
    for step in 0..steps {
        let (ix0, iz0, ix1, iz1) = match edge {
            Edge::West => (0, step, 0, step + 1),
            Edge::East => (n - 1, step, n - 1, step + 1),
            Edge::South => (step, 0, step + 1, 0),
            Edge::North => (step, n - 1, step + 1, n - 1),
        };
        skirts.push(Skirt {
            x0: origin_x + ix0 as f32 * cell,
            y0: heights[(iz0 * n + ix0) as usize],
            z0: origin_z + iz0 as f32 * cell,
            x1: origin_x + ix1 as f32 * cell,
            y1: heights[(iz1 * n + ix1) as usize],
            z1: origin_z + iz1 as f32 * cell,
        });
    }
    skirts
}

fn seam_or_sample(
    x: f32,
    z: f32,
    along_x: bool,
    spacing: f32,
    height: impl Fn(f32, f32) -> f32,
) -> (f32, bool) {
    let t = if along_x { x } else { z };
    let a = (t / spacing).floor() * spacing;
    let u = ((t - a) / spacing).clamp(0.0, 1.0);
    if u < 1.0e-3 || u > 1.0 - 1.0e-3 {
        return (height(x, z), true);
    }
    let b = a + spacing;
    let (x0, z0, x1, z1) = if along_x { (a, z, b, z) } else { (x, a, x, b) };
    let h0 = height(x0, z0);
    let h1 = height(x1, z1);
    (h0 + (h1 - h0) * u, false)
}

struct FarBuild {
    vertices: Vec<GroundVertex>,
    triangles: Vec<[u32; 3]>,
    index: HashMap<(i32, i32), u32>,
    /// Boundary edges still waiting for a second quad: (a, b) -> opposite vertex.
    open: HashMap<(u32, u32), u32>,
}

impl FarBuild {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            triangles: Vec::new(),
            index: HashMap::new(),
            open: HashMap::new(),
        }
    }

    fn vertex(&mut self, x: f32, y: f32, z: f32, sampled: bool) -> u32 {
        let key = ((x * 50.0).round() as i32, (z * 50.0).round() as i32);
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let index = self.vertices.len() as u32;
        self.vertices.push(GroundVertex { x, y, z, sampled });
        self.index.insert(key, index);
        index
    }

    fn edge(&mut self, a: u32, b: u32, opposite: u32) {
        let key = if a < b { (a, b) } else { (b, a) };
        if self.open.remove(&key).is_none() {
            self.open.insert(key, opposite);
        }
    }

    fn quad(&mut self, corners: [(f32, f32, f32, bool); 4]) {
        let ids = [
            self.vertex(corners[0].0, corners[0].1, corners[0].2, corners[0].3),
            self.vertex(corners[1].0, corners[1].1, corners[1].2, corners[1].3),
            self.vertex(corners[2].0, corners[2].1, corners[2].2, corners[2].3),
            self.vertex(corners[3].0, corners[3].1, corners[3].2, corners[3].3),
        ];
        self.triangles.push([ids[0], ids[1], ids[3]]);
        self.triangles.push([ids[1], ids[2], ids[3]]);
        self.edge(ids[0], ids[1], ids[3]);
        self.edge(ids[1], ids[2], ids[0]);
        self.edge(ids[2], ids[3], ids[1]);
        self.edge(ids[3], ids[0], ids[2]);
    }
}

fn plan_far(
    field: &Field,
    span_min_x: f32,
    span_max_x: f32,
    span_min_z: f32,
    span_max_z: f32,
    occupied: &HashSet<(i32, i32)>,
) -> PlannedFar {
    let inner = bounds(
        span_min_x,
        span_max_x,
        span_min_z,
        span_max_z,
        NEAR_REACH_M,
        FAR_CELL_M,
    );
    let outer = bounds(
        span_min_x,
        span_max_x,
        span_min_z,
        span_max_z,
        FAR_REACH_M,
        FAR_CELL_M,
    );
    let mut build = FarBuild::new();
    let mut z = inner.min_z;
    while z < inner.max_z - 0.1 {
        let mut x = inner.min_x;
        while x < inner.max_x - 0.1 {
            if !cell_in_corridor(x, z, NEAR_CELL_M, occupied) {
                build.quad(cell_corners(field, x, z, NEAR_CELL_M, &inner));
            }
            x += NEAR_CELL_M;
        }
        z += NEAR_CELL_M;
    }
    z = outer.min_z;
    while z < outer.max_z - 0.1 {
        let mut x = outer.min_x;
        while x < outer.max_x - 0.1 {
            let inside_inner = x >= inner.min_x - 0.1
                && x + FAR_CELL_M <= inner.max_x + 0.1
                && z >= inner.min_z - 0.1
                && z + FAR_CELL_M <= inner.max_z + 0.1;
            if !inside_inner {
                build.quad(cell_corners(field, x, z, FAR_CELL_M, &inner));
            }
            x += FAR_CELL_M;
        }
        z += FAR_CELL_M;
    }
    let mut skirts = Vec::new();
    for ((a, b), _) in build.open {
        let va = build.vertices[a as usize];
        let vb = build.vertices[b as usize];
        skirts.push(Skirt {
            x0: va.x,
            y0: va.y,
            z0: va.z,
            x1: vb.x,
            y1: vb.y,
            z1: vb.z,
        });
    }
    PlannedFar {
        vertices: build.vertices,
        triangles: build.triangles,
        skirts,
    }
}

struct Box2 {
    min_x: f32,
    max_x: f32,
    min_z: f32,
    max_z: f32,
}

fn bounds(min_x: f32, max_x: f32, min_z: f32, max_z: f32, reach: f32, snap: f32) -> Box2 {
    Box2 {
        min_x: ((min_x - reach) / snap).floor() * snap,
        max_x: ((max_x + reach) / snap).ceil() * snap,
        min_z: ((min_z - reach) / snap).floor() * snap,
        max_z: ((max_z + reach) / snap).ceil() * snap,
    }
}

fn cell_in_corridor(x: f32, z: f32, cell: f32, occupied: &HashSet<(i32, i32)>) -> bool {
    let cx = (x / CHUNK_M).floor() as i32;
    let cz = (z / CHUNK_M).floor() as i32;
    let x0 = cx as f32 * CHUNK_M;
    let z0 = cz as f32 * CHUNK_M;
    let inside = x >= x0 - 0.01
        && x + cell <= x0 + CHUNK_M + 0.01
        && z >= z0 - 0.01
        && z + cell <= z0 + CHUNK_M + 0.01;
    inside && occupied.contains(&(cx, cz))
}

fn cell_corners(
    field: &Field,
    x: f32,
    z: f32,
    cell: f32,
    inner: &Box2,
) -> [(f32, f32, f32, bool); 4] {
    let points = [(x, z), (x + cell, z), (x + cell, z + cell), (x, z + cell)];
    points.map(|(px, pz)| {
        let (y, sampled) = corner_height(field, px, pz, inner);
        (px, y, pz, sampled)
    })
}

fn corner_height(field: &Field, x: f32, z: f32, inner: &Box2) -> (f32, bool) {
    let on = |value: f32, edge: f32| (value - edge).abs() < 0.2;
    let on_x = on(x, inner.min_x) || on(x, inner.max_x);
    let on_z = on(z, inner.min_z) || on(z, inner.max_z);
    if on_x ^ on_z {
        let along_x = on_z;
        return seam_or_sample(x, z, along_x, FAR_CELL_M, |px, pz| field.height(px, pz));
    }
    (field.height(x, z), true)
}

fn collect_borders(
    chunks: &[PlannedChunk],
    far: &PlannedFar,
    occupied: &HashSet<(i32, i32)>,
) -> Vec<Border> {
    let mut by_coord = HashMap::new();
    for chunk in chunks {
        by_coord.insert((chunk.cx, chunk.cz), chunk);
    }
    let mut borders = Vec::new();
    for chunk in chunks {
        if let Some(east) = by_coord.get(&(chunk.cx + 1, chunk.cz)) {
            borders.push(Border {
                left: chunk_edge_points(chunk, Edge::East),
                right: chunk_edge_points(east, Edge::West),
            });
        }
        if let Some(north) = by_coord.get(&(chunk.cx, chunk.cz + 1)) {
            borders.push(Border {
                left: chunk_edge_points(chunk, Edge::North),
                right: chunk_edge_points(north, Edge::South),
            });
        }
        if !occupied.contains(&(chunk.cx - 1, chunk.cz)) {
            push_far_border(&mut borders, chunk, Edge::West, far);
        }
        if !occupied.contains(&(chunk.cx + 1, chunk.cz)) {
            push_far_border(&mut borders, chunk, Edge::East, far);
        }
        if !occupied.contains(&(chunk.cx, chunk.cz - 1)) {
            push_far_border(&mut borders, chunk, Edge::South, far);
        }
        if !occupied.contains(&(chunk.cx, chunk.cz + 1)) {
            push_far_border(&mut borders, chunk, Edge::North, far);
        }
    }
    borders
}

fn chunk_edge_points(chunk: &PlannedChunk, edge: Edge) -> Vec<[f32; 3]> {
    let n = chunk.steps + 1;
    let origin_x = chunk.cx as f32 * CHUNK_M;
    let origin_z = chunk.cz as f32 * CHUNK_M;
    let mut points = Vec::new();
    match edge {
        Edge::West | Edge::East => {
            let ix = if matches!(edge, Edge::West) { 0 } else { n - 1 };
            for iz in 0..n {
                let y = chunk.heights[(iz * n + ix) as usize];
                points.push([
                    origin_x + ix as f32 * chunk.cell,
                    y,
                    origin_z + iz as f32 * chunk.cell,
                ]);
            }
        }
        Edge::South | Edge::North => {
            let iz = if matches!(edge, Edge::South) {
                0
            } else {
                n - 1
            };
            for ix in 0..n {
                let y = chunk.heights[(iz * n + ix) as usize];
                points.push([
                    origin_x + ix as f32 * chunk.cell,
                    y,
                    origin_z + iz as f32 * chunk.cell,
                ]);
            }
        }
    }
    points
}

fn push_far_border(borders: &mut Vec<Border>, chunk: &PlannedChunk, edge: Edge, far: &PlannedFar) {
    let left = chunk_edge_points(chunk, edge);
    if left.len() < 2 {
        return;
    }
    let start = left[0];
    let end = left[left.len() - 1];
    let mut right: Vec<[f32; 3]> = far
        .vertices
        .iter()
        .filter(|vertex| on_segment(vertex.x, vertex.z, start, end))
        .map(|vertex| [vertex.x, vertex.y, vertex.z])
        .collect();
    if right.len() < 2 {
        return;
    }
    right.sort_by(|a, b| {
        let da = (a[0] - start[0]).powi(2) + (a[2] - start[2]).powi(2);
        let db = (b[0] - start[0]).powi(2) + (b[2] - start[2]).powi(2);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    borders.push(Border { left, right });
}

fn on_segment(x: f32, z: f32, start: [f32; 3], end: [f32; 3]) -> bool {
    let dx = end[0] - start[0];
    let dz = end[2] - start[2];
    let len2 = dx * dx + dz * dz;
    if len2 < 1.0e-4 {
        return false;
    }
    let t = ((x - start[0]) * dx + (z - start[2]) * dz) / len2;
    if !(0.0..=1.0).contains(&t) {
        return false;
    }
    let px = start[0] + dx * t;
    let pz = start[2] + dz * t;
    (x - px).hypot(z - pz) < 0.05
}

/// Height of a point by walking `chain` and lerping along it.
#[must_use]
#[allow(dead_code)]
pub fn height_along(chain: &[[f32; 3]], distance: f32) -> Option<[f32; 3]> {
    if chain.len() < 2 {
        return None;
    }
    let mut walked = 0.0;
    for pair in chain.windows(2) {
        let dx = pair[1][0] - pair[0][0];
        let dz = pair[1][2] - pair[0][2];
        let len = dx.hypot(dz);
        if walked + len + 1.0e-3 >= distance {
            let t = if len < 1.0e-4 {
                0.0
            } else {
                ((distance - walked) / len).clamp(0.0, 1.0)
            };
            return Some([
                pair[0][0] + dx * t,
                pair[0][1] + (pair[1][1] - pair[0][1]) * t,
                pair[0][2] + dz * t,
            ]);
        }
        walked += len;
    }
    chain.last().copied()
}

/// Length of a polyline in the horizontal plane.
#[must_use]
#[allow(dead_code)]
pub fn chain_length(chain: &[[f32; 3]]) -> f32 {
    chain.windows(2).fold(0.0, |sum, pair| {
        sum + (pair[1][0] - pair[0][0]).hypot(pair[1][2] - pair[0][2])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heightfield::Field;
    use pp_course::Course;
    use pp_routegen::{RouteSpec, Terrain, TerrainPreset, generate};
    use pp_view::theme::Theme;

    fn field_for(course: &Course, seed: u32) -> Field {
        let mut raw = Vec::new();
        let mut distance = 0.0;
        while distance < course.length_m() {
            let position = course.position_at(distance);
            raw.push([position[0], position[1] + 0.08, position[2]]);
            distance += 2.0;
        }
        let mut field = Field::new(&raw, seed as i32);
        field.use_relief(Theme::builtin().relief.clone(), seed);
        field
    }

    fn assert_plan(field: &Field) {
        let plan = plan(field);
        for chunk in &plan.chunks {
            let n = chunk.steps + 1;
            let origin_x = chunk.cx as f32 * CHUNK_M;
            let origin_z = chunk.cz as f32 * CHUNK_M;
            for iz in 0..n {
                for ix in 0..n {
                    let index = (iz * n + ix) as usize;
                    let x = origin_x + ix as f32 * chunk.cell;
                    let z = origin_z + iz as f32 * chunk.cell;
                    let y = chunk.heights[index];
                    if chunk.sampled[index] {
                        let expect = field.height(x, z);
                        assert!(
                            (y - expect).abs() < 0.01,
                            "chunk vertex {y:.3} != height {expect:.3} at ({x:.1}, {z:.1})"
                        );
                    }
                }
            }
            assert!(
                chunk
                    .skirts
                    .iter()
                    .all(|skirt| skirt.y0.is_finite() && skirt.y1.is_finite()),
                "a skirt height is not finite"
            );
        }
        for vertex in &plan.far.vertices {
            if vertex.sampled {
                let expect = field.height(vertex.x, vertex.z);
                assert!(
                    (vertex.y - expect).abs() < 0.01,
                    "far vertex {y:.3} != height {expect:.3} at ({x:.1}, {z:.1})",
                    y = vertex.y,
                    expect = expect,
                    x = vertex.x,
                    z = vertex.z
                );
            }
        }
        assert!(
            !plan.far.skirts.is_empty(),
            "the far mesh has an outer skirt"
        );
        for border in &plan.borders {
            let length = chain_length(&border.left).min(chain_length(&border.right));
            let mut distance = 0.0;
            while distance <= length {
                let left = height_along(&border.left, distance).expect("left");
                let right = height_along(&border.right, distance).expect("right");
                assert!(
                    (left[1] - right[1]).abs() < 0.05,
                    "border disagrees by {:.3} m at {distance:.0}",
                    (left[1] - right[1]).abs()
                );
                distance += 1.0;
            }
        }
    }

    #[test]
    fn ground_vertices_lie_on_the_height_and_borders_meet() {
        assert_plan(&field_for(&Course::penguin_pass(), 7));
        let spec = RouteSpec::new(
            "mountain",
            11,
            10_000.0,
            Terrain::Preset(TerrainPreset::Mountain),
        );
        let course = generate(&spec).expect("mountain").course().clone();
        assert_plan(&field_for(&course, 11));
    }
}
