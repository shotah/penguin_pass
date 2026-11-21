//! Chunked heightfield. Near-road chunks are finer than the hills behind them.
//!
//! Each chunk is a function of its coordinates, the course samples, and the
//! theme seed. Chunks do not read each other's meshes.

use godot::classes::base_material_3d::Flags;
use godot::classes::mesh::PrimitiveType;
use godot::classes::{ArrayMesh, MeshInstance3D, Shader, ShaderMaterial, SurfaceTool};
use std::collections::HashSet;

use godot::obj::Gd;
use godot::prelude::*;
use pp_course::Course;
use pp_view::roadbed::RoadIndex;

use crate::assets::Catalog;
use crate::road::CenterSample;
use crate::theme::{Theme, rgb};

const CHUNK_M: f32 = 128.0;
const FINE_M: f32 = 4.0;
const COARSE_M: f32 = 8.0;
const NEAR_ROAD_M: f32 = 48.0;
/// Ground built along the road. A filled bounding box does not scale with a
/// 10 km loop, and this is wide enough that the rider is not looking at the
/// mesh edge.
const CORRIDOR_HALF_M: f64 = 288.0;
const GRID_PAD_M: f32 = 360.0;
const GRID_CELL_M: f32 = 8.0;
const MID_CELL_M: f32 = 64.0;
const FAR_CELL_M: f32 = 256.0;
const MID_REACH_M: f32 = 1_500.0;
const FAR_REACH_M: f32 = 5_600.0;
/// Distance grid past the fine stamp, so the relief blend still knows the road.
const FAR_GRID_M: f32 = 48.0;
const FAR_PAD_M: f32 = 1_400.0;

const TERRAIN_SHADER: &str = r#"shader_type spatial;
render_mode diffuse_burley, specular_disabled;

varying vec3 world_pos;
varying vec3 world_normal;

float hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    float a = hash(i);
    float b = hash(i + vec2(1.0, 0.0));
    float c = hash(i + vec2(0.0, 1.0));
    float d = hash(i + vec2(1.0, 1.0));
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

void vertex() {
    world_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
    world_normal = normalize((MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz);
}

uniform sampler2D layer0 : source_color, repeat_enable, filter_linear_mipmap;
uniform sampler2D layer1 : source_color, repeat_enable, filter_linear_mipmap;
uniform sampler2D layer2 : source_color, repeat_enable, filter_linear_mipmap;
uniform sampler2D layer3 : source_color, repeat_enable, filter_linear_mipmap;
uniform float tile0 = 4.0;
uniform float tile1 = 4.0;
uniform float tile2 = 6.0;
uniform float tile3 = 8.0;
uniform float textured = 0.0;
uniform vec3 haze_color = vec3(0.62, 0.74, 0.86);
uniform float haze_start = 600.0;
uniform float haze_end = 7000.0;

vec3 triplanar(sampler2D tex, float tile) {
    vec3 n = abs(world_normal);
    n /= max(n.x + n.y + n.z, 0.001);
    vec3 x = texture(tex, world_pos.zy / tile).rgb;
    vec3 y = texture(tex, world_pos.xz / tile).rgb;
    vec3 z = texture(tex, world_pos.xy / tile).rgb;
    return x * n.x + y * n.y + z * n.z;
}

void fragment() {
    float fine = noise(world_pos.xz * 0.09);
    float macro = noise(world_pos.xz * 0.012);
    vec3 tint = mix(vec3(0.94, 0.97, 0.93), vec3(1.04, 1.0, 0.96), macro);
    tint *= mix(0.92, 1.06, fine);
    vec3 procedural = COLOR.rgb * tint;
    vec4 w = COLOR;
    vec3 snow = mix(triplanar(layer3, tile3), vec3(0.93, 0.95, 0.97), 0.55);
    vec3 sampled = triplanar(layer0, tile0) * w.r
        + triplanar(layer1, tile1) * w.g
        + triplanar(layer2, tile2) * w.b
        + snow * w.a;
    ALBEDO = mix(procedural, sampled * tint, textured);
    float away = smoothstep(haze_start, haze_end, length(VERTEX));
    ALBEDO = mix(ALBEDO, haze_color, away * 0.42);
    ROUGHNESS = mix(0.94, 0.78, w.a * textured);
}
"#;

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
    relief: pp_view::relief::Relief,
    relief_seed: u32,
    far_origin_x: f32,
    far_origin_z: f32,
    far_cell: f32,
    far_nx: i32,
    far_nz: i32,
    far_dist: Vec<f32>,
    far_y: Vec<f32>,
    datum_y: f32,
    /// Baked surface. Every mesh level samples this, so a border cannot step.
    surface_origin_x: f32,
    surface_origin_z: f32,
    surface_cell: f32,
    surface_nx: i32,
    surface_nz: i32,
    surface: Vec<f32>,
    snow_line: f32,
}

impl Field {
    pub(crate) fn new(samples: &[[f32; 3]], seed: i32) -> Self {
        // Callers pass the ribbon samples, which sit 0.08 m above the course.
        // The ground under the asphalt is lower so the two surfaces do not fight.
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
        // Past this stamp a cell's distance stays at 1e9 and the height
        // function used to drop the road clamp, so one vertex became a needle.
        let reach = (340.0 / cell).ceil() as i32;
        for (step, sample) in samples.iter().enumerate() {
            // Samples are a metre or two apart. The 8 m grid does not need all of them.
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
        let corridor = pp_world::ChunkGrid::default()
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
            relief: pp_view::theme::Theme::builtin().relief,
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
        }
    }

    /// Mountains for this theme, then one baked surface every level samples.
    pub(crate) fn use_relief(&mut self, relief: pp_view::relief::Relief, seed: u32) {
        self.relief = relief;
        self.relief_seed = seed;
        self.bake_surface();
    }

    fn bake_surface(&mut self) {
        let reach = FAR_REACH_M + 400.0;
        let cell = 48.0;
        let origin_x = (self.span_min_x - reach) / cell;
        let origin_x = origin_x.floor() * cell;
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

    /// The one ground formula. The mesh samples a bake of this, so every
    /// level agrees, and this is what the continuity tests cover.
    fn height_exact(&self, x: f32, z: f32) -> f32 {
        let (dist, road_y, field_y) = self.road_inputs(x, z);
        pp_view::relief::compose(x, z, dist, road_y, field_y, &self.relief, self.relief_seed)
    }

    fn road_inputs(&self, x: f32, z: f32) -> (f32, f32, f32) {
        let Some((dist, tight, wide)) = self.roads.blend_heights(x, z) else {
            return (3_000.0, self.datum_y, self.datum_y);
        };
        let mix = pp_view::relief::smoothstep(6.0, 30.0, dist);
        let road_y = tight * (1.0 - mix) + wide * mix;
        (dist, road_y, wide)
    }

    /// Horizontal distance to the centreline and that sample's height.
    ///
    /// Inside the road bed the distance is measured against every sample, not
    /// the 8 m grid, so a parallel leg cannot hide the nearer one.
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
    /// The tuck outside the course is part of the height, so scatter and the
    /// mesh share one surface. A prop on the un-tucked height floats.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        self.sample_surface(x, z)
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

    #[allow(dead_code)]
    fn blended_road(&self, x: f32, z: f32) -> Option<f32> {
        let ix = ((x - self.origin_x) / self.cell).round() as i32;
        let iz = ((z - self.origin_z) / self.cell).round() as i32;
        if ix < 0 || iz < 0 || ix >= self.nx || iz >= self.nz {
            return None;
        }
        let index = (iz * self.nx + ix) as usize;
        let weight = self.blend_w[index];
        if weight <= 0.0 {
            None
        } else {
            Some(self.blend_y[index] / weight)
        }
    }

    pub(crate) fn each_corridor_chunk(&self, mut visit: impl FnMut(i32, i32)) {
        for &(cx, cz) in &self.corridor {
            visit(cx, cz);
        }
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

/// Build chunk meshes and the query field the scatter and rails share.
pub fn build(
    parent: &mut Gd<Node3D>,
    course: &Course,
    theme: &Theme,
    catalog: &Catalog,
) -> (Vec<CenterSample>, Field) {
    let samples = sample_centerline(course);
    let raw: Vec<[f32; 3]> = samples
        .iter()
        .map(|sample| [sample.position.x, sample.position.y, sample.position.z])
        .collect();
    let started = godot::classes::Time::singleton().get_ticks_msec();
    let mut field = Field::new(&raw, theme.seed as i32);
    field.use_relief(theme.relief.clone(), theme.seed);
    let mut material = terrain_material();
    let haze = Vector3::new(theme.fog.color[0], theme.fog.color[1], theme.fog.color[2]);
    material.set_shader_parameter("haze_color", &haze.to_variant());
    let textured = catalog.bind_ground(&mut material, theme);
    let mut group = Node3D::new_alloc();
    group.set_name("Terrain");
    let mut chunks = 0_i32;
    let mut terrain_tris = 0_i32;
    for &(cx, cz) in &field.corridor {
        terrain_tris += emit_chunk(&mut group, &field, theme, &material, cx, cz, textured);
        chunks += 1;
    }
    let range_tris = add_range(&mut group, &field, theme, &material, textured);
    let fill_tris = 0;
    let backdrop_tris = range_tris;
    parent.add_child(&group);
    let elapsed = godot::classes::Time::singleton().get_ticks_msec() - started;
    let road_tris = samples.len().saturating_sub(1) * 2;
    godot_print!(
        "PENGUIN_PASS_WORLD ms={} chunks={} terrain_tris={} fill_tris={} backdrop_tris={} road_tris={} samples={}",
        elapsed,
        chunks,
        terrain_tris,
        fill_tris,
        backdrop_tris,
        road_tris,
        samples.len()
    );
    (samples, field)
}

fn emit_chunk(
    parent: &mut Gd<Node3D>,
    field: &Field,
    theme: &Theme,
    material: &Gd<ShaderMaterial>,
    cx: i32,
    cz: i32,
    textured: bool,
) -> i32 {
    let origin_x = cx as f32 * CHUNK_M;
    let origin_z = cz as f32 * CHUNK_M;
    let center_x = origin_x + CHUNK_M * 0.5;
    let center_z = origin_z + CHUNK_M * 0.5;
    let cell = if field.road(center_x, center_z).0 < NEAR_ROAD_M {
        FINE_M
    } else {
        COARSE_M
    };
    let steps = (CHUNK_M / cell).round() as i32;
    let n = steps + 1;
    let mut heights = vec![0.0_f32; (n * n) as usize];
    for iz in 0..n {
        for ix in 0..n {
            let x = origin_x + ix as f32 * cell;
            let z = origin_z + iz as f32 * cell;
            heights[(iz * n + ix) as usize] = field.height(x, z);
        }
    }
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    for iz in 0..n {
        for ix in 0..n {
            let x = origin_x + ix as f32 * cell;
            let z = origin_z + iz as f32 * cell;
            let h = heights[(iz * n + ix) as usize];
            let slope = grid_slope(&heights, n, ix, iz, cell);
            tool.set_normal(grid_normal(&heights, n, ix, iz, cell));
            tool.set_color(ground_color(
                theme,
                h,
                slope,
                detail_noise(x, z),
                textured,
                field.snow_line,
            ));
            tool.add_vertex(Vector3::new(x, h, z));
        }
    }
    for iz in 0..steps {
        for ix in 0..steps {
            let i00 = iz * n + ix;
            let i10 = i00 + 1;
            let i01 = i00 + n;
            let i11 = i01 + 1;
            tool.add_index(i00);
            tool.add_index(i10);
            tool.add_index(i01);
            tool.add_index(i10);
            tool.add_index(i11);
            tool.add_index(i01);
        }
    }
    let mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut node = MeshInstance3D::new_alloc();
    node.set_name(&format!("Chunk{cx}_{cz}"));
    node.set_mesh(&mesh);
    node.set_surface_override_material(0, material);
    parent.add_child(&node);
    steps * steps * 2
}

fn terrain_material() -> Gd<ShaderMaterial> {
    let mut shader = Shader::new_gd();
    shader.set_code(TERRAIN_SHADER);
    let mut material = ShaderMaterial::new_gd();
    material.set_shader(&shader);
    let _keep_vertex_colors = Flags::ALBEDO_FROM_VERTEX_COLOR;
    material
}

fn ground_color(
    theme: &Theme,
    height: f32,
    slope: f32,
    breakup: f32,
    textured: bool,
    snow_line: f32,
) -> Color {
    let mut weight_sum = 0.0_f32;
    let mut rgb_sum = [0.0_f32; 3];
    let mut weights = [0.0_f32; 4];
    for (index, layer) in theme.ground.iter().enumerate() {
        let altitude_window = if layer.name == "snow" {
            [snow_line, snow_line + 800.0]
        } else {
            layer.altitude
        };
        let altitude = window_weight(height, altitude_window, layer.blend.max(1.0));
        let slope_w = window_weight(slope, layer.slope, layer.blend.max(0.05));
        let mut weight = altitude * slope_w;
        if layer.name == "snow" {
            weight *= smoothstep(0.25, 0.75, breakup * 0.5 + 0.5);
        }
        weight_sum += weight;
        for (channel, sum) in rgb_sum.iter_mut().enumerate() {
            *sum += layer.color[channel] * weight;
        }
        if index < 4 {
            weights[index] = weight;
        }
    }
    if weight_sum < 1.0e-4 {
        return rgb(theme.ground[0].color);
    }
    if textured {
        return Color::from_rgba(
            weights[0] / weight_sum,
            weights[1] / weight_sum,
            weights[2] / weight_sum,
            weights[3] / weight_sum,
        );
    }
    rgb([
        rgb_sum[0] / weight_sum,
        rgb_sum[1] / weight_sum,
        rgb_sum[2] / weight_sum,
    ])
}

fn window_weight(value: f32, window: [f32; 2], blend: f32) -> f32 {
    let enter = smoothstep(window[0] - blend, window[0] + blend * 0.25, value);
    let leave = 1.0 - smoothstep(window[1] - blend * 0.25, window[1] + blend, value);
    enter * leave
}

fn add_range(
    parent: &mut Gd<Node3D>,
    field: &Field,
    theme: &Theme,
    material: &Gd<ShaderMaterial>,
    textured: bool,
) -> i32 {
    let occupied: HashSet<(i32, i32)> = field.corridor.iter().copied().collect();
    let mid = grid_bounds(
        field.span_min_x,
        field.span_max_x,
        field.span_min_z,
        field.span_max_z,
        MID_REACH_M,
        FAR_CELL_M,
    );
    let far = grid_bounds(
        field.span_min_x,
        field.span_max_x,
        field.span_min_z,
        field.span_max_z,
        FAR_REACH_M,
        FAR_CELL_M,
    );
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    let mut tris = 0_i32;
    tris += emit_lod(
        &mut tool, field, theme, textured, mid, MID_CELL_M, &occupied, None,
    );
    tris += emit_lod(
        &mut tool,
        field,
        theme,
        textured,
        far,
        FAR_CELL_M,
        &occupied,
        Some((mid, MID_CELL_M)),
    );
    if tris == 0 {
        return 0;
    }
    let mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut node = MeshInstance3D::new_alloc();
    node.set_name("Range");
    node.set_mesh(&mesh);
    node.set_surface_override_material(0, material);
    parent.add_child(&node);
    tris
}

#[derive(Clone, Copy)]
struct GridBox {
    min_x: f32,
    max_x: f32,
    min_z: f32,
    max_z: f32,
}

fn grid_bounds(
    span_min_x: f32,
    span_max_x: f32,
    span_min_z: f32,
    span_max_z: f32,
    reach: f32,
    snap: f32,
) -> GridBox {
    let snap_down = |value: f32| (value / snap).floor() * snap;
    let snap_up = |value: f32| (value / snap).ceil() * snap;
    GridBox {
        min_x: snap_down(span_min_x - reach),
        max_x: snap_up(span_max_x + reach),
        min_z: snap_down(span_min_z - reach),
        max_z: snap_up(span_max_z + reach),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_lod(
    tool: &mut Gd<SurfaceTool>,
    field: &Field,
    theme: &Theme,
    textured: bool,
    bounds: GridBox,
    cell: f32,
    occupied: &HashSet<(i32, i32)>,
    hole: Option<(GridBox, f32)>,
) -> i32 {
    let mut tris = 0_i32;
    let mut z = bounds.min_z;
    while z < bounds.max_z - 0.1 {
        let mut x = bounds.min_x;
        while x < bounds.max_x - 0.1 {
            let quad = [x, z, x + cell, z + cell];
            let inside_hole = hole.is_some_and(|(inner, _)| {
                quad[0] >= inner.min_x - 0.1
                    && quad[2] <= inner.max_x + 0.1
                    && quad[1] >= inner.min_z - 0.1
                    && quad[3] <= inner.max_z + 0.1
            });
            let center_chunk = chunk_at(x + cell * 0.5, z + cell * 0.5);
            if !inside_hole && !occupied.contains(&center_chunk) {
                tris += push_ground_quad(tool, field, theme, textured, quad, cell, occupied, hole);
            }
            x += cell;
        }
        z += cell;
    }
    tris
}

fn chunk_at(x: f32, z: f32) -> (i32, i32) {
    ((x / CHUNK_M).floor() as i32, (z / CHUNK_M).floor() as i32)
}

#[allow(clippy::too_many_arguments)]
fn push_ground_quad(
    tool: &mut Gd<SurfaceTool>,
    field: &Field,
    theme: &Theme,
    textured: bool,
    quad: [f32; 4],
    cell: f32,
    occupied: &HashSet<(i32, i32)>,
    hole: Option<(GridBox, f32)>,
) -> i32 {
    let x0 = quad[0];
    let z0 = quad[1];
    let x1 = quad[2];
    let z1 = quad[3];
    let edges = [
        (x0, z0, x1, z0),
        (x1, z0, x1, z1),
        (x1, z1, x0, z1),
        (x0, z1, x0, z0),
    ];
    let mut ring: Vec<(f32, f32)> = Vec::new();
    for (index, edge) in edges.iter().enumerate() {
        let step = edge_step(*edge, cell, occupied, hole);
        let length = ((edge.2 - edge.0).powi(2) + (edge.3 - edge.1).powi(2)).sqrt();
        let pieces = ((length / step).round() as i32).max(1);
        let start = if index == 0 { 0 } else { 1 };
        for piece in start..=pieces {
            let t = piece as f32 / pieces as f32;
            ring.push((
                edge.0 + (edge.2 - edge.0) * t,
                edge.1 + (edge.3 - edge.1) * t,
            ));
        }
    }
    if ring.len() < 3 {
        return 0;
    }
    let mut placed = Vec::with_capacity(ring.len());
    for (x, z) in &ring {
        let h = field.height(*x, *z);
        placed.push(Vector3::new(*x, h, *z));
    }
    let mut count = 0_i32;
    for index in 1..placed.len() - 1 {
        let tri = [0, index, index + 1];
        let face = (placed[tri[1]] - placed[tri[0]]).cross(placed[tri[2]] - placed[tri[0]]);
        if face.length_squared() < 1.0e-6 {
            continue;
        }
        let normal = face.normalized();
        let slope = (1.0 - normal.y.abs()).clamp(0.0, 1.0);
        for corner in tri {
            let position = placed[corner];
            tool.set_normal(normal);
            tool.set_color(ground_color(
                theme,
                position.y,
                slope,
                detail_noise(position.x, position.z),
                textured,
                field.snow_line,
            ));
            tool.add_vertex(position);
        }
        count += 1;
    }
    count
}

fn edge_step(
    edge: (f32, f32, f32, f32),
    cell: f32,
    occupied: &HashSet<(i32, i32)>,
    hole: Option<(GridBox, f32)>,
) -> f32 {
    if let Some((inner, step)) = hole
        && on_box_edge(edge, inner)
    {
        return step;
    }
    let mx = (edge.0 + edge.2) * 0.5;
    let mz = (edge.1 + edge.3) * 0.5;
    let (nx, nz) = if (edge.2 - edge.0).abs() >= (edge.3 - edge.1).abs() {
        (0.0, 1.0)
    } else {
        (1.0, 0.0)
    };
    let left = chunk_at(mx - nx, mz - nz);
    let right = chunk_at(mx + nx, mz + nz);
    if occupied.contains(&left) != occupied.contains(&right) {
        COARSE_M
    } else {
        cell
    }
}

fn on_box_edge(edge: (f32, f32, f32, f32), bounds: GridBox) -> bool {
    let vertical = (edge.0 - edge.2).abs() < 0.5;
    let horizontal = (edge.1 - edge.3).abs() < 0.5;
    if vertical {
        let x = edge.0;
        return (x - bounds.min_x).abs() < 0.5 || (x - bounds.max_x).abs() < 0.5;
    }
    if horizontal {
        let z = edge.1;
        return (z - bounds.min_z).abs() < 0.5 || (z - bounds.max_z).abs() < 0.5;
    }
    false
}

pub(crate) fn sample_centerline(course: &Course) -> Vec<CenterSample> {
    let length = course.length_m();
    let mut samples = Vec::new();
    let mut distance = 0.0;
    while distance < length {
        samples.push(sample_at(course, distance));
        let here = course.tangent_at(distance);
        let next = course.tangent_at(distance + 2.0);
        let dot = here[0] * next[0] + here[1] * next[1] + here[2] * next[2];
        let step = if dot < 0.985 { 1.0 } else { 2.0 };
        distance += step;
    }
    samples.push(sample_at(course, 0.0));
    samples
}

fn sample_at(course: &Course, distance: f64) -> CenterSample {
    let position = course.position_at(distance);
    let tangent = course.tangent_at(distance);
    CenterSample {
        position: Vector3::new(position[0], position[1] + 0.08, position[2]),
        tangent: Vector3::new(tangent[0], tangent[1], tangent[2]),
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

fn detail_noise(x: f32, z: f32) -> f32 {
    let ix = (x * 0.17).floor();
    let iz = (z * 0.17).floor();
    let fx = (x * 0.17).fract();
    let fz = (z * 0.17).fract();
    let h = |a: f32, b: f32| {
        let n = a.to_bits().wrapping_mul(374761393) ^ b.to_bits().wrapping_mul(668265263);
        (n >> 8) as f32 / 16_777_216.0
    };
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uz = fz * fz * (3.0 - 2.0 * fz);
    let a = h(ix, iz);
    let b = h(ix + 1.0, iz);
    let c = h(ix, iz + 1.0);
    let d = h(ix + 1.0, iz + 1.0);
    (a + (b - a) * ux + (c - a) * uz + (a - b - c + d) * ux * uz) * 2.0 - 1.0
}

fn grid_slope(heights: &[f32], n: i32, ix: i32, iz: i32, cell: f32) -> f32 {
    let h = |x: i32, z: i32| heights[(z.clamp(0, n - 1) * n + x.clamp(0, n - 1)) as usize];
    let dx = (h(ix + 1, iz) - h(ix - 1, iz)) / (2.0 * cell);
    let dz = (h(ix, iz + 1) - h(ix, iz - 1)) / (2.0 * cell);
    (dx * dx + dz * dz).sqrt()
}

fn grid_normal(heights: &[f32], n: i32, ix: i32, iz: i32, cell: f32) -> Vector3 {
    let h = |x: i32, z: i32| heights[(z.clamp(0, n - 1) * n + x.clamp(0, n - 1)) as usize];
    let dx = h(ix + 1, iz) - h(ix - 1, iz);
    let dz = h(ix, iz + 1) - h(ix, iz - 1);
    Vector3::new(-dx, 2.0 * cell, -dz).normalized()
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
