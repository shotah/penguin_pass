//! Sky, sun, and the phase-1 stand-in valley.
//!
//! Phase 2 replaces [`build_placeholder`] with a course-shaped heightfield.
//! The environment (sky, sun, fog, tonemap) stays.

use godot::classes::directional_light_3d::ShadowMode;
use godot::classes::environment::{AmbientSource, BgMode, FogMode, ReflectionSource, ToneMapper};
use godot::classes::geometry_instance_3d::ShadowCastingSetting;
use godot::classes::light_3d::Param as LightParam;
use godot::classes::{
    CylinderMesh, DirectionalLight3D, Environment, Material, MeshInstance3D, Node3D, PlaneMesh,
    ProceduralSkyMaterial, Shader, ShaderMaterial, Sky, StandardMaterial3D, WorldEnvironment,
};
use godot::obj::Gd;
use godot::prelude::*;

use crate::mesh_util::{add_box, standard_material};
use crate::road::{self, CenterSample};

/// Height of the road surface above the meadow, in meters.
pub const ROAD_Y: f32 = 0.08;

/// How far the placeholder road runs, in meters.
pub const ROAD_LENGTH_M: f32 = 2_000.0;

const GROUND_SHADER: &str = r#"shader_type spatial;
render_mode diffuse_burley, specular_disabled;

varying vec3 world_pos;

void vertex() {
    world_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

void fragment() {
    vec2 cell = floor(world_pos.xz * 0.07);
    float fine = fract(sin(dot(cell, vec2(127.1, 311.7))) * 43758.5453);
    vec2 hill = floor(world_pos.xz * 0.011);
    float broad = fract(sin(dot(hill, vec2(269.5, 183.3))) * 43758.5453);
    vec3 grass = mix(vec3(0.20, 0.34, 0.15), vec3(0.33, 0.42, 0.20), fine);
    grass = mix(grass, vec3(0.28, 0.33, 0.22), broad * 0.5);
    ALBEDO = grass;
    ROUGHNESS = 1.0;
}
"#;

/// Alpine morning: procedural sky, a low sun, aerial fog, AgX so snow holds detail.
pub fn build_environment(parent: &mut Gd<Node3D>) {
    let mut sky_mat = ProceduralSkyMaterial::new_gd();
    sky_mat.set_sky_top_color(Color::from_rgb(0.27, 0.46, 0.80));
    sky_mat.set_sky_horizon_color(Color::from_rgb(0.78, 0.84, 0.90));
    sky_mat.set_sky_curve(0.10);
    sky_mat.set_ground_horizon_color(Color::from_rgb(0.55, 0.60, 0.58));
    sky_mat.set_ground_bottom_color(Color::from_rgb(0.30, 0.34, 0.30));
    sky_mat.set_ground_curve(0.06);
    sky_mat.set_sun_angle_max(26.0);
    sky_mat.set_sun_curve(0.10);
    sky_mat.set_energy_multiplier(1.02);
    sky_mat.set_use_debanding(true);

    let mut sky = Sky::new_gd();
    sky.set_material(&sky_mat.upcast::<Material>());

    let mut env = Environment::new_gd();
    env.set_background(BgMode::SKY);
    env.set_sky(&sky);
    env.set_ambient_source(AmbientSource::SKY);
    env.set_ambient_light_energy(0.85);
    env.set_reflection_source(ReflectionSource::SKY);
    env.set_tonemapper(ToneMapper::AGX);
    env.set_tonemap_exposure(1.0);
    env.set_fog_enabled(true);
    env.set_fog_mode(FogMode::EXPONENTIAL);
    env.set_fog_light_color(Color::from_rgb(0.74, 0.81, 0.86));
    env.set_fog_light_energy(0.9);
    env.set_fog_density(0.00028);
    env.set_fog_aerial_perspective(0.5);
    env.set_fog_sky_affect(0.9);
    env.set_fog_sun_scatter(0.10);
    env.set_adjustment_enabled(true);
    env.set_adjustment_contrast(1.05);
    env.set_adjustment_saturation(1.06);

    let mut world_env = WorldEnvironment::new_alloc();
    world_env.set_name("WorldEnvironment");
    world_env.set_environment(&env);
    parent.add_child(&world_env);

    let mut sun = DirectionalLight3D::new_alloc();
    sun.set_name("Sun");
    sun.set_rotation_degrees(Vector3::new(-38.0, -48.0, 0.0));
    sun.set_color(Color::from_rgb(1.0, 0.96, 0.90));
    sun.set_param(LightParam::ENERGY, 1.15);
    sun.set_param(LightParam::SPECULAR, 0.25);
    sun.set_shadow(true);
    sun.set_shadow_mode(ShadowMode::PARALLEL_4_SPLITS);
    sun.set_param(LightParam::SHADOW_BIAS, 0.04);
    sun.set_param(LightParam::SHADOW_NORMAL_BIAS, 1.4);
    sun.set_param(LightParam::SHADOW_BLUR, 0.6);
    sun.set_param(LightParam::SHADOW_MAX_DISTANCE, 900.0);
    sun.set_blend_splits(true);
    parent.add_child(&sun);
}

/// Meadow, a straight road, distant cones, and a start arch.
///
/// Returns the road samples so the rider can be placed on the same line.
pub fn build_placeholder(parent: &mut Gd<Node3D>) -> Vec<CenterSample> {
    add_ground(parent);
    let samples = road::straight_road(ROAD_LENGTH_M, ROAD_Y, 2.0);
    add_road(parent, &samples);
    add_peaks(parent);
    add_arch(parent);
    samples
}

fn add_ground(parent: &mut Gd<Node3D>) {
    let mut shader = Shader::new_gd();
    shader.set_code(GROUND_SHADER);
    let mut material = ShaderMaterial::new_gd();
    material.set_shader(&shader);

    let mut mesh = PlaneMesh::new_gd();
    mesh.set_size(Vector2::new(20_000.0, 20_000.0));
    let mut ground = MeshInstance3D::new_alloc();
    ground.set_name("Ground");
    ground.set_mesh(&mesh);
    ground.set_surface_override_material(0, &material);
    ground.set_cast_shadows_setting(ShadowCastingSetting::OFF);
    parent.add_child(&ground);
}

fn add_road(parent: &mut Gd<Node3D>, samples: &[CenterSample]) {
    let mesh = road::build_ribbon(samples, 6.0);
    let mut road = MeshInstance3D::new_alloc();
    road.set_name("Road");
    road.set_mesh(&mesh);
    road.set_surface_override_material(0, &road::road_material());
    // The ribbon sits 8 cm up. It does not cast, so it cannot shadow-acne itself.
    road.set_cast_shadows_setting(ShadowCastingSetting::OFF);
    parent.add_child(&road);
}

fn add_peaks(parent: &mut Gd<Node3D>) {
    let mut group = Node3D::new_alloc();
    group.set_name("Peaks");
    let rock = standard_material(Color::from_rgb(0.42, 0.42, 0.40), 0.92, 0.0);
    let snow = standard_material(Color::from_rgb(0.90, 0.91, 0.92), 0.55, 0.0);
    // (x, z, height, radius). Bases stay clear of the x = 0 road.
    let peaks = [
        (180.0, -280.0, 160.0, 70.0),
        (-210.0, -420.0, 210.0, 90.0),
        (420.0, -180.0, 260.0, 200.0),
        (-480.0, -420.0, 440.0, 300.0),
        (700.0, -860.0, 560.0, 360.0),
        (-820.0, -1000.0, 680.0, 420.0),
        (260.0, -1500.0, 240.0, 160.0),
        (-340.0, -1750.0, 360.0, 240.0),
        (1100.0, -300.0, 620.0, 400.0),
        (-1300.0, -200.0, 860.0, 520.0),
        (1500.0, -1100.0, 980.0, 560.0),
        (-1600.0, -1500.0, 900.0, 540.0),
        (360.0, 700.0, 320.0, 240.0),
        (-600.0, 900.0, 540.0, 360.0),
        (1200.0, 500.0, 780.0, 480.0),
        (-1400.0, 280.0, 700.0, 440.0),
        (0.0, -2800.0, 1100.0, 700.0),
        (1800.0, 1400.0, 1040.0, 620.0),
    ];
    for (index, (x, z, height, radius)) in peaks.iter().enumerate() {
        add_cone(
            &mut group,
            &format!("Rock{index}"),
            Vector3::new(*x, height * 0.5, *z),
            *radius,
            *height,
            7,
            &rock,
        );
        if *height > 300.0 {
            let snow_h = height * 0.22;
            add_cone(
                &mut group,
                &format!("Snow{index}"),
                Vector3::new(*x, height - snow_h * 0.5, *z),
                radius * snow_h / height,
                snow_h,
                6,
                &snow,
            );
        }
    }
    parent.add_child(&group);
}

fn add_cone(
    parent: &mut Gd<Node3D>,
    name: &str,
    position: Vector3,
    radius: f32,
    height: f32,
    segments: i32,
    material: &Gd<StandardMaterial3D>,
) {
    let mut mesh = CylinderMesh::new_gd();
    mesh.set_top_radius(0.0);
    mesh.set_bottom_radius(radius);
    mesh.set_height(height);
    mesh.set_radial_segments(segments);
    let mut instance = MeshInstance3D::new_alloc();
    instance.set_name(name);
    instance.set_mesh(&mesh);
    instance.set_position(position);
    instance.set_surface_override_material(0, &material.clone().upcast::<Material>());
    parent.add_child(&instance);
}

fn add_arch(parent: &mut Gd<Node3D>) {
    let mut arch = Node3D::new_alloc();
    arch.set_name("StartArch");
    // Just ahead of the rider at distance 0, clear of the 6 m road.
    let banner = standard_material(Color::from_rgb(0.93, 0.95, 0.96), 0.5, 0.0);
    let accent = standard_material(Color::from_rgb(0.90, 0.34, 0.16), 0.55, 0.0);
    let post = standard_material(Color::from_rgb(0.16, 0.17, 0.18), 0.6, 0.2);
    add_box(
        &mut arch,
        "LeftPost",
        Vector3::new(0.28, 4.2, 0.28),
        Vector3::new(-4.3, 2.1, -12.0),
        &post,
    );
    add_box(
        &mut arch,
        "RightPost",
        Vector3::new(0.28, 4.2, 0.28),
        Vector3::new(4.3, 2.1, -12.0),
        &post,
    );
    add_box(
        &mut arch,
        "Beam",
        Vector3::new(9.2, 0.36, 0.28),
        Vector3::new(0.0, 4.3, -12.0),
        &banner,
    );
    add_box(
        &mut arch,
        "Band",
        Vector3::new(6.4, 0.55, 0.08),
        Vector3::new(0.0, 3.7, -11.84),
        &accent,
    );
    parent.add_child(&arch);
}

/// Pose on the straight placeholder road. `distance_m` grows without bound; the mesh loops.
pub fn placeholder_pose(distance_m: f64) -> (Vector3, Vector3) {
    let along = distance_m.rem_euclid(f64::from(ROAD_LENGTH_M)) as f32;
    (Vector3::new(0.0, ROAD_Y, -along), Vector3::FORWARD)
}
