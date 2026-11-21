//! The ground height the meshes and the scatter share.
//!
//! Near the centreline this is the exact relief formula. Further out it fades
//! to one baked surface. Every detail level calls [`Field::height`], so a
//! vertex cannot sit on a different formula from its neighbour.

use pp_view::relief::{self, Relief};
use pp_view::roadbed::RoadIndex;
use pp_view::theme::Theme;

use crate::ChunkGrid;

const CHUNK_M: f32 = 128.0;
const CORRIDOR_HALF_M: f64 = 288.0;
const GRID_PAD_M: f32 = 360.0;
const GRID_CELL_M: f32 = 8.0;
const FAR_GRID_M: f32 = 48.0;
const FAR_PAD_M: f32 = 1_400.0;
const FAR_REACH_M: f32 = 5_600.0;
/// Within this distance of the centreline the ground is the exact formula.
const EXACT_NEAR_M: f32 = 96.0;
/// Past this distance the ground is the baked surface alone.
const EXACT_FAR_M: f32 = 192.0;
/// Chunks whose centre is this close to the road use the 4 m grid.
const NEAR_ROAD_M: f32 = 48.0;
const FINE_M: f32 = 4.0;
const COARSE_M: f32 = 8.0;

/// Course samples arranged so a chunk can ask "how far is the road?" on its own.
pub struct Field {
    origin_x: f32,
    origin_z: f32,
    cell: f32,
    nx: i32,
    nz: i32,
    dist: Vec<f32>,
    road_y: Vec<f32>,
    #[allow(dead_code)]
    blend_w: Vec<f32>,
    #[allow(dead_code)]
    blend_y: Vec<f32>,
    roads: RoadIndex,
    corridor: Vec<(i32, i32)>,
    span_min_x: f32,
    span_max_x: f32,
    span_min_z: f32,
    span_max_z: f32,
    relief: Relief,
    relief_seed: u32,
    far_origin_x: f32,
    far_origin_z: f32,
    far_cell: f32,
    far_nx: i32,
    far_nz: i32,
    far_dist: Vec<f32>,
    far_y: Vec<f32>,
    datum_y: f32,
    surface_origin_x: f32,
    surface_origin_z: f32,
    surface_cell: f32,
    surface_nx: i32,
    surface_nz: i32,
    surface: Vec<f32>,
    snow_line: f32,
    /// Flat pads for landmarks, `(x, z, radius_m, height_m)`.
    pads: Vec<(f32, f32, f32, f32)>,
}

impl Field {
    /// Build the distance grids from ribbon samples.
    ///
    /// Callers pass the ribbon samples, which sit 0.08 m above the course.
    /// The ground under the asphalt is lower so the two surfaces do not fight.
    #[must_use]
    pub fn new(samples: &[[f32; 3]], seed: i32) -> Self {
        let lowered: Vec<[f32; 3]> = samples
            .iter()
            .map(|sample| [sample[0], sample[1] - 0.22, sample[2]])
            .collect();
        let samples = lowered.as_slice();
        let (min_x, max_x, min_z, max_z) = tight_bounds(samples);
        let cell = GRID_CELL_M;
        let origin_x = min_x - GRID_PAD_M;
        let origin_z = min_z - GRID_PAD_M;
        let nx = ((max_x + GRID_PAD_M - origin_x) / cell).ceil() as i32 + 1;
        let nz = ((max_z + GRID_PAD_M - origin_z) / cell).ceil() as i32 + 1;
        let nx = nx.max(1);
        let nz = nz.max(1);
        let mut dist = vec![1.0e9_f32; (nx * nz) as usize];
        let mut road_y = vec![0.0_f32; (nx * nz) as usize];
        let mut blend_w = vec![0.0_f32; (nx * nz) as usize];
        let mut blend_y = vec![0.0_f32; (nx * nz) as usize];
        let reach = (340.0 / cell).ceil() as i32;
        for (step, sample) in samples.iter().enumerate() {
            if step % 2 != 0 && step + 1 != samples.len() {
                continue;
            }
            let cx = ((sample[0] - origin_x) / cell).round() as i32;
            let cz = ((sample[2] - origin_z) / cell).round() as i32;
            for dz in -reach..=reach {
                for dx in -reach..=reach {
                    let ix = cx + dx;
                    let iz = cz + dz;
                    if ix < 0 || iz < 0 || ix >= nx || iz >= nz {
                        continue;
                    }
                    let x = origin_x + ix as f32 * cell;
                    let z = origin_z + iz as f32 * cell;
                    let d = ((sample[0] - x).powi(2) + (sample[2] - z).powi(2)).sqrt();
                    let index = (iz * nx + ix) as usize;
                    if d < dist[index] {
                        dist[index] = d;
                        road_y[index] = sample[1];
                    }
                    if d < 120.0 {
                        let w = 1.0 / (d * d + 24.0);
                        blend_w[index] += w;
                        blend_y[index] += w * sample[1];
                    }
                }
            }
        }
        let polyline: Vec<[f64; 2]> = samples
            .iter()
            .map(|sample| [f64::from(sample[0]), f64::from(sample[2])])
            .collect();
        let corridor = ChunkGrid::default()
            .chunks_along_polyline(&polyline, CORRIDOR_HALF_M)
            .into_iter()
            .map(|coord| (coord.x, coord.z))
            .collect();
        let (far_origin_x, far_origin_z, far_nx, far_nz, far_dist, far_y) =
            far_distance_grid(samples, min_x, max_x, min_z, max_z);
        let datum_y = samples
            .get(samples.len() / 2)
            .map(|sample| sample[1])
            .unwrap_or(0.0);
        Self {
            origin_x,
            origin_z,
            cell,
            nx,
            nz,
            dist,
            road_y,
            blend_w,
            blend_y,
            roads: RoadIndex::build(samples),
            corridor,
            span_min_x: min_x,
            span_max_x: max_x,
            span_min_z: min_z,
            span_max_z: max_z,
            relief: Theme::builtin().relief,
            relief_seed: seed as u32,
            far_origin_x,
            far_origin_z,
            far_cell: FAR_GRID_M,
            far_nx,
            far_nz,
            far_dist,
            far_y,
            datum_y,
            surface_origin_x: 0.0,
            surface_origin_z: 0.0,
            surface_cell: 32.0,
            surface_nx: 1,
            surface_nz: 1,
            surface: vec![0.0],
            snow_line: 200.0,
            pads: Vec::new(),
        }
    }

    /// Level small pads into the ground. A pad never changes the road bed:
    /// inside 12 m of the centreline the height is left alone.
    pub fn set_pads(&mut self, pads: Vec<(f32, f32, f32, f32)>) {
        self.pads = pads;
    }

    /// Mountains for this theme, then one baked surface every level samples.
    pub fn use_relief(&mut self, relief: Relief, seed: u32) {
        self.relief = relief;
        self.relief_seed = seed;
        self.bake_surface();
    }

    fn bake_surface(&mut self) {
        let reach = FAR_REACH_M + 400.0;
        let cell = 48.0;
        let origin_x = ((self.span_min_x - reach) / cell).floor() * cell;
        let origin_z = ((self.span_min_z - reach) / cell).floor() * cell;
        let max_x = self.span_max_x + reach;
        let max_z = self.span_max_z + reach;
        let nx = ((max_x - origin_x) / cell).ceil() as i32 + 1;
        let nz = ((max_z - origin_z) / cell).ceil() as i32 + 1;
        let nx = nx.max(2);
        let nz = nz.max(2);
        let mut surface = vec![0.0_f32; (nx * nz) as usize];
        let mut elev_min = f32::MAX;
        let mut elev_max = f32::MIN;
        for iz in 0..nz {
            for ix in 0..nx {
                let x = origin_x + ix as f32 * cell;
                let z = origin_z + iz as f32 * cell;
                let h = self.height_exact(x, z);
                surface[(iz * nx + ix) as usize] = h;
                elev_min = elev_min.min(h);
                elev_max = elev_max.max(h);
            }
        }
        let span = (elev_max - elev_min).max(80.0);
        self.snow_line = elev_min + span * 0.78;
        self.surface_origin_x = origin_x;
        self.surface_origin_z = origin_z;
        self.surface_cell = cell;
        self.surface_nx = nx;
        self.surface_nz = nz;
        self.surface = surface;
    }

    /// The exact relief formula, before the far bake is blended in.
    #[must_use]
    pub fn height_exact(&self, x: f32, z: f32) -> f32 {
        let (dist, road_y, field_y) = self.road_inputs(x, z);
        relief::compose(x, z, dist, road_y, field_y, &self.relief, self.relief_seed)
    }

    fn road_inputs(&self, x: f32, z: f32) -> (f32, f32, f32) {
        let Some((dist, tight, wide)) = self.roads.blend_heights(x, z) else {
            return (3_000.0, self.datum_y, self.datum_y);
        };
        let mix = relief::smoothstep(6.0, 30.0, dist);
        let road_y = tight * (1.0 - mix) + wide * mix;
        (dist, road_y, wide)
    }

    /// Horizontal distance to the centreline and that sample's height.
    ///
    /// Inside the road bed the distance is measured against every sample, not
    /// the 8 m grid, so a parallel leg cannot hide the nearer one.
    #[must_use]
    pub fn road(&self, x: f32, z: f32) -> (f32, f32) {
        let (grid_distance, grid_height) = self.grid_road(x, z);
        if grid_distance <= 24.0 {
            return self
                .roads
                .nearest(x, z, 48.0)
                .unwrap_or((grid_distance, grid_height));
        }
        if grid_distance < 1.0e8 {
            return (grid_distance, grid_height);
        }
        let (far_distance, far_height) = self.far_road(x, z);
        if far_distance < 1.0e8 {
            return (far_distance, far_height);
        }
        (2_000.0, self.datum_y)
    }

    /// Terrain height at a world position.
    ///
    /// The 48 m bake cannot hold the road bed or its banks, so near the road
    /// the height is the exact formula. It fades to the bake between
    /// [`EXACT_NEAR_M`] and [`EXACT_FAR_M`] from the centreline. The fade is a
    /// function of position only, so every level still agrees.
    #[must_use]
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let base = if self.road(x, z).0 > EXACT_FAR_M + 16.0 {
            self.sample_surface(x, z)
        } else {
            let (dist, road_y, field_y) = self.road_inputs(x, z);
            let exact =
                relief::compose(x, z, dist, road_y, field_y, &self.relief, self.relief_seed);
            let fade = relief::smoothstep(EXACT_NEAR_M, EXACT_FAR_M, dist);
            if fade <= 0.0 {
                exact
            } else {
                exact + (self.sample_surface(x, z) - exact) * fade
            }
        };
        self.with_pads(x, z, base)
    }

    fn with_pads(&self, x: f32, z: f32, height: f32) -> f32 {
        if self.pads.is_empty() || self.road(x, z).0 < 12.0 {
            return height;
        }
        let mut out = height;
        for &(px, pz, radius, flat) in &self.pads {
            let distance = (x - px).hypot(z - pz);
            if distance >= radius {
                continue;
            }
            let edge = relief::smoothstep(radius * 0.45, radius, distance);
            out = flat * (1.0 - edge) + height * edge;
        }
        out
    }

    fn sample_surface(&self, x: f32, z: f32) -> f32 {
        let cell = self.surface_cell;
        let fx = (x - self.surface_origin_x) / cell;
        let fz = (z - self.surface_origin_z) / cell;
        let x0 = fx.floor() as i32;
        let z0 = fz.floor() as i32;
        let tx = fx - x0 as f32;
        let tz = fz - z0 as f32;
        let h00 = self.surface_at(x0, z0);
        let h10 = self.surface_at(x0 + 1, z0);
        let h01 = self.surface_at(x0, z0 + 1);
        let h11 = self.surface_at(x0 + 1, z0 + 1);
        let a = h00 + (h10 - h00) * tx;
        let b = h01 + (h11 - h01) * tx;
        a + (b - a) * tz
    }

    fn surface_at(&self, ix: i32, iz: i32) -> f32 {
        if self.surface.is_empty() {
            return self.datum_y;
        }
        let ix = ix.clamp(0, self.surface_nx - 1);
        let iz = iz.clamp(0, self.surface_nz - 1);
        self.surface[(iz * self.surface_nx + ix) as usize]
    }

    fn grid_road(&self, x: f32, z: f32) -> (f32, f32) {
        let ix = ((x - self.origin_x) / self.cell).round() as i32;
        let iz = ((z - self.origin_z) / self.cell).round() as i32;
        if ix < 0 || iz < 0 || ix >= self.nx || iz >= self.nz {
            return (1.0e9, 0.0);
        }
        let index = (iz * self.nx + ix) as usize;
        (self.dist[index], self.road_y[index])
    }

    fn far_road(&self, x: f32, z: f32) -> (f32, f32) {
        let ix = ((x - self.far_origin_x) / self.far_cell).round() as i32;
        let iz = ((z - self.far_origin_z) / self.far_cell).round() as i32;
        if ix < 0 || iz < 0 || ix >= self.far_nx || iz >= self.far_nz {
            return (1.0e9, self.datum_y);
        }
        let index = (iz * self.far_nx + ix) as usize;
        (self.far_dist[index], self.far_y[index])
    }

    /// Visit each corridor chunk coordinate.
    pub fn each_corridor_chunk(&self, mut visit: impl FnMut(i32, i32)) {
        for &(cx, cz) in &self.corridor {
            visit(cx, cz);
        }
    }

    /// Corridor chunk coordinates, x then z.
    #[must_use]
    pub fn corridor(&self) -> &[(i32, i32)] {
        &self.corridor
    }

    /// Snow line for this course, relative to the baked height range.
    #[must_use]
    pub fn snow_line(&self) -> f32 {
        self.snow_line
    }

    /// Axis-aligned bounds of the centreline samples, in meters.
    #[must_use]
    pub fn spans(&self) -> (f32, f32, f32, f32) {
        (
            self.span_min_x,
            self.span_max_x,
            self.span_min_z,
            self.span_max_z,
        )
    }

    /// Cell size of one corridor chunk: 4 m beside the road, 8 m further out.
    #[must_use]
    pub fn chunk_cell(&self, cx: i32, cz: i32) -> f32 {
        let center_x = cx as f32 * CHUNK_M + CHUNK_M * 0.5;
        let center_z = cz as f32 * CHUNK_M + CHUNK_M * 0.5;
        if self.road(center_x, center_z).0 < NEAR_ROAD_M {
            FINE_M
        } else {
            COARSE_M
        }
    }

    /// Distance from the world origin to the outer corner of the far grid.
    ///
    /// The shader hazes the last fifth of this distance so the mesh edge is
    /// not a hard line.
    #[must_use]
    pub fn haze_radius(&self) -> f32 {
        let reach = FAR_REACH_M;
        let min_x = self.span_min_x - reach;
        let max_x = self.span_max_x + reach;
        let min_z = self.span_min_z - reach;
        let max_z = self.span_max_z + reach;
        let corners = [
            (min_x, min_z),
            (min_x, max_z),
            (max_x, min_z),
            (max_x, max_z),
        ];
        corners
            .into_iter()
            .map(|(x, z)| x.hypot(z))
            .fold(1_000.0_f32, f32::max)
    }
}

fn tight_bounds(samples: &[[f32; 3]]) -> (f32, f32, f32, f32) {
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_z = f32::MAX;
    let mut max_z = f32::MIN;
    for sample in samples {
        min_x = min_x.min(sample[0]);
        max_x = max_x.max(sample[0]);
        min_z = min_z.min(sample[2]);
        max_z = max_z.max(sample[2]);
    }
    if samples.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        (min_x, max_x, min_z, max_z)
    }
}

fn far_distance_grid(
    samples: &[[f32; 3]],
    min_x: f32,
    max_x: f32,
    min_z: f32,
    max_z: f32,
) -> (f32, f32, i32, i32, Vec<f32>, Vec<f32>) {
    let origin_x = min_x - FAR_PAD_M;
    let origin_z = min_z - FAR_PAD_M;
    let nx = ((max_x + FAR_PAD_M - origin_x) / FAR_GRID_M).ceil() as i32 + 1;
    let nz = ((max_z + FAR_PAD_M - origin_z) / FAR_GRID_M).ceil() as i32 + 1;
    let nx = nx.max(1);
    let nz = nz.max(1);
    let mut dist = vec![1.0e9_f32; (nx * nz) as usize];
    let mut road_y = vec![0.0_f32; (nx * nz) as usize];
    let reach = (FAR_PAD_M / FAR_GRID_M).ceil() as i32;
    for (step, sample) in samples.iter().enumerate() {
        if step % 4 != 0 && step + 1 != samples.len() {
            continue;
        }
        let cx = ((sample[0] - origin_x) / FAR_GRID_M).round() as i32;
        let cz = ((sample[2] - origin_z) / FAR_GRID_M).round() as i32;
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let ix = cx + dx;
                let iz = cz + dz;
                if ix < 0 || iz < 0 || ix >= nx || iz >= nz {
                    continue;
                }
                let x = origin_x + ix as f32 * FAR_GRID_M;
                let z = origin_z + iz as f32 * FAR_GRID_M;
                let d = ((sample[0] - x).powi(2) + (sample[2] - z).powi(2)).sqrt();
                let index = (iz * nx + ix) as usize;
                if d < dist[index] {
                    dist[index] = d;
                    road_y[index] = sample[1];
                }
            }
        }
    }
    (origin_x, origin_z, nx, nz, dist, road_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pp_course::Course;
    use pp_routegen::{RouteSpec, Terrain, TerrainPreset, generate};

    fn field_for(course: &Course) -> Field {
        let mut raw = Vec::new();
        let mut distance = 0.0;
        let length = course.length_m();
        while distance < length {
            let position = course.position_at(distance);
            raw.push([position[0], position[1] + 0.08, position[2]]);
            distance += 2.0;
        }
        raw.push({
            let position = course.position_at(0.0);
            [position[0], position[1] + 0.08, position[2]]
        });
        let mut field = Field::new(&raw, 7);
        field.use_relief(Theme::builtin().relief, 7);
        field
    }

    fn assert_bed(course: &Course) {
        let field = field_for(course);
        let mut distance = 0.0;
        let length = course.length_m();
        while distance < length {
            let position = course.position_at(distance);
            let tangent = course.tangent_at(distance);
            let mut right_x = -tangent[2];
            let mut right_z = tangent[0];
            let span = (right_x * right_x + right_z * right_z).sqrt();
            if span > 1.0e-4 {
                right_x /= span;
                right_z /= span;
            }
            let road_surface = position[1] + 0.08;
            for offset in [-3.0_f32, 0.0, 3.0] {
                let x = position[0] + right_x * offset;
                let z = position[2] + right_z * offset;
                let height = field.height(x, z);
                let formula = field.height_exact(x, z);
                assert!(
                    (height - formula).abs() < 0.05,
                    "bed drift {height:.3} vs formula {formula:.3} at {distance:.0} m offset {offset}"
                );
                assert!(
                    height <= road_surface + 0.005,
                    "ground {height:.3} rises above the road {road_surface:.3} at {distance:.0} m offset {offset}"
                );
            }
            distance += 2.0;
        }
    }

    #[test]
    fn the_road_bed_stays_under_the_pavement() {
        assert_bed(&Course::penguin_pass());
        let spec = RouteSpec::new(
            "mountain",
            11,
            10_000.0,
            Terrain::Preset(TerrainPreset::Mountain),
        );
        let course = generate(&spec).expect("mountain").course().clone();
        assert_bed(&course);
    }
}
