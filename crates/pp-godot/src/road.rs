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

const ROAD_SHADER: &str = r#"shader_type spatial;
render_mode cull_disabled, diffuse_burley, specular_schlick_ggx;

void fragment() {
    float u = UV.x;
    float along = UV.y;
    vec3 asphalt = vec3(0.15, 0.15, 0.16);
    float grain = fract(sin(dot(floor(vec2(u * 48.0, along * 0.85)), vec2(127.1, 311.7))) * 43758.5453);
    asphalt = mix(asphalt, vec3(0.20, 0.20, 0.21), grain * 0.4);
    vec3 paint = vec3(0.93, 0.91, 0.84);
    float edge = step(u, 0.026) + step(0.974, u);
    float dash = step(fract(along / 8.0), 0.42);
    float center = 1.0 - step(0.016, abs(u - 0.5));
    float mark = clamp(edge + dash * center, 0.0, 1.0);
    ALBEDO = mix(asphalt, paint, mark);
    ROUGHNESS = mix(0.95, 0.55, mark);
    METALLIC = 0.0;
}
"#;

/// Asphalt shader with a dashed center line and solid edges.
///
/// `UV.x` runs across the road (0 left, 1 right). `UV.y` is distance in meters.
pub fn road_material() -> Gd<ShaderMaterial> {
    let mut shader = Shader::new_gd();
    shader.set_code(ROAD_SHADER);
    let mut material = ShaderMaterial::new_gd();
    material.set_shader(&shader);
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
    for index in 0..samples.len() - 1 {
        let next = index + 1;
        push_vertex(
            &mut tool,
            left[index],
            normal[index],
            Vector2::new(0.0, distance[index]),
        );
        push_vertex(
            &mut tool,
            right[index],
            normal[index],
            Vector2::new(1.0, distance[index]),
        );
        push_vertex(
            &mut tool,
            left[next],
            normal[next],
            Vector2::new(0.0, distance[next]),
        );
        push_vertex(
            &mut tool,
            left[next],
            normal[next],
            Vector2::new(0.0, distance[next]),
        );
        push_vertex(
            &mut tool,
            right[index],
            normal[index],
            Vector2::new(1.0, distance[index]),
        );
        push_vertex(
            &mut tool,
            right[next],
            normal[next],
            Vector2::new(1.0, distance[next]),
        );
    }
    tool.commit().unwrap_or_else(ArrayMesh::new_gd)
}

/// Straight centerline along -Z (Godot forward), raised by `y` so it clears the ground.
pub fn straight_road(length_m: f32, y: f32, step_m: f32) -> Vec<CenterSample> {
    let step = step_m.max(0.5);
    let mut samples = Vec::new();
    // A little road behind the rider, then `length_m` ahead.
    let mut z = 30.0_f32;
    let end = -length_m;
    while z >= end {
        samples.push(CenterSample {
            position: Vector3::new(0.0, y, z),
            tangent: Vector3::FORWARD,
        });
        z -= step;
    }
    samples
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
