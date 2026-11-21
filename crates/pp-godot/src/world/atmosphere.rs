//! Applies an [`pp_world::AtmosphereState`] to the sun, sky, fog, road, and rider.
//!
//! [`Atmosphere::default_atmosphere`] is clear noon. [`build_environment`] applies
//! it so a ride that never calls in stays on the theme's daytime look.

use godot::classes::base_material_3d::{BillboardMode, ShadingMode, Transparency};
use godot::classes::light_3d::Param as LightParam;
use godot::classes::{
    DirectionalLight3D, Material, MeshInstance3D, Node3D, ProceduralSkyMaterial, QuadMesh,
    ShaderMaterial, Sky, SphereMesh, SpotLight3D, StandardMaterial3D, WorldEnvironment,
};
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;
use pp_world::{AtmosphereState, PrecipKind, TimeOfDay, Weather, atmosphere_at, hour_at};

const PRECIP_COUNT: u32 = 40;

/// Sky, sun, headlight, and precipitation for one course.
#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Atmosphere {
    base: Base<Node3D>,
    procedural: Option<Gd<ProceduralSkyMaterial>>,
    saved_sky: Option<Gd<Material>>,
    wet_ready: bool,
    shown_wet: f32,
    shown_precip: f32,
    effects_ready: bool,
}

#[godot_api]
impl INode3D for Atmosphere {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            procedural: None,
            saved_sky: None,
            wet_ready: false,
            shown_wet: 0.0,
            shown_precip: 0.0,
            effects_ready: false,
        }
    }
}

#[godot_api]
impl Atmosphere {
    /// Drive the look from the inspection script.
    ///
    /// `weather` is `clear`, `cloudy`, `rain`, `snow`, `fog`, or `auto`.
    /// A call snaps. The rider vectors are world space.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn apply_named(
        &mut self,
        theme: GString,
        hour: f64,
        weather: GString,
        x: f32,
        y: f32,
        z: f32,
        fx: f32,
        fy: f32,
        fz: f32,
    ) {
        let state = atmosphere_at(
            theme.to_string().as_str(),
            hour as f32,
            parse_weather(&weather.to_string()),
        );
        self.apply_atmosphere(&state, Vector3::new(x, y, z), Vector3::new(fx, fy, fz), 0.0);
    }
}

impl Atmosphere {
    /// Clear alpine noon. Matches the built-in daytime theme.
    #[must_use]
    pub fn default_atmosphere() -> AtmosphereState {
        atmosphere_at("alpine", hour_at(TimeOfDay::Day, 0.0), Weather::Clear)
    }

    /// Push `state` onto the sun, sky, fog, wet road, headlight, and precipitation.
    ///
    /// `dt_s` eases wetness and precipitation. Zero snaps, which is what the
    /// world build and the inspection pictures use.
    pub fn apply_atmosphere(
        &mut self,
        state: &AtmosphereState,
        rider_pos: Vector3,
        rider_forward: Vector3,
        dt_s: f32,
    ) {
        let blend = if dt_s <= 0.0 {
            1.0
        } else {
            (dt_s * 2.5).clamp(0.0, 1.0)
        };
        self.shown_wet += (state.wetness - self.shown_wet) * blend;
        self.shown_precip += (state.precip_intensity - self.shown_precip) * blend;
        self.ensure_effects();
        let forward = unit(rider_forward, Vector3::new(0.0, 0.0, -1.0));
        if self.base().is_inside_tree() {
            self.base_mut().set_global_position(rider_pos);
        }
        self.update_sun(state);
        self.update_sky(state);
        self.update_headlight(state, rider_pos, forward);
        self.update_precip(state, forward);
        self.update_stars(state, forward);
        self.update_road();
    }

    fn ensure_effects(&mut self) {
        if self.effects_ready {
            return;
        }
        self.effects_ready = true;
        let mut root = self.base_mut();
        let mut headlight = SpotLight3D::new_alloc();
        headlight.set_name("Headlight");
        headlight.set_shadow(false);
        headlight.set_param(LightParam::SPOT_ANGLE, 34.0);
        headlight.set_param(LightParam::SPOT_ATTENUATION, 0.7);
        headlight.set_param(LightParam::RANGE, 42.0);
        headlight.set_param(LightParam::ENERGY, 0.0);
        headlight.set_color(Color::from_rgb(1.0, 0.94, 0.82));
        headlight.set_visible(false);
        root.add_child(&headlight);

        let mut stars = Node3D::new_alloc();
        stars.set_name("Stars");
        let star_mat = star_material();
        for index in 0..18 {
            let mut quad = MeshInstance3D::new_alloc();
            let mut mesh = QuadMesh::new_gd();
            mesh.set_size(Vector2::new(1.8, 1.8));
            quad.set_name(&format!("Star{index}"));
            quad.set_mesh(&mesh);
            quad.set_surface_override_material(0, &star_mat.clone().upcast::<Material>());
            quad.set_position(sky_dir(index) * 380.0);
            stars.add_child(&quad);
        }
        let mut moon = MeshInstance3D::new_alloc();
        moon.set_name("Moon");
        let mut sphere = SphereMesh::new_gd();
        sphere.set_radius(6.5);
        sphere.set_height(13.0);
        moon.set_mesh(&sphere);
        moon.set_surface_override_material(0, &moon_material().upcast::<Material>());
        moon.set_position(Vector3::new(42.0, 46.0, -120.0));
        stars.add_child(&moon);
        stars.set_visible(false);
        root.add_child(&stars);

        let mut precip = Node3D::new_alloc();
        precip.set_name("Precip");
        let mat = precip_material();
        for index in 0..PRECIP_COUNT {
            let mut flake = MeshInstance3D::new_alloc();
            flake.set_name(&format!("Flake{index}"));
            let mut mesh = QuadMesh::new_gd();
            mesh.set_size(Vector2::new(0.06, 1.5));
            flake.set_mesh(&mesh);
            flake.set_surface_override_material(0, &mat.clone().upcast::<Material>());
            flake.set_cast_shadows_setting(
                godot::classes::geometry_instance_3d::ShadowCastingSetting::OFF,
            );
            precip.add_child(&flake);
        }
        precip.set_visible(false);
        root.add_child(&precip);
    }

    fn update_sun(&self, state: &AtmosphereState) {
        let Some(mut sun) = child_as::<DirectionalLight3D>(self.base().get_parent(), "Sun") else {
            return;
        };
        sun.set_rotation_degrees(Vector3::new(
            -state.sun_elevation_deg,
            state.sun_azimuth_deg,
            0.0,
        ));
        sun.set_color(rgb(state.sun_color));
        sun.set_param(LightParam::ENERGY, state.sun_energy);
    }

    fn update_sky(&mut self, state: &AtmosphereState) {
        let Some(mut env_node) =
            child_as::<WorldEnvironment>(self.base().get_parent(), "WorldEnvironment")
        else {
            return;
        };
        let Some(mut env) = env_node.get_environment() else {
            return;
        };
        env.set_fog_light_color(rgb(state.fog_color));
        env.set_fog_light_energy(state.fog_energy);
        env.set_fog_density(state.fog_density);
        env.set_fog_aerial_perspective(state.fog_aerial);
        env.set_ambient_light_energy(state.ambient_energy);
        env.set_tonemap_exposure(0.92 + 0.14 * state.headlight);
        let Some(mut sky) = env.get_sky() else {
            return;
        };
        let clear_day = state.headlight < 0.02
            && state.precip_intensity < 0.02
            && state.wetness < 0.02
            && state.stars < 0.02
            && state.sun_elevation_deg > 28.0
            && state.sun_energy > 1.0;
        if clear_day {
            if let Some(saved) = &self.saved_sky {
                sky.set_material(saved);
            }
            if let Some(material) = sky.get_material()
                && let Ok(mut procedural) = material.try_cast::<ProceduralSkyMaterial>()
            {
                paint_sky(&mut procedural, state);
            }
        } else {
            let mut procedural = self.procedural_sky(&mut sky);
            paint_sky(&mut procedural, state);
        }
        env.set_sky(&sky);
        env_node.set_environment(&env);
    }

    fn procedural_sky(&mut self, sky: &mut Gd<Sky>) -> Gd<ProceduralSkyMaterial> {
        if self.procedural.is_none() {
            if self.saved_sky.is_none() {
                self.saved_sky = sky.get_material();
            }
            let mut created = ProceduralSkyMaterial::new_gd();
            created.set_sun_angle_max(22.0);
            created.set_sun_curve(0.08);
            created.set_ground_curve(0.05);
            created.set_use_debanding(true);
            self.procedural = Some(created);
        }
        let procedural = self.procedural.clone().expect("procedural sky");
        sky.set_material(&procedural.clone().upcast::<Material>());
        procedural
    }

    fn update_headlight(&self, state: &AtmosphereState, rider_pos: Vector3, forward: Vector3) {
        let Some(mut light) = self.base().try_get_node_as::<SpotLight3D>("Headlight") else {
            return;
        };
        let on = state.headlight > 0.05;
        light.set_visible(on);
        light.set_param(LightParam::ENERGY, state.headlight * 14.0);
        if !on {
            return;
        }
        let right = unit(Vector3::UP.cross(forward), Vector3::RIGHT);
        let up = unit(forward.cross(right), Vector3::UP);
        let pos = rider_pos + up * 1.2 + forward * 0.7;
        let aim = pos + forward * 14.0 - up * 1.6;
        light.look_at_from_position(pos, aim);
    }

    fn update_precip(&mut self, state: &AtmosphereState, forward: Vector3) {
        let Some(mut precip) = self.base().try_get_node_as::<Node3D>("Precip") else {
            return;
        };
        let intensity = self.shown_precip;
        let show = intensity > 0.04 && state.precipitation != PrecipKind::None;
        precip.set_visible(show);
        if !show {
            return;
        }
        let (width, height, tint) = match state.precipitation {
            PrecipKind::Snow => (0.16, 0.16, Color::from_rgba(0.95, 0.96, 1.0, 0.8)),
            PrecipKind::Dust => (0.9, 0.28, Color::from_rgba(0.72, 0.58, 0.38, 0.28)),
            _ => (0.045, 1.45, Color::from_rgba(0.72, 0.80, 0.88, 0.55)),
        };
        let alpha = tint.a * intensity.clamp(0.35, 1.0);
        let color = Color::from_rgba(tint.r, tint.g, tint.b, alpha);
        let right = unit(Vector3::UP.cross(forward), Vector3::RIGHT);
        let up = unit(forward.cross(right), Vector3::UP);
        for index in 0..PRECIP_COUNT {
            let Some(mut flake) =
                precip.try_get_node_as::<MeshInstance3D>(&format!("Flake{index}"))
            else {
                continue;
            };
            if let Some(mesh) = flake.get_mesh()
                && let Ok(mut quad) = mesh.try_cast::<QuadMesh>()
            {
                quad.set_size(Vector2::new(width, height));
            }
            if let Some(material) = flake.get_surface_override_material(0)
                && let Ok(mut standard) = material.try_cast::<StandardMaterial3D>()
            {
                standard.set_albedo(color);
            }
            let slot = hash01(index);
            let across = hash01(index.wrapping_add(17));
            let lift = hash01(index.wrapping_add(41));
            let depth = 1.5 + slot * 14.0;
            let side = (across - 0.5) * 9.0;
            let rise = (lift - 0.35) * 6.5;
            flake.set_position(forward * depth + right * side + up * rise);
        }
    }

    fn update_stars(&self, state: &AtmosphereState, forward: Vector3) {
        let Some(mut stars) = self.base().try_get_node_as::<Node3D>("Stars") else {
            return;
        };
        stars.set_visible(state.stars > 0.04);
        if state.stars <= 0.04 {
            return;
        }
        let alpha = (state.stars * 0.95).clamp(0.0, 1.0);
        let right = unit(Vector3::UP.cross(forward), Vector3::RIGHT);
        let up = unit(forward.cross(right), Vector3::UP);
        for index in 0..18 {
            let Some(mut quad) = stars.try_get_node_as::<MeshInstance3D>(&format!("Star{index}"))
            else {
                continue;
            };
            let spread = (index as f32 - 8.5) * 0.16;
            let lift = 0.18 + (index % 5) as f32 * 0.07;
            let dir = (forward + right * spread + up * lift).normalized();
            quad.set_position(dir * 280.0);
            if let Some(material) = quad.get_surface_override_material(0)
                && let Ok(mut standard) = material.try_cast::<StandardMaterial3D>()
            {
                standard.set_albedo(Color::from_rgba(1.0, 0.98, 0.92, alpha));
            }
        }
        if let Some(mut moon) = stars.try_get_node_as::<MeshInstance3D>("Moon") {
            moon.set_position(forward * 240.0 + up * 78.0 + right * 36.0);
            if let Some(material) = moon.get_surface_override_material(0)
                && let Ok(mut standard) = material.try_cast::<StandardMaterial3D>()
            {
                standard.set_albedo(Color::from_rgba(0.95, 0.95, 0.90, alpha));
            }
        }
    }

    fn update_road(&mut self) {
        let Some(road) = child_as::<MeshInstance3D>(self.base().get_parent(), "Road") else {
            return;
        };
        let Some(material) = road.get_surface_override_material(0) else {
            return;
        };
        let Ok(mut shader_mat) = material.try_cast::<ShaderMaterial>() else {
            return;
        };
        if !self.wet_ready {
            self.wet_ready = true;
            patch_wetness(&mut shader_mat);
        }
        shader_mat.set_shader_parameter("wetness", &self.shown_wet.to_variant());
    }
}

fn paint_sky(sky: &mut Gd<ProceduralSkyMaterial>, state: &AtmosphereState) {
    sky.set_sky_top_color(rgb(state.sky_zenith));
    sky.set_sky_horizon_color(rgb(state.sky_horizon));
    sky.set_ground_horizon_color(rgb(state.sky_ground));
    sky.set_ground_bottom_color(rgb(state.ground_bottom));
    sky.set_sky_curve(state.sky_curve);
    sky.set_energy_multiplier(state.sky_energy);
}

fn patch_wetness(material: &mut Gd<ShaderMaterial>) {
    let Some(mut shader) = material.get_shader() else {
        return;
    };
    let code = shader.get_code().to_string();
    if code.contains("uniform float wetness") {
        return;
    }
    let patched = code
        .replacen(
            "uniform float textured = 0.0;",
            "uniform float textured = 0.0;\nuniform float wetness = 0.0;",
            1,
        )
        .replacen(
            "ALBEDO = mix(surface, paint, mark);",
            "ALBEDO = mix(surface, paint, mark);\n    ALBEDO = mix(ALBEDO, ALBEDO * vec3(0.55, 0.58, 0.62), wetness * (1.0 - verge));",
            1,
        )
        .replacen(
            "ROUGHNESS = mix(0.88, 0.96, shoulder);",
            "ROUGHNESS = mix(mix(0.88, 0.96, shoulder), 0.32, wetness * (1.0 - shoulder));",
            1,
        );
    shader.set_code(patched.as_str());
}

fn star_material() -> Gd<StandardMaterial3D> {
    let mut material = StandardMaterial3D::new_gd();
    material.set_shading_mode(ShadingMode::UNSHADED);
    material.set_transparency(Transparency::ALPHA);
    material.set_albedo(Color::from_rgba(1.0, 0.98, 0.92, 0.0));
    material.set_billboard_mode(BillboardMode::ENABLED);
    material
}

fn moon_material() -> Gd<StandardMaterial3D> {
    let mut material = StandardMaterial3D::new_gd();
    material.set_shading_mode(ShadingMode::UNSHADED);
    material.set_transparency(Transparency::ALPHA);
    material.set_albedo(Color::from_rgba(0.92, 0.93, 0.88, 0.0));
    material.set_emission(Color::from_rgb(0.85, 0.88, 0.95));
    material.set_emission_energy_multiplier(0.4);
    material
}

fn precip_material() -> Gd<StandardMaterial3D> {
    let mut material = StandardMaterial3D::new_gd();
    material.set_shading_mode(ShadingMode::UNSHADED);
    material.set_transparency(Transparency::ALPHA);
    material.set_albedo(Color::from_rgba(0.75, 0.82, 0.9, 0.0));
    material.set_billboard_mode(BillboardMode::ENABLED);
    material
}

fn sky_dir(index: i32) -> Vector3 {
    let angle = index as f32 * 0.55;
    let lift = 0.22 + (index % 4) as f32 * 0.07;
    Vector3::new(angle.sin() * 0.85, lift, -0.75).normalized()
}

fn hash01(index: u32) -> f32 {
    let mut value = index.wrapping_mul(747796405).wrapping_add(2_891_336_453);
    value = (value >> 16) ^ value;
    (value % 10_000) as f32 / 10_000.0
}

fn unit(vector: Vector3, fallback: Vector3) -> Vector3 {
    if vector.length_squared() < 1.0e-8 {
        fallback
    } else {
        vector.normalized()
    }
}

fn rgb(color: [f32; 3]) -> Color {
    Color::from_rgb(color[0], color[1], color[2])
}

fn parse_weather(name: &str) -> Weather {
    match name {
        "cloudy" => Weather::Cloudy,
        "rain" => Weather::Rain,
        "snow" => Weather::Snow,
        "fog" => Weather::Fog,
        "auto" => Weather::Auto,
        _ => Weather::Clear,
    }
}

fn child_as<T>(parent: Option<Gd<Node>>, name: &str) -> Option<Gd<T>>
where
    T: GodotClass + Inherits<Node>,
{
    let parent = parent?;
    let node = parent.get_node_or_null(name)?;
    node.try_cast::<T>().ok()
}
