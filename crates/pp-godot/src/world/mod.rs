//! Sky, sun, and the course world.
//!
//! [`build_environment`] is the alpine morning. [`build_course`] lays the road,
//! the heightfield, the trees, and the start line on top of a [`Course`].

use godot::classes::directional_light_3d::ShadowMode;
use godot::classes::environment::{AmbientSource, BgMode, FogMode, ReflectionSource, ToneMapper};
use godot::classes::geometry_instance_3d::ShadowCastingSetting;
use godot::classes::light_3d::Param as LightParam;
use godot::classes::{
    DirectionalLight3D, Environment, Material, MeshInstance3D, Node3D, ProceduralSkyMaterial, Sky,
    WorldEnvironment,
};
use godot::obj::Gd;
use godot::prelude::*;

pub(crate) mod scenery;
pub(crate) mod terrain;

use crate::assets::Catalog;
use crate::road::{self, CenterSample};
use crate::theme::Theme;
use pp_course::Course;

pub use scenery::ScatterReport;

/// Alpine morning: procedural sky, a low sun, aerial fog, AgX so snow holds detail.
pub fn build_environment(parent: &mut Gd<Node3D>, theme: &Theme, catalog: &Catalog) {
    let mut sky_mat = ProceduralSkyMaterial::new_gd();
    sky_mat.set_sky_top_color(crate::theme::rgb(theme.sky.top));
    sky_mat.set_sky_horizon_color(crate::theme::rgb(theme.sky.horizon));
    sky_mat.set_sky_curve(theme.sky.curve);
    sky_mat.set_ground_horizon_color(crate::theme::rgb(theme.sky.ground_horizon));
    sky_mat.set_ground_bottom_color(crate::theme::rgb(theme.sky.ground_bottom));
    sky_mat.set_ground_curve(0.05);
    sky_mat.set_sun_angle_max(22.0);
    sky_mat.set_sun_curve(0.08);
    sky_mat.set_energy_multiplier(theme.sky.energy);
    sky_mat.set_use_debanding(true);

    let mut sky = Sky::new_gd();
    let use_photo = theme.sky.panorama != "none";
    if use_photo && let Some(panorama) = catalog.sky_texture() {
        let mut photo = godot::classes::PanoramaSkyMaterial::new_gd();
        photo.set_panorama(&panorama);
        photo.set_energy_multiplier(theme.sky.energy);
        sky.set_material(&photo.upcast::<Material>());
    } else {
        sky.set_material(&sky_mat.upcast::<Material>());
    }

    let mut env = Environment::new_gd();
    env.set_background(BgMode::SKY);
    env.set_sky(&sky);
    env.set_ambient_source(AmbientSource::SKY);
    env.set_ambient_light_energy(0.85);
    env.set_reflection_source(ReflectionSource::SKY);
    env.set_tonemapper(ToneMapper::AGX);
    env.set_tonemap_exposure(0.92);
    env.set_fog_enabled(true);
    env.set_fog_mode(FogMode::EXPONENTIAL);
    env.set_fog_light_color(crate::theme::rgb(theme.fog.color));
    env.set_fog_light_energy(theme.fog.energy);
    env.set_fog_density(theme.fog.density);
    env.set_fog_aerial_perspective(theme.fog.aerial);
    env.set_fog_sky_affect(0.45);
    env.set_fog_sun_scatter(0.06);
    env.set_adjustment_enabled(true);
    env.set_adjustment_contrast(1.05);
    env.set_adjustment_saturation(1.06);

    let mut world_env = WorldEnvironment::new_alloc();
    world_env.set_name("WorldEnvironment");
    world_env.set_environment(&env);
    parent.add_child(&world_env);

    let mut sun = DirectionalLight3D::new_alloc();
    sun.set_name("Sun");
    sun.set_rotation_degrees(Vector3::new(theme.sun.pitch, theme.sun.yaw, 0.0));
    sun.set_color(crate::theme::rgb(theme.sun.color));
    sun.set_param(LightParam::ENERGY, theme.sun.energy);
    sun.set_param(LightParam::SPECULAR, 0.25);
    sun.set_shadow(true);
    sun.set_shadow_mode(ShadowMode::PARALLEL_4_SPLITS);
    sun.set_param(LightParam::SHADOW_BIAS, 0.04);
    sun.set_param(LightParam::SHADOW_NORMAL_BIAS, 1.4);
    sun.set_param(LightParam::SHADOW_BLUR, 0.6);
    sun.set_param(LightParam::SHADOW_MAX_DISTANCE, theme.sun.shadow_distance);
    sun.set_blend_splits(true);
    parent.add_child(&sun);
}

/// Checksum of the scatter the world would build for this course and theme.
///
/// Uses the same centreline samples, the same ground drop under the ribbon,
/// and the same relief as [`build_course`], so it matches the live world.
pub fn scatter_checksum(course: &Course, theme: &Theme) -> i64 {
    let samples = terrain::sample_centerline(course);
    let raw: Vec<[f32; 3]> = samples
        .iter()
        .map(|sample| [sample.position.x, sample.position.y, sample.position.z])
        .collect();
    let mut field = terrain::Field::new(&raw, theme.seed as i32);
    field.use_relief(theme.relief.clone(), theme.seed);
    scenery::recompute_checksum(&field, theme).1
}

/// Road, terrain, trees, the start arch, and the kilometer posts.
pub fn build_course(
    parent: &mut Gd<Node3D>,
    course: &Course,
    theme: &Theme,
    catalog: &Catalog,
) -> ScatterReport {
    let (samples, field) = terrain::build(parent, course, theme, catalog);
    add_road(parent, &samples, catalog);
    scenery::build(parent, course, &field, theme, catalog)
}

fn add_road(parent: &mut Gd<Node3D>, samples: &[CenterSample], catalog: &Catalog) {
    let mesh = road::build_ribbon(samples, road::RIBBON_WIDTH_M);
    let mut road = MeshInstance3D::new_alloc();
    road.set_name("Road");
    road.set_mesh(&mesh);
    road.set_surface_override_material(0, &road::road_material(catalog));
    // The ribbon sits 8 cm up. It does not cast, so it cannot shadow-acne itself.
    road.set_cast_shadows_setting(ShadowCastingSetting::OFF);
    parent.add_child(&road);
}
