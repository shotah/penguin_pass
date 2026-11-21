//! Theme file: colours, scatter rules, and the distant backdrop.
//!
//! A theme that fails to parse or fails the checks is rejected. The game then
//! uses [`Theme::builtin`] and keeps running.

use serde::Deserialize;

/// Nothing is scattered inside this distance from the centreline.
pub const ROAD_KEEP_OUT_M: f32 = 5.0;

/// How the morning is lit, what gets scattered, and the skyline.
#[derive(Clone, Debug)]
pub struct Theme {
    /// Theme name from the file, or `"alpine"` for the built-in copy.
    pub name: String,
    /// Master seed mixed into every scatter layer and the backdrop.
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
    /// Distant skyline. Defaults when the file has no `backdrop` section.
    pub backdrop: Backdrop,
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

/// Distant skyline. `style` is `"ridges"` or `"mesas"`.
#[derive(Clone, Debug)]
pub struct Backdrop {
    /// How many rings of mountains. Zero draws none.
    pub rings: u32,
    /// Metres past the furthest road sample where the first ring starts.
    pub inner_margin_m: f32,
    /// Extra metres of radius for each further ring.
    pub ring_gap_m: f32,
    /// How far below zero the skirt starts, so it tucks under the terrain.
    pub base_drop_m: f32,
    /// Ridge height range, metres.
    pub height_m: [f32; 2],
    /// Fraction of the ridge that is snow, wobbling with the noise.
    pub snow_fraction: f32,
    /// Vertices around one ring.
    pub segments: u32,
    /// `"ridges"` or `"mesas"`. Anything else is drawn as ridges.
    pub style: String,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self {
            rings: 3,
            inner_margin_m: 90.0,
            ring_gap_m: 700.0,
            base_drop_m: 60.0,
            height_m: [140.0, 280.0],
            snow_fraction: 0.86,
            segments: 128,
            style: "ridges".to_owned(),
        }
    }
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
    #[serde(default)]
    backdrop: BackdropFile,
}

#[derive(Deserialize)]
struct SkyFile {
    top: [f32; 3],
    horizon: [f32; 3],
    ground_horizon: [f32; 3],
    ground_bottom: [f32; 3],
    curve: f32,
    energy: f32,
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
struct BackdropFile {
    #[serde(default)]
    rings: Option<u32>,
    #[serde(default)]
    inner_margin_m: Option<f32>,
    #[serde(default)]
    ring_gap_m: Option<f32>,
    #[serde(default)]
    base_drop_m: Option<f32>,
    #[serde(default)]
    height_m: Option<[f32; 2]>,
    #[serde(default)]
    snow_fraction: Option<f32>,
    #[serde(default)]
    segments: Option<u32>,
    #[serde(default)]
    style: Option<String>,
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
        let defaults = Backdrop::default();
        let file_backdrop = file.backdrop;
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
            backdrop: Backdrop {
                rings: file_backdrop.rings.unwrap_or(defaults.rings).min(6),
                inner_margin_m: file_backdrop
                    .inner_margin_m
                    .unwrap_or(defaults.inner_margin_m)
                    .max(40.0),
                ring_gap_m: file_backdrop
                    .ring_gap_m
                    .unwrap_or(defaults.ring_gap_m)
                    .max(80.0),
                base_drop_m: file_backdrop
                    .base_drop_m
                    .unwrap_or(defaults.base_drop_m)
                    .max(0.0),
                height_m: file_backdrop.height_m.unwrap_or(defaults.height_m),
                snow_fraction: file_backdrop
                    .snow_fraction
                    .unwrap_or(defaults.snow_fraction)
                    .clamp(0.0, 1.0),
                segments: file_backdrop
                    .segments
                    .unwrap_or(defaults.segments)
                    .clamp(24, 180),
                style: file_backdrop
                    .style
                    .filter(|style| !style.trim().is_empty())
                    .unwrap_or(defaults.style),
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
    if let Some(height) = file.backdrop.height_m
        && (height[0] <= 0.0 || height[1] < height[0])
    {
        return Err("backdrop height range is invalid".to_string());
    }
    Ok(())
}

impl Theme {
    /// Whether the backdrop should be flat-topped mesas.
    pub fn style_is_mesa(&self) -> bool {
        self.backdrop.style == "mesas"
    }
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
    fn the_built_in_alpine_theme_loads_and_keeps_the_road_clear() {
        let theme = Theme::builtin();
        assert_eq!(theme.name, "alpine");
        assert!(theme.scatter.iter().all(|layer| layer.min_road_m >= 5.0));
        assert_eq!(theme.backdrop.rings, 3);
        assert_eq!(theme.backdrop.style, "ridges");
    }

    #[test]
    fn a_missing_backdrop_section_uses_the_defaults() {
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
        assert_eq!(parsed.backdrop.rings, Backdrop::default().rings);
        assert_eq!(parsed.backdrop.style, "ridges");
        assert_eq!(parsed.scatter.len(), 0);
    }

    #[test]
    fn a_bad_theme_is_rejected_and_a_mesa_style_is_kept() {
        assert!(Theme::parse("{").is_err());
        assert!(Theme::parse(r#"{"name":" ","seed":1}"#).is_err());
        let mut text = include_str!("../../../godot/themes/alpine.json").to_owned();
        text = text.replacen("\"ridges\"", "\"mesas\"", 1);
        let theme = Theme::parse(&text).unwrap();
        assert!(theme.style_is_mesa());
        assert_eq!(theme.backdrop.style, "mesas");
    }
}
