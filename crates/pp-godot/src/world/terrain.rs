//! Chunked heightfield. Near-road chunks are finer than the hills behind them.
//!
//! Each chunk is a function of its coordinates, the course samples, and the
//! theme seed. Chunks do not read each other's meshes.

use godot::classes::base_material_3d::Flags;
use godot::classes::fast_noise_lite::{FractalType, NoiseType};
use godot::classes::mesh::PrimitiveType;
use godot::classes::{
    ArrayMesh, FastNoiseLite, MeshInstance3D, Shader, ShaderMaterial, StandardMaterial3D,
    SurfaceTool,
};
use godot::obj::Gd;
use godot::prelude::*;
use pp_course::Course;

use crate::assets::Catalog;
use crate::road::CenterSample;
use crate::theme::{Theme, rgb};

const CHUNK_M: f32 = 128.0;
const FINE_M: f32 = 4.0;
const COARSE_M: f32 = 16.0;
const NEAR_ROAD_M: f32 = 80.0;
const PLAY_M: f32 = 1_100.0;

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
    noise: Gd<FastNoiseLite>,
}

impl Field {
    pub(crate) fn new(samples: &[[f32; 3]], seed: i32) -> Self {
        let (min_x, max_x, min_z, max_z) = bounds(samples);
        let cell = 8.0_f32;
        let origin_x = min_x - 160.0;
        let origin_z = min_z - 160.0;
        let nx = ((max_x - origin_x + 160.0) / cell).ceil() as i32 + 1;
        let nz = ((max_z - origin_z + 160.0) / cell).ceil() as i32 + 1;
        let mut dist = vec![1.0e9_f32; (nx * nz) as usize];
        let mut road_y = vec![0.0_f32; (nx * nz) as usize];
        let reach = (120.0 / cell).ceil() as i32;
        for sample in samples {
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
                }
            }
        }
        let mut noise = FastNoiseLite::new_gd();
        noise.set_seed(seed);
        noise.set_noise_type(NoiseType::SIMPLEX_SMOOTH);
        noise.set_fractal_type(FractalType::FBM);
        noise.set_fractal_octaves(4);
        noise.set_frequency(0.00048);
        Self {
            origin_x,
            origin_z,
            cell,
            nx,
            nz,
            dist,
            road_y,
            noise,
        }
    }

    /// Horizontal distance to the centreline and that sample's height.
    pub fn road(&self, x: f32, z: f32) -> (f32, f32) {
        let ix = ((x - self.origin_x) / self.cell).round() as i32;
        let iz = ((z - self.origin_z) / self.cell).round() as i32;
        if ix < 0 || iz < 0 || ix >= self.nx || iz >= self.nz {
            return (1.0e9, 0.0);
        }
        let index = (iz * self.nx + ix) as usize;
        (self.dist[index], self.road_y[index])
    }

    /// Terrain height at a world position.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let (distance, road_y) = self.road(x, z);
        let mountains = self.noise.get_noise_2d(x, z) * 220.0 + detail_noise(x, z) * 10.0;
        let free = 35.0 + mountains;
        conform(distance, road_y, free)
    }
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
    let field = Field::new(&raw, theme.seed as i32);
    let mut material = terrain_material();
    let textured = catalog.bind_ground(&mut material, theme);
    let mut group = Node3D::new_alloc();
    group.set_name("Terrain");
    let (min_x, max_x, min_z, max_z) = bounds(&raw);
    let mut cx = (min_x / CHUNK_M).floor() as i32;
    let x1 = (max_x / CHUNK_M).ceil() as i32;
    let z0 = (min_z / CHUNK_M).floor() as i32;
    let z1 = (max_z / CHUNK_M).ceil() as i32;
    while cx <= x1 {
        let mut cz = z0;
        while cz <= z1 {
            emit_chunk(&mut group, &field, theme, &material, cx, cz, textured);
            cz += 1;
        }
        cx += 1;
    }
    add_backdrop(&mut group, theme, &samples);
    parent.add_child(&group);
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
) {
    let origin_x = cx as f32 * CHUNK_M;
    let origin_z = cz as f32 * CHUNK_M;
    let center_x = origin_x + CHUNK_M * 0.5;
    let center_z = origin_z + CHUNK_M * 0.5;
    if center_x.abs().max(center_z.abs()) > PLAY_M && field.road(center_x, center_z).0 > NEAR_ROAD_M
    {
        return;
    }
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
            tool.set_color(ground_color(theme, h, slope, detail_noise(x, z), textured));
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
}

fn terrain_material() -> Gd<ShaderMaterial> {
    let mut shader = Shader::new_gd();
    shader.set_code(TERRAIN_SHADER);
    let mut material = ShaderMaterial::new_gd();
    material.set_shader(&shader);
    let _keep_vertex_colors = Flags::ALBEDO_FROM_VERTEX_COLOR;
    material
}

fn conform(distance: f32, road_y: f32, free: f32) -> f32 {
    if distance <= 5.2 {
        return road_y - 0.4;
    }
    let allowance = (distance - 5.2) * 0.55;
    let capped = free.clamp(road_y - allowance, road_y + allowance);
    let t = smoothstep(5.2, 42.0, distance);
    (road_y - 0.4) + (capped - (road_y - 0.4)) * t
}

fn ground_color(theme: &Theme, height: f32, slope: f32, breakup: f32, textured: bool) -> Color {
    let mut weight_sum = 0.0_f32;
    let mut rgb_sum = [0.0_f32; 3];
    let mut weights = [0.0_f32; 4];
    for (index, layer) in theme.ground.iter().enumerate() {
        let altitude = window_weight(height, layer.altitude, layer.blend.max(1.0));
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

fn add_backdrop(parent: &mut Gd<Node3D>, theme: &Theme, samples: &[CenterSample]) {
    let reach = samples
        .iter()
        .map(|sample| {
            let position = sample.position;
            (position.x * position.x + position.z * position.z).sqrt()
        })
        .fold(0.0_f32, f32::max);
    let tris = pp_view::backdrop::build(&theme.backdrop, reach, theme.seed);
    if tris.is_empty() {
        return;
    }
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    for tri in &tris {
        let corners: Vec<Vector3> = tri
            .v
            .iter()
            .map(|vert| Vector3::new(vert.x, vert.y, vert.z))
            .collect();
        let mut normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        if normal.length_squared() < 1.0e-8 {
            continue;
        }
        normal = normal.normalized();
        let mid = (corners[0] + corners[1] + corners[2]) / 3.0;
        if normal.dot(-mid) < 0.0 {
            normal = -normal;
        }
        for (corner, vert) in corners.iter().zip(tri.v.iter()) {
            tool.set_normal(normal);
            tool.set_color(Color::from_rgb(vert.r, vert.g, vert.b));
            tool.add_vertex(*corner);
        }
    }
    let mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut material = StandardMaterial3D::new_gd();
    material.set_flag(Flags::ALBEDO_FROM_VERTEX_COLOR, true);
    material.set_roughness(1.0);
    material.set_specular(0.0);
    let mut node = MeshInstance3D::new_alloc();
    node.set_name("Backdrop");
    node.set_mesh(&mesh);
    node.set_surface_override_material(0, &material);
    node.set_cast_shadows_setting(godot::classes::geometry_instance_3d::ShadowCastingSetting::OFF);
    parent.add_child(&node);
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

fn bounds(samples: &[[f32; 3]]) -> (f32, f32, f32, f32) {
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
    (min_x - 180.0, max_x + 180.0, min_z - 180.0, max_z + 180.0)
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
