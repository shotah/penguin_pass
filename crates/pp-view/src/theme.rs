//! Theme file: colours, scatter rules, and how the ground rises into mountains.
//!
//! A theme that fails to parse or fails the checks is rejected. The game then
//! uses [`Theme::builtin`] and keeps running.

use serde::Deserialize;

/// Nothing is scattered inside this distance from the centreline.
pub const ROAD_KEEP_OUT_M: f32 = 5.0;

/// `true` when `name` can be a theme file stem: 1 to 32 lower-case ASCII
/// letters, digits, `-`, or `_`. Anything else (a path, an empty string)
/// is not looked for on disk.
pub fn is_theme_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// How the morning is lit, what gets scattered, and the skyline.
#[derive(Clone, Debug)]
pub struct Theme {
    /// Theme name from the file, or `"alpine"` for the built-in copy.
    pub name: String,
    /// Master seed mixed into every scatter layer and the relief.
    pub seed: u32,
    /// Procedural sky colours.
    pub sky: SkyLook,
    /// Sun direction and energy.
    pub sun: SunLook,
    /// Exponential fog.
    pub fog: FogLook,
    /// Ground layers, blended by altitude and slope.
    pub ground: Vec<GroundLayer>,
    /// What to instance beside the road.
    pub scatter: Vec<ScatterLayer>,
    /// How the ground rises into ridges away from the road.
    pub relief: crate::relief::Relief,
}

/// Sky gradient.
#[derive(Clone, Debug)]
pub struct SkyLook {
    /// Overhead colour, 0..1 RGB.
    pub top: [f32; 3],
    /// Horizon colour.
    pub horizon: [f32; 3],
    /// Ground-plane colour at the horizon.
    pub ground_horizon: [f32; 3],
    /// Ground-plane colour at the nadir.
    pub ground_bottom: [f32; 3],
    /// Sky curve.
    pub curve: f32,
    /// Sky energy.
    pub energy: f32,
    /// Catalog sky id. Empty keeps the catalog panorama. `"none"` is the
    /// procedural gradient, so a desert morning is not the alpine HDR.
    pub panorama: String,
}

/// Directional sun.
#[derive(Clone, Debug)]
pub struct SunLook {
    /// Pitch in degrees, negative is above the horizon.
    pub pitch: f32,
    /// Yaw in degrees.
    pub yaw: f32,
    /// Light energy.
    pub energy: f32,
    /// Light colour.
    pub color: [f32; 3],
    /// Directional shadow distance, in meters.
    pub shadow_distance: f32,
}

/// Aerial fog.
#[derive(Clone, Debug)]
pub struct FogLook {
    /// Exponential density.
    pub density: f32,
    /// Aerial perspective amount.
    pub aerial: f32,
    /// Fog colour.
    pub color: [f32; 3],
    /// Fog light energy.
    pub energy: f32,
}

/// One ground material band.
#[derive(Clone, Debug)]
pub struct GroundLayer {
    /// Layer name (`meadow`, `forest`, `rock`, `snow`, or anything else).
    pub name: String,
    /// Albedo, 0..1 RGB. Used when `texture` is empty.
    pub color: [f32; 3],
    /// Albedo texture id from the asset manifest.
    pub texture: String,
    /// Normal texture id. The terrain shader currently uses albedo only.
    pub normal: String,
    /// Altitude window, meters.
    pub altitude: [f32; 2],
    /// Slope window. 0 is flat, 1 is vertical.
    pub slope: [f32; 2],
    /// Blend width. Meters for altitude layers, slope units for rock.
    pub blend: f32,
}

/// One scatter rule. Placement is a function of chunk coordinates and seed.
#[derive(Clone, Debug)]
pub struct ScatterLayer {
    /// Placeholder mesh key: `tree`, `bush`, `grass`, or `rock`.
    pub kind: String,
    /// Optional asset-manifest ids. Empty means the built-in mesh for `kind`.
    pub ids: Vec<String>,
    /// Instances per hectare before the cluster test.
    pub density_per_ha: f32,
    /// Altitude window, meters.
    pub altitude: [f32; 2],
    /// Reject steeper than this. 0 is flat, 1 is vertical.
    pub max_slope: f32,
    /// Closest the instance may sit to the road centreline, meters.
    pub min_road_m: f32,
    /// Farthest it is placed from the centreline, meters.
    pub max_road_m: f32,
    /// Uniform scale range.
    pub scale: [f32; 2],
    /// Random yaw around Y.
    pub random_yaw: bool,
    /// Tip the instance a little with the slope.
    pub tilt_to_slope: bool,
    /// 0 scatters evenly. 1 keeps only the densest noise peaks.
    pub cluster: f32,
    /// Cluster noise frequency, cycles per meter.
    pub cluster_frequency: f32,
    /// Layer seed, mixed with the theme seed and the chunk.
    pub seed: u32,
    /// MultiMesh visibility end, meters.
    pub visibility_m: f32,
}

#[derive(Deserialize)]
struct ThemeFile {
    name: String,
    seed: u32,
    sky: SkyFile,
    sun: SunFile,
    fog: FogFile,
    ground: Vec<GroundFile>,
    scatter: Vec<ScatterFile>,
    /// Old files still carry this. It is ignored; a ring of triangles is not
    /// a mountain range. Present so a leftover section is not a parse error.
    #[serde(default)]
    backdrop: serde_json::Value,
    #[serde(default)]
    relief: ReliefFile,
}

#[derive(Deserialize)]
struct SkyFile {
    top: [f32; 3],
    horizon: [f32; 3],
    ground_horizon: [f32; 3],
    ground_bottom: [f32; 3],
    curve: f32,
    energy: f32,
    #[serde(default)]
    panorama: String,
}

#[derive(Deserialize)]
struct SunFile {
    pitch: f32,
    yaw: f32,
    energy: f32,
    color: [f32; 3],
    shadow_distance: f32,
}

#[derive(Deserialize)]
struct FogFile {
    density: f32,
    aerial: f32,
    color: [f32; 3],
    energy: f32,
}

#[derive(Deserialize)]
struct GroundFile {
    name: String,
    color: [f32; 3],
    #[serde(default)]
    texture: String,
    #[serde(default)]
    normal: String,
    altitude: [f32; 2],
    slope: [f32; 2],
    blend: f32,
}

#[derive(Deserialize)]
struct ScatterFile {
    kind: String,
    #[serde(default)]
    ids: Vec<String>,
    density_per_ha: f32,
    altitude: [f32; 2],
    max_slope: f32,
    min_road_m: f32,
    max_road_m: f32,
    scale: [f32; 2],
    random_yaw: bool,
    tilt_to_slope: bool,
    cluster: f32,
    cluster_frequency: f32,
    seed: u32,
    visibility_m: f32,
}

#[derive(Deserialize, Default)]
struct ReliefFile {
    #[serde(default)]
    amplitude_m: Option<f32>,
    #[serde(default)]
    wavelength_m: Option<f32>,
    #[serde(default)]
    octaves: Option<u32>,
    #[serde(default)]
    snow_line_m: Option<f32>,
    #[serde(default)]
    terraced: Option<bool>,
    #[serde(default)]
    terrace_m: Option<f32>,
    #[serde(default)]
    side_m: Option<f32>,
}

impl Theme {
    /// Alpine morning used when the file is missing or invalid.
    pub fn builtin() -> Self {
        Self::parse(include_str!("../../../godot/themes/alpine.json"))
            .expect("built-in alpine theme is valid")
    }

    /// Parse and validate theme JSON.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: ThemeFile = serde_json::from_str(text).map_err(|error| error.to_string())?;
        validate(&file)?;
        let defaults = crate::relief::Relief::default();
        let file_relief = file.relief;
        let _old_backdrop = file.backdrop;
        Ok(Self {
            name: file.name,
            seed: file.seed,
            sky: SkyLook {
                top: file.sky.top,
                horizon: file.sky.horizon,
                ground_horizon: file.sky.ground_horizon,
                ground_bottom: file.sky.ground_bottom,
                curve: file.sky.curve,
                energy: file.sky.energy,
                panorama: file.sky.panorama,
            },
            sun: SunLook {
                pitch: file.sun.pitch,
                yaw: file.sun.yaw,
                energy: file.sun.energy,
                color: file.sun.color,
                shadow_distance: file.sun.shadow_distance,
            },
            fog: FogLook {
                density: file.fog.density,
                aerial: file.fog.aerial,
                color: file.fog.color,
                energy: file.fog.energy,
            },
            ground: file
                .ground
                .into_iter()
                .map(|layer| GroundLayer {
                    name: layer.name,
                    color: layer.color,
                    texture: layer.texture,
                    normal: layer.normal,
                    altitude: layer.altitude,
                    slope: layer.slope,
                    blend: layer.blend,
                })
                .collect(),
            scatter: file
                .scatter
                .into_iter()
                .map(|layer| ScatterLayer {
                    kind: layer.kind,
                    ids: layer.ids,
                    density_per_ha: layer.density_per_ha,
                    altitude: layer.altitude,
                    max_slope: layer.max_slope,
                    min_road_m: layer.min_road_m.max(ROAD_KEEP_OUT_M),
                    max_road_m: layer.max_road_m,
                    scale: layer.scale,
                    random_yaw: layer.random_yaw,
                    tilt_to_slope: layer.tilt_to_slope,
                    cluster: layer.cluster.clamp(0.0, 1.0),
                    cluster_frequency: layer.cluster_frequency,
                    seed: layer.seed,
                    visibility_m: layer.visibility_m,
                })
                .collect(),
            relief: crate::relief::Relief {
                amplitude_m: file_relief
                    .amplitude_m
                    .unwrap_or(defaults.amplitude_m)
                    .clamp(0.0, 800.0),
                wavelength_m: file_relief
                    .wavelength_m
                    .unwrap_or(defaults.wavelength_m)
                    .clamp(400.0, 4_000.0),
                octaves: file_relief.octaves.unwrap_or(defaults.octaves).clamp(1, 5),
                snow_line_m: file_relief.snow_line_m.unwrap_or(defaults.snow_line_m),
                terraced: file_relief.terraced.unwrap_or(false),
                terrace_m: file_relief
                    .terrace_m
                    .unwrap_or(defaults.terrace_m)
                    .clamp(8.0, 120.0),
                rise_near_m: defaults.rise_near_m,
                rise_far_m: defaults.rise_far_m,
                side_m: file_relief
                    .side_m
                    .unwrap_or(defaults.side_m)
                    .clamp(0.0, 80.0),
            },
        })
    }
}

fn validate(file: &ThemeFile) -> Result<(), String> {
    if file.name.trim().is_empty() {
        return Err("theme name is empty".to_string());
    }
    if file.ground.is_empty() {
        return Err("theme has no ground layers".to_string());
    }
    for layer in &file.ground {
        check_window(&layer.name, "altitude", layer.altitude)?;
        check_window(&layer.name, "slope", layer.slope)?;
        if layer.blend < 0.0 {
            return Err(format!("{} blend is negative", layer.name));
        }
    }
    for layer in &file.scatter {
        if layer.kind.trim().is_empty() {
            return Err("scatter kind is empty".to_string());
        }
        if layer.density_per_ha < 0.0 {
            return Err(format!("{} density is negative", layer.kind));
        }
        check_window(&layer.kind, "altitude", layer.altitude)?;
        if layer.min_road_m < 0.0 || layer.max_road_m < layer.min_road_m {
            return Err(format!("{} road distance range is invalid", layer.kind));
        }
        if layer.scale[0] <= 0.0 || layer.scale[1] < layer.scale[0] {
            return Err(format!("{} scale range is invalid", layer.kind));
        }
        if layer.visibility_m <= 0.0 {
            return Err(format!("{} visibility is not positive", layer.kind));
        }
    }
    if let Some(amplitude) = file.relief.amplitude_m
        && !(0.0..=800.0).contains(&amplitude)
    {
        return Err("relief amplitude is out of range".to_string());
    }
    Ok(())
}

fn check_window(name: &str, label: &str, window: [f32; 2]) -> Result<(), String> {
    if window[1] < window[0] {
        Err(format!("{name} {label} range is inverted"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_names_are_file_stems() {
        assert!(is_theme_name("desert"));
        assert!(is_theme_name("alpine_2"));
        assert!(!is_theme_name(""));
        assert!(!is_theme_name("Desert"));
        assert!(!is_theme_name("../alpine"));
        assert!(!is_theme_name(&"a".repeat(33)));
    }

    #[test]
    fn the_built_in_alpine_theme_loads_and_keeps_the_road_clear() {
        let theme = Theme::builtin();
        assert_eq!(theme.name, "alpine");
        assert!(theme.scatter.iter().all(|layer| layer.min_road_m >= 5.0));
        assert!(!theme.relief.terraced);
        assert!(theme.relief.amplitude_m > 100.0);
        assert!(theme.relief.wavelength_m >= 900.0);
    }

    #[test]
    fn a_missing_relief_section_uses_the_defaults() {
        let text = r#"{
            "name": "desert",
            "seed": 3,
            "sky": {"top": [1,1,1], "horizon": [1,1,1], "ground_horizon": [1,1,1], "ground_bottom": [1,1,1], "curve": 0.1, "energy": 1.0},
            "sun": {"pitch": -20.0, "yaw": 10.0, "energy": 1.0, "color": [1,1,1], "shadow_distance": 100.0},
            "fog": {"density": 0.001, "aerial": 0.1, "color": [1,1,1], "energy": 1.0},
            "ground": [{"name": "sand", "color": [0.8, 0.7, 0.4], "altitude": [0.0, 10.0], "slope": [0.0, 1.0], "blend": 1.0}],
            "scatter": []
        }"#;
        let parsed = Theme::parse(text).unwrap();
        assert!(!parsed.relief.terraced);
        assert!(
            (parsed.relief.amplitude_m - crate::relief::Relief::default().amplitude_m).abs() < 1.0
        );
        assert_eq!(parsed.scatter.len(), 0);
        assert!(parsed.sky.panorama.is_empty());
    }

    #[test]
    fn an_old_backdrop_section_is_ignored() {
        let text = r#"{
            "name": "alpine",
            "seed": 1,
            "sky": {"top": [1,1,1], "horizon": [1,1,1], "ground_horizon": [1,1,1], "ground_bottom": [1,1,1], "curve": 0.1, "energy": 1.0},
            "sun": {"pitch": -20.0, "yaw": 10.0, "energy": 1.0, "color": [1,1,1], "shadow_distance": 100.0},
            "fog": {"density": 0.001, "aerial": 0.1, "color": [1,1,1], "energy": 1.0},
            "ground": [{"name": "meadow", "color": [0.3, 0.5, 0.2], "altitude": [0.0, 10.0], "slope": [0.0, 1.0], "blend": 1.0}],
            "backdrop": {"rings": 3, "style": "mesas", "height_m": [90.0, 200.0], "segments": 96},
            "scatter": []
        }"#;
        let parsed = Theme::parse(text).unwrap();
        assert!(!parsed.relief.terraced);
        assert!(parsed.relief.amplitude_m > 0.0);
    }

    #[test]
    fn a_bad_theme_is_rejected_and_a_terrace_is_kept() {
        assert!(Theme::parse("{").is_err());
        assert!(Theme::parse(r#"{"name":" ","seed":1}"#).is_err());
        let theme = Theme::parse(include_str!("../../../godot/themes/desert.json")).unwrap();
        assert!(theme.relief.terraced);
    }

    #[test]
    fn the_desert_file_is_a_mesa_theme_without_the_alpine_sky() {
        let theme = Theme::parse(include_str!("../../../godot/themes/desert.json")).unwrap();
        assert_eq!(theme.name, "desert");
        assert_eq!(theme.sky.panorama, "none");
        assert!(theme.relief.terraced);
        assert!(theme.relief.snow_line_m > 1_000.0);
        assert!(
            theme
                .ground
                .iter()
                .any(|layer| layer.texture == "ground_sand")
        );
        assert!(theme.scatter.iter().all(|layer| layer.min_road_m >= 5.0));
    }
}
