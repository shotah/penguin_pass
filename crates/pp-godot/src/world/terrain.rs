//! Chunked heightfield. Near-road chunks are finer than the hills behind them.
//!
//! Each chunk is a function of its coordinates, the course samples, and the
//! theme seed. Chunks do not read each other's meshes.

use crate::assets::Catalog;
use crate::road::CenterSample;
use crate::theme::{Theme, rgb};
use godot::classes::base_material_3d::Flags;
use godot::classes::mesh::PrimitiveType;
use godot::classes::{ArrayMesh, MeshInstance3D, Shader, ShaderMaterial, SurfaceTool};
use godot::obj::Gd;
use godot::prelude::*;
use pp_course::Course;
pub use pp_world::Field;
use pp_world::{PlannedChunk, PlannedFar, SKIRT_M, Skirt, plan};

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
uniform float farmland = 0.0;
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
    if (farmland > 0.5) {
        float cell_x = 260.0;
        float cell_z = 340.0;
        vec2 xz = world_pos.xz;
        float col = floor((xz.x + 17.3) / cell_x);
        float row = floor((xz.y + 23.7) / cell_z);
        float off_x = hash(vec2(row, 19.0)) * 14.0;
        float off_z = hash(vec2(col, 31.0)) * 14.0;
        vec2 cell = vec2(xz.x - col * cell_x - off_x, xz.y - row * cell_z - off_z);
        float pid = floor(hash(vec2(col, row)) * 4.0);
        vec3 plot = vec3(0.74, 0.58, 0.24);
        if (pid > 0.5) plot = vec3(0.68, 0.59, 0.36);
        if (pid > 1.5) plot = vec3(0.36, 0.27, 0.18);
        if (pid > 2.5) plot = vec3(0.35, 0.48, 0.21);
        float ns = mod(col + row, 2.0);
        float coord = ns < 1.0 ? cell.x : cell.y;
        plot += sin(coord * 0.55) * 0.045;
        float edge = min(min(cell.x, cell_x - cell.x), min(cell.y, cell_z - cell.y));
        float border = smoothstep(3.0, 0.8, edge);
        plot = mix(plot * 0.82, plot, border);
        vec3 grain = mix(vec3(1.0), sampled, textured * 0.65);
        vec3 patch = plot * grain * tint;
        ALBEDO = mix(ALBEDO, patch, 0.70);
    }
    float away = smoothstep(haze_start, haze_end, length(VERTEX));
    ALBEDO = mix(ALBEDO, haze_color, away);
    ROUGHNESS = mix(0.94, 0.78, w.a * textured);
}
"#;

/// Build chunk meshes and the query field the scatter and rails share.
pub fn build(
    parent: &mut Gd<Node3D>,
    course: &Course,
    theme: &Theme,
    catalog: &Catalog,
) -> (Vec<CenterSample>, Field, Vec<pp_world::PlacedLandmark>) {
    let samples = sample_centerline(course);
    let raw: Vec<[f32; 3]> = samples
        .iter()
        .map(|sample| [sample.position.x, sample.position.y, sample.position.z])
        .collect();
    let started = godot::classes::Time::singleton().get_ticks_msec();
    let mut field = Field::new(&raw, theme.seed as i32);
    field.use_relief(theme.relief.clone(), theme.seed);
    let placed = crate::world::landmarks::prepare(course, &mut field, theme);
    let plan = plan(&field);
    let mut material = terrain_material();
    let haze = Vector3::new(theme.fog.color[0], theme.fog.color[1], theme.fog.color[2]);
    material.set_shader_parameter("haze_color", &haze.to_variant());
    let radius = field.haze_radius();
    material.set_shader_parameter("haze_start", &(radius * 0.45).to_variant());
    material.set_shader_parameter("haze_end", &(radius * 0.80).to_variant());
    let textured = catalog.bind_ground(&mut material, theme);
    material.set_shader_parameter(
        "farmland",
        &(if theme.name == "farmland" {
            1.0_f32
        } else {
            0.0_f32
        })
        .to_variant(),
    );
    let mut group = Node3D::new_alloc();
    group.set_name("Terrain");
    let mut chunks = 0_i32;
    let mut terrain_tris = 0_i32;
    for chunk in &plan.chunks {
        terrain_tris += emit_chunk(&mut group, &field, theme, &material, chunk, textured);
        chunks += 1;
    }
    let backdrop_tris = emit_far(&mut group, &field, theme, &material, &plan.far, textured);
    if theme.name != "farmland" {
        emit_horizon(&mut group, course, &field, theme, &material);
    }
    let fill_tris = 0;
    parent.add_child(&group);
    crate::world::landmarks::spawn(parent, course, &field, theme, &placed);
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
    (samples, field, placed)
}

fn emit_chunk(
    parent: &mut Gd<Node3D>,
    field: &Field,
    theme: &Theme,
    material: &Gd<ShaderMaterial>,
    chunk: &PlannedChunk,
    textured: bool,
) -> i32 {
    let n = chunk.steps + 1;
    let origin_x = chunk.cx as f32 * 128.0;
    let origin_z = chunk.cz as f32 * 128.0;
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    for iz in 0..n {
        for ix in 0..n {
            let x = origin_x + ix as f32 * chunk.cell;
            let z = origin_z + iz as f32 * chunk.cell;
            let h = chunk.heights[(iz * n + ix) as usize];
            let slope = grid_slope(&chunk.heights, n, ix, iz, chunk.cell);
            tool.set_normal(grid_normal(&chunk.heights, n, ix, iz, chunk.cell));
            tool.set_color(ground_color(
                theme,
                h,
                slope,
                detail_noise(x, z),
                textured,
                field.snow_line(),
            ));
            tool.add_vertex(Vector3::new(x, h, z));
        }
    }
    for iz in 0..chunk.steps {
        for ix in 0..chunk.steps {
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
    let mut cursor = n * n;
    for skirt in &chunk.skirts {
        add_skirt(&mut tool, theme, textured, skirt, field.snow_line(), cursor);
        cursor += 4;
    }
    let mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut node = MeshInstance3D::new_alloc();
    node.set_name(&format!("Chunk{}_{}", chunk.cx, chunk.cz));
    node.set_mesh(&mesh);
    node.set_surface_override_material(0, material);
    parent.add_child(&node);
    chunk.steps * chunk.steps * 2 + chunk.skirts.len() as i32 * 4
}

fn emit_far(
    parent: &mut Gd<Node3D>,
    field: &Field,
    theme: &Theme,
    material: &Gd<ShaderMaterial>,
    far: &PlannedFar,
    textured: bool,
) -> i32 {
    if far.triangles.is_empty() {
        return 0;
    }
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    for vertex in &far.vertices {
        let (normal, slope) = slope_at(field, vertex.x, vertex.z);
        tool.set_normal(normal);
        tool.set_color(ground_color(
            theme,
            vertex.y,
            slope,
            detail_noise(vertex.x, vertex.z),
            textured,
            field.snow_line(),
        ));
        tool.add_vertex(Vector3::new(vertex.x, vertex.y, vertex.z));
    }
    for tri in &far.triangles {
        tool.add_index(tri[0] as i32);
        tool.add_index(tri[1] as i32);
        tool.add_index(tri[2] as i32);
    }
    let mut cursor = far.vertices.len() as i32;
    for skirt in &far.skirts {
        add_skirt(&mut tool, theme, textured, skirt, field.snow_line(), cursor);
        cursor += 4;
    }
    let mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut node = MeshInstance3D::new_alloc();
    node.set_name("Far");
    node.set_mesh(&mesh);
    node.set_surface_override_material(0, material);
    parent.add_child(&node);
    far.triangles.len() as i32 + far.skirts.len() as i32 * 4
}

fn slope_at(field: &Field, x: f32, z: f32) -> (Vector3, f32) {
    let step = 16.0;
    let dx = field.height(x + step, z) - field.height(x - step, z);
    let dz = field.height(x, z + step) - field.height(x, z - step);
    let normal = Vector3::new(-dx, 2.0 * step, -dz).normalized();
    let slope = (dx * dx + dz * dz).sqrt() / (2.0 * step);
    (normal, slope)
}

fn add_skirt(
    tool: &mut Gd<SurfaceTool>,
    theme: &Theme,
    textured: bool,
    skirt: &Skirt,
    snow_line: f32,
    base: i32,
) {
    let positions = [
        Vector3::new(skirt.x0, skirt.y0, skirt.z0),
        Vector3::new(skirt.x1, skirt.y1, skirt.z1),
        Vector3::new(skirt.x1, skirt.y1 - SKIRT_M, skirt.z1),
        Vector3::new(skirt.x0, skirt.y0 - SKIRT_M, skirt.z0),
    ];
    let colors = [
        ground_color(
            theme,
            skirt.y0,
            0.2,
            detail_noise(skirt.x0, skirt.z0),
            textured,
            snow_line,
        ),
        ground_color(
            theme,
            skirt.y1,
            0.2,
            detail_noise(skirt.x1, skirt.z1),
            textured,
            snow_line,
        ),
        ground_color(
            theme,
            skirt.y1,
            0.2,
            detail_noise(skirt.x1, skirt.z1),
            textured,
            snow_line,
        ),
        ground_color(
            theme,
            skirt.y0,
            0.2,
            detail_noise(skirt.x0, skirt.z0),
            textured,
            snow_line,
        ),
    ];
    let edge = positions[1] - positions[0];
    let down = positions[3] - positions[0];
    let normal = edge.cross(down).normalized();
    for (position, color) in positions.iter().zip(colors) {
        tool.set_normal(normal);
        tool.set_color(color);
        tool.add_vertex(*position);
    }
    let b = base;
    tool.add_index(b);
    tool.add_index(b + 1);
    tool.add_index(b + 2);
    tool.add_index(b);
    tool.add_index(b + 2);
    tool.add_index(b + 3);
    tool.add_index(b);
    tool.add_index(b + 2);
    tool.add_index(b + 1);
    tool.add_index(b);
    tool.add_index(b + 3);
    tool.add_index(b + 2);
}

fn emit_horizon(
    parent: &mut Gd<Node3D>,
    course: &Course,
    field: &Field,
    theme: &Theme,
    shared: &Gd<ShaderMaterial>,
) {
    let (anchor_x, anchor_z) = pp_world::horizon_in_view(course);
    let desert = theme.name == "desert";
    let peak_h = if desert { 620.0 } else { 1_250.0 };
    let base_r = if desert { 1_600.0 } else { 720.0 };
    let rings = 26_i32;
    let sectors = 40_i32;
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    let mut grid: Vec<Vector3> = Vec::with_capacity(((rings + 1) * (sectors + 1)) as usize);
    for ir in 0..=rings {
        let radial = ir as f32 / rings as f32;
        for ia in 0..=sectors {
            let angle = ia as f32 / sectors as f32 * std::f32::consts::TAU;
            let shoulder = (angle * 3.0).sin() * 0.16 + (angle * 5.0).cos() * 0.07;
            let noise = peak_hash(angle, radial) * 0.08;
            let (r, h) = if desert {
                // Flat sandstone top, then a steep rim. No snow.
                let drop = smoothstep(0.64, 0.90, radial);
                (
                    base_r * radial * (1.0 + shoulder * 0.08),
                    peak_h * (1.0 - drop),
                )
            } else {
                let cone = (1.0 - radial).powf(1.28);
                (
                    base_r * radial.powf(0.82) * (1.0 + shoulder * (0.35 + radial)),
                    peak_h * cone * (1.0 + noise + shoulder * 0.12 * radial),
                )
            };
            let x = anchor_x + angle.cos() * r;
            let z = anchor_z + angle.sin() * r;
            let ground = field.height(x, z);
            let skirt = smoothstep(0.82, 1.0, radial);
            let y = ground + h * (1.0 - skirt);
            grid.push(Vector3::new(x, y.max(ground), z));
        }
    }
    let cols = sectors + 1;
    for ir in 0..rings {
        for ia in 0..sectors {
            let i00 = (ir * cols + ia) as usize;
            let i10 = i00 + 1;
            let i01 = i00 + cols as usize;
            let i11 = i01 + 1;
            let height = grid[i00].y - field.height(grid[i00].x, grid[i00].z);
            let color = if desert {
                let band = ((height / peak_h) * 6.0).floor() as i32;
                if band % 2 == 0 {
                    Color::from_rgb(0.40, 0.28, 0.18)
                } else {
                    Color::from_rgb(0.62, 0.45, 0.28)
                }
            } else {
                let snow = smoothstep(peak_h * 0.58, peak_h * 0.78, height);
                let rock = Color::from_rgb(0.40, 0.36, 0.32);
                let cap = Color::from_rgb(0.94, 0.96, 0.98);
                rock.lerp(cap, f64::from(snow))
            };
            for (a, b, c) in [(i00, i11, i10), (i00, i01, i11)] {
                for index in [a, b, c] {
                    tool.set_color(color);
                    tool.add_vertex(grid[index]);
                }
            }
        }
    }
    tool.generate_normals();
    let mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut material = ShaderMaterial::new_gd();
    let code = TERRAIN_SHADER.replacen(
        "render_mode diffuse_burley, specular_disabled;",
        "render_mode diffuse_burley, specular_disabled, cull_disabled;",
        1,
    );
    let mut shader = Shader::new_gd();
    shader.set_code(&code);
    material.set_shader(&shader);
    copy_haze(&mut material, shared);
    material.set_shader_parameter("textured", &0.0_f32.to_variant());
    let mut node = MeshInstance3D::new_alloc();
    node.set_name("HorizonPeak");
    node.set_mesh(&mesh);
    node.set_surface_override_material(0, &material);
    parent.add_child(&node);
    if desert {
        emit_volcano(parent, course, field, &material);
    }
}

fn emit_volcano(
    parent: &mut Gd<Node3D>,
    course: &Course,
    field: &Field,
    shared: &Gd<ShaderMaterial>,
) {
    let at = course.position_at(40.0);
    let tangent = course.tangent_at(40.0);
    let tlen = tangent[0].hypot(tangent[2]).max(0.001);
    let tx = tangent[0] / tlen;
    let tz = tangent[2] / tlen;
    let rx = -tz;
    let rz = tx;
    let range = 4_600.0;
    let x0 = at[0] + tx * range + rx * 700.0;
    let z0 = at[2] + tz * range + rz * 700.0;
    let peak_h = 980.0;
    let base_r = 780.0;
    let rings = 22_i32;
    let sectors = 32_i32;
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    let mut grid = Vec::new();
    for ir in 0..=rings {
        let radial = ir as f32 / rings as f32;
        for ia in 0..=sectors {
            let angle = ia as f32 / sectors as f32 * std::f32::consts::TAU;
            let crater = if radial < 0.14 {
                (1.0 - radial / 0.14) * 0.16
            } else {
                0.0
            };
            let cone = (1.0 - radial).powf(1.15);
            let r = base_r * radial.powf(0.9);
            let h = peak_h * (cone - crater).max(0.0);
            let x = x0 + angle.cos() * r;
            let z = z0 + angle.sin() * r;
            let ground = field.height(x, z);
            let skirt = smoothstep(0.84, 1.0, radial);
            grid.push(Vector3::new(x, ground + h * (1.0 - skirt), z));
        }
    }
    let cols = sectors + 1;
    for ir in 0..rings {
        for ia in 0..sectors {
            let i00 = (ir * cols + ia) as usize;
            let i10 = i00 + 1;
            let i01 = i00 + cols as usize;
            let i11 = i01 + 1;
            let height = grid[i00].y - field.height(grid[i00].x, grid[i00].z);
            let hot = smoothstep(peak_h * 0.72, peak_h * 0.84, height);
            let rock = Color::from_rgb(0.22, 0.16, 0.13);
            let rim = Color::from_rgb(0.45, 0.22, 0.12);
            let color = rock.lerp(rim, f64::from(hot));
            for index in [i00, i11, i10, i00, i01, i11] {
                tool.set_color(color);
                tool.add_vertex(grid[index]);
            }
        }
    }
    tool.generate_normals();
    let mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut material = ShaderMaterial::new_gd();
    let mut shader = Shader::new_gd();
    shader.set_code(&TERRAIN_SHADER.replacen(
        "render_mode diffuse_burley, specular_disabled;",
        "render_mode diffuse_burley, specular_disabled, cull_disabled;",
        1,
    ));
    material.set_shader(&shader);
    copy_haze(&mut material, shared);
    material.set_shader_parameter("textured", &0.0_f32.to_variant());
    let mut node = MeshInstance3D::new_alloc();
    node.set_name("Volcano");
    node.set_mesh(&mesh);
    node.set_surface_override_material(0, &material);
    parent.add_child(&node);
    let ground = field.height(x0, z0);
    let mut plume = MeshInstance3D::new_alloc();
    plume.set_name("Plume");
    plume.set_position(Vector3::new(x0, ground + peak_h * 0.82, z0));
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    let width = 80.0;
    let tall = 420.0;
    for (a, b, c) in [
        (
            Vector3::new(-width, 0.0, 0.0),
            Vector3::new(width, tall, 40.0),
            Vector3::new(width, 0.0, 0.0),
        ),
        (
            Vector3::new(-width, 0.0, 0.0),
            Vector3::new(-width, tall, 40.0),
            Vector3::new(width, tall, 40.0),
        ),
    ] {
        tool.set_uv(Vector2::new(0.0, 1.0));
        tool.add_vertex(a);
        tool.set_uv(Vector2::new(1.0, 0.0));
        tool.add_vertex(b);
        tool.set_uv(Vector2::new(1.0, 1.0));
        tool.add_vertex(c);
    }
    let sheet = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    plume.set_mesh(&sheet);
    let mut smoke = ShaderMaterial::new_gd();
    let mut smoke_shader = Shader::new_gd();
    smoke_shader.set_code(
        r#"shader_type spatial;
render_mode cull_disabled, blend_mix, depth_draw_opaque, unshaded;
void fragment() {
    float band = fract(UV.y - TIME * 0.08);
    float puff = smoothstep(0.15, 0.7, band);
    ALBEDO = mix(vec3(0.18, 0.16, 0.15), vec3(0.45, 0.40, 0.36), puff);
    ALPHA = (1.0 - UV.y) * 0.45;
}"#,
    );
    smoke.set_shader(&smoke_shader);
    plume.set_surface_override_material(0, &smoke);
    parent.add_child(&plume);
}

fn copy_haze(into: &mut Gd<ShaderMaterial>, from: &Gd<ShaderMaterial>) {
    for name in ["haze_color", "haze_start", "haze_end"] {
        into.set_shader_parameter(name, &from.get_shader_parameter(name));
    }
}

fn peak_hash(angle: f32, radial: f32) -> f32 {
    let n =
        angle.to_bits().wrapping_mul(1_597_334_677) ^ radial.to_bits().wrapping_mul(3_812_015_801);
    (n >> 8) as f32 / 16_777_216.0 * 2.0 - 1.0
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
