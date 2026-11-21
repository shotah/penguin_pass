//! Flat road ribbon. Phase 2 feeds it the course centerline; phase 1 uses a straight line.

use godot::classes::mesh::PrimitiveType;
use godot::classes::{ArrayMesh, Shader, ShaderMaterial, SurfaceTool};
use godot::obj::Gd;
use godot::prelude::*;

/// One centerline sample. `tangent` need not be unit length.
pub struct CenterSample {
    /// Center of the road, in meters.
    pub position: Vector3,
    /// Direction of travel.
    pub tangent: Vector3,
}

/// Full ribbon width in meters: 6 m of asphalt plus shoulders.
pub const RIBBON_WIDTH_M: f32 = 9.0;

const ROAD_SHADER: &str = r#"shader_type spatial;
render_mode diffuse_burley, specular_schlick_ggx;

float hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

uniform sampler2D asphalt_tex : source_color, repeat_enable, filter_linear_mipmap;
uniform sampler2D gravel_tex : source_color, repeat_enable, filter_linear_mipmap;
uniform float asphalt_tile = 4.0;
uniform float gravel_tile = 3.0;
uniform float textured = 0.0;

void fragment() {
    float u = UV.x;
    float along = UV.y;
    float from_center = abs(u - 0.5);
    // 9 m ribbon. Asphalt is the middle 6 m (from_center < 0.333).
    float shoulder = smoothstep(0.30, 0.36, from_center);
    float verge = smoothstep(0.44, 0.50, from_center);
    float grain = hash(floor(vec2(u * 70.0, along * 1.4)));
    float streak = hash(vec2(floor(along * 0.35), floor(u * 12.0)));
    // Albedo is this dark so the ribbon still reads as the climb-chase
    // asphalt once the front face takes the sun. The old values were tuned
    // for a back face that only saw the sky.
    vec3 asphalt = mix(vec3(0.15, 0.15, 0.16), vec3(0.20, 0.195, 0.18), grain);
    asphalt = mix(asphalt, vec3(0.11, 0.11, 0.12), streak * 0.35);
    vec3 gravel = mix(vec3(0.32, 0.27, 0.21), vec3(0.40, 0.34, 0.26), grain);
    vec2 across = vec2((u - 0.5) * 9.0, along);
    vec3 photo_asphalt = texture(asphalt_tex, across / asphalt_tile).rgb * vec3(0.42, 0.42, 0.44);
    vec3 photo_gravel = texture(gravel_tex, across / gravel_tile).rgb * vec3(0.72, 0.68, 0.62);
    asphalt = mix(asphalt, photo_asphalt, textured);
    gravel = mix(gravel, photo_gravel, textured);
    vec3 grass = vec3(0.28, 0.40, 0.20);
    vec3 surface = mix(asphalt, gravel, shoulder);
    surface = mix(surface, grass, verge);
    vec3 paint = vec3(0.90, 0.90, 0.86);
    float edge = step(0.30, from_center) * step(from_center, 0.325);
    float dash = step(fract(along / 6.0), 0.45);
    float center = step(from_center, 0.008);
    float mark = clamp(edge + dash * center, 0.0, 1.0) * (1.0 - verge);
    ALBEDO = mix(surface, paint, mark);
    ROUGHNESS = mix(0.88, 0.96, shoulder);
}
"#;

/// Asphalt shader with a dashed center line and solid edges.
///
/// `UV.x` runs across the road (0 left, 1 right). `UV.y` is distance in meters.
pub fn road_material(catalog: &crate::assets::Catalog) -> Gd<ShaderMaterial> {
    let mut shader = Shader::new_gd();
    shader.set_code(ROAD_SHADER);
    let mut material = ShaderMaterial::new_gd();
    material.set_shader(&shader);
    catalog.bind_road(&mut material);
    material
}

/// Build a ribbon `width_m` wide. The surface stays flat across its width (no banking).
pub fn build_ribbon(samples: &[CenterSample], width_m: f32) -> Gd<ArrayMesh> {
    if samples.len() < 2 {
        return ArrayMesh::new_gd();
    }
    let half = width_m * 0.5;
    let mut left = Vec::with_capacity(samples.len());
    let mut right = Vec::with_capacity(samples.len());
    let mut normal = Vec::with_capacity(samples.len());
    let mut distance = Vec::with_capacity(samples.len());
    let mut travelled = 0.0_f32;
    for (index, sample) in samples.iter().enumerate() {
        if index > 0 {
            travelled += samples[index - 1].position.distance_to(sample.position);
        }
        let tangent = unit_or(sample.tangent, Vector3::FORWARD);
        let side = flat_right(tangent);
        let mut up = side.cross(tangent);
        if up.length_squared() < 1.0e-8 {
            up = Vector3::UP;
        } else {
            up = up.normalized();
            if up.y < 0.0 {
                up = -up;
            }
        }
        // `side` points to the rider's right. Left is the minus side.
        left.push(sample.position - side * half);
        right.push(sample.position + side * half);
        normal.push(up);
        distance.push(travelled);
    }

    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    // Godot's front face is clockwise. Seen from above with the road running
    // away, left-near, left-far, right-near is clockwise, so the top of the
    // ribbon is the lit side and takes shadows.
    for index in 0..samples.len() - 1 {
        let next = index + 1;
        let corners = [
            (left[index], index, 0.0),
            (left[next], next, 0.0),
            (right[index], index, 1.0),
            (left[next], next, 0.0),
            (right[next], next, 1.0),
            (right[index], index, 1.0),
        ];
        for (position, at, u) in corners {
            push_vertex(
                &mut tool,
                position,
                normal[at],
                Vector2::new(u, distance[at]),
            );
        }
    }
    tool.commit().unwrap_or_else(ArrayMesh::new_gd)
}

fn push_vertex(tool: &mut Gd<SurfaceTool>, position: Vector3, normal: Vector3, uv: Vector2) {
    tool.set_normal(normal);
    tool.set_uv(uv);
    tool.add_vertex(position);
}

fn unit_or(vector: Vector3, fallback: Vector3) -> Vector3 {
    if vector.length_squared() < 1.0e-8 {
        fallback
    } else {
        vector.normalized()
    }
}

/// Horizontal perpendicular to `tangent`, pointing to the rider's right.
fn flat_right(tangent: Vector3) -> Vector3 {
    let flat = Vector3::new(tangent.x, 0.0, tangent.z);
    let forward = unit_or(flat, Vector3::FORWARD);
    // forward × up, with up = +Y.
    Vector3::new(-forward.z, 0.0, forward.x)
}
