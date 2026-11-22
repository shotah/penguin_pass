//! Time of day and weather, with no Godot types.
//!
//! [`hour_at`] turns a [`TimeOfDay`] and ride time into a clock hour.
//! [`resolve_weather`] turns [`Weather::Auto`] into a slow blend. [`atmosphere_at`]
//! paints one hour and one concrete weather into an [`AtmosphereState`] the
//! renderer can apply. Hours interpolate, so dawn, day, sunset, and night do
//! not pop.

/// One full day of [`TimeOfDay::Auto`], in seconds of ride time.
pub const AUTO_DAY_SECONDS: f32 = 30.0 * 60.0;

/// Clock hour where [`TimeOfDay::Auto`] starts. Morning.
pub const AUTO_START_HOUR: f32 = 8.0;

/// Clock hour of [`TimeOfDay::Day`]. Noon, matching the built-in sun pitch.
pub const DAY_HOUR: f32 = 12.0;

/// How the ride clock is chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeOfDay {
    /// A full day every [`AUTO_DAY_SECONDS`] of ride time, starting in the morning.
    Auto,
    /// Low warm sun, just after sunrise.
    Dawn,
    /// The built-in daytime look.
    Day,
    /// Low warm sun, before dusk.
    Sunset,
    /// Dark sky. The headlight is on.
    Night,
}

/// Weather the rider is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weather {
    /// Drift between concrete weathers over minutes of ride time.
    Auto,
    /// The built-in look.
    Clear,
    /// Grey light, no precipitation.
    Cloudy,
    /// Rain and a wet road.
    Rain,
    /// Snow, or dust on a desert theme.
    Snow,
    /// Thick fog, little precipitation.
    Fog,
}

/// What is falling, if anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecipKind {
    /// Nothing falling.
    None,
    /// Rain streaks.
    Rain,
    /// Snowflakes.
    Snow,
    /// Desert dust. Used when a desert theme is asked for snow.
    Dust,
}

/// A slow blend between two concrete weathers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeatherMix {
    /// Weather at the start of the blend. Never [`Weather::Auto`].
    pub from: Weather,
    /// Weather at the end of the blend. Never [`Weather::Auto`].
    pub to: Weather,
    /// 0 keeps `from`. 1 is `to`.
    pub t: f32,
}

/// Everything the renderer needs for one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtmosphereState {
    /// Clock hour in `0.0..24.0`.
    pub hour: f32,
    /// Degrees above the horizon. Negative is night.
    pub sun_elevation_deg: f32,
    /// Godot yaw degrees. Noon matches the theme sun.
    pub sun_azimuth_deg: f32,
    /// Linear sun colour.
    pub sun_color: [f32; 3],
    /// Directional light energy.
    pub sun_energy: f32,
    /// Sky top colour.
    pub sky_zenith: [f32; 3],
    /// Sky horizon colour.
    pub sky_horizon: [f32; 3],
    /// Ground-horizon colour.
    pub sky_ground: [f32; 3],
    /// Ground-bottom colour.
    pub ground_bottom: [f32; 3],
    /// Procedural sky curve.
    pub sky_curve: f32,
    /// Exponential fog density.
    pub fog_density: f32,
    /// Fog colour.
    pub fog_color: [f32; 3],
    /// Fog light energy.
    pub fog_energy: f32,
    /// Aerial perspective amount.
    pub fog_aerial: f32,
    /// Ambient light energy.
    pub ambient_energy: f32,
    /// Sky energy multiplier.
    pub sky_energy: f32,
    /// 0 hides stars. 1 is a full night sky.
    pub stars: f32,
    /// What is falling.
    pub precipitation: PrecipKind,
    /// 0 is dry air. 1 is a heavy fall.
    pub precip_intensity: f32,
    /// 0 is the dry road. 1 is a glossy wet road.
    pub wetness: f32,
    /// 0 is off. 1 is the night headlight.
    pub headlight: f32,
}

/// Clock hour for `time` after `ride_s` seconds of riding.
#[must_use]
pub fn hour_at(time: TimeOfDay, ride_s: f32) -> f32 {
    let hour = match time {
        TimeOfDay::Auto => AUTO_START_HOUR + (ride_s / AUTO_DAY_SECONDS) * 24.0,
        TimeOfDay::Dawn => 7.1,
        TimeOfDay::Day => DAY_HOUR,
        TimeOfDay::Sunset => 16.9,
        TimeOfDay::Night => 22.5,
    };
    wrap_hour(hour)
}

/// Resolve `weather` at `ride_s`. A concrete weather stays put.
///
/// [`Weather::Auto`] walks a fixed cycle (clear, cloudy, rain, cloudy, snow,
/// fog) and blends across the last part of each step. `seed` rotates where
/// the cycle starts. The same seed and time always return the same mix.
#[must_use]
pub fn resolve_weather(weather: Weather, ride_s: f32, seed: u32) -> WeatherMix {
    if weather != Weather::Auto {
        let concrete = concrete(weather);
        return WeatherMix {
            from: concrete,
            to: concrete,
            t: 0.0,
        };
    }
    const CYCLE_S: f32 = 6.0 * 60.0;
    const STATES: [Weather; 6] = [
        Weather::Clear,
        Weather::Cloudy,
        Weather::Rain,
        Weather::Cloudy,
        Weather::Snow,
        Weather::Fog,
    ];
    let shift = (seed % STATES.len() as u32) as f32;
    let pos = (ride_s / CYCLE_S + shift).rem_euclid(STATES.len() as f32);
    let index = pos.floor() as usize % STATES.len();
    let frac = pos - index as f32;
    WeatherMix {
        from: STATES[index],
        to: STATES[(index + 1) % STATES.len()],
        t: smoothstep(0.55, 1.0, frac),
    }
}

/// Atmosphere for one theme, one hour, and one weather.
///
/// [`Weather::Auto`] is treated as clear. Use [`resolve_weather`] and
/// [`mix_atmosphere`] to drift. `theme_name == "desert"` turns snow into dust.
#[must_use]
pub fn atmosphere_at(theme_name: &str, hour: f32, weather: Weather) -> AtmosphereState {
    let hour = wrap_hour(hour);
    let look = look_for(theme_name);
    let elevation = sun_elevation_deg(hour) * look.elevation / 34.0;
    let azimuth = look.azimuth + azimuth_offset_deg(hour);
    let day_t = smoothstep(8.0, 26.0, elevation);
    let warm_t = smoothstep(-1.0, 7.0, elevation) * (1.0 - smoothstep(12.0, 28.0, elevation));
    let night_sun = [0.35, 0.42, 0.62];
    let warm_sun = if hour < 12.0 {
        [1.0, 0.74, 0.52]
    } else {
        [1.0, 0.60, 0.38]
    };
    let sun_color = lerp3(lerp3(night_sun, look.sun_color, day_t), warm_sun, warm_t);
    let sun_energy = look.sun_energy * smoothstep(-4.0, 18.0, elevation);
    let sky_t = smoothstep(-6.0, 14.0, elevation);
    let night_zenith = [0.015, 0.02, 0.05];
    let night_horizon = [0.04, 0.045, 0.08];
    let warm_horizon = if hour < 12.0 {
        [0.96, 0.62, 0.46]
    } else {
        [0.98, 0.46, 0.26]
    };
    let warm_ground = if hour < 12.0 {
        [0.78, 0.62, 0.48]
    } else {
        [0.82, 0.54, 0.38]
    };
    let warm_fog = if hour < 12.0 {
        [0.88, 0.68, 0.52]
    } else {
        [0.92, 0.58, 0.40]
    };
    let zenith = lerp3(night_zenith, look.zenith, sky_t);
    let horizon = lerp3(
        lerp3(night_horizon, look.horizon, sky_t),
        warm_horizon,
        warm_t * 0.9,
    );
    let ground = lerp3(
        lerp3([0.03, 0.03, 0.045], look.ground, sky_t),
        warm_ground,
        warm_t * 0.55,
    );
    let ground_bottom = lerp3([0.01, 0.012, 0.02], look.ground_bottom, sky_t);
    let mut state = AtmosphereState {
        hour,
        sun_elevation_deg: elevation,
        sun_azimuth_deg: azimuth,
        sun_color,
        sun_energy,
        sky_zenith: zenith,
        sky_horizon: horizon,
        sky_ground: ground,
        ground_bottom,
        sky_curve: look.sky_curve,
        fog_density: look.fog_density * (1.0 + (1.0 - sky_t) * 1.5),
        fog_color: lerp3(
            lerp3([0.05, 0.06, 0.09], look.fog_color, sky_t),
            warm_fog,
            warm_t * 0.5,
        ),
        fog_energy: look.fog_energy,
        fog_aerial: look.fog_aerial,
        ambient_energy: lerp(0.16, look.ambient, sky_t),
        sky_energy: lerp(0.12, look.sky_energy, sky_t),
        stars: smoothstep(1.0, -12.0, elevation),
        precipitation: PrecipKind::None,
        precip_intensity: 0.0,
        wetness: 0.0,
        headlight: smoothstep(8.0, -2.0, elevation),
    };
    apply_weather(&mut state, concrete(weather), theme_name == "desert");
    state
}

/// Blend two atmospheres. Precipitation switches at the midpoint so rain and
/// snow are not drawn together.
#[must_use]
pub fn mix_atmosphere(from: &AtmosphereState, to: &AtmosphereState, t: f32) -> AtmosphereState {
    let t = t.clamp(0.0, 1.0);
    let mut mixed = AtmosphereState {
        hour: lerp(from.hour, to.hour, t),
        sun_elevation_deg: lerp(from.sun_elevation_deg, to.sun_elevation_deg, t),
        sun_azimuth_deg: lerp(from.sun_azimuth_deg, to.sun_azimuth_deg, t),
        sun_color: lerp3(from.sun_color, to.sun_color, t),
        sun_energy: lerp(from.sun_energy, to.sun_energy, t),
        sky_zenith: lerp3(from.sky_zenith, to.sky_zenith, t),
        sky_horizon: lerp3(from.sky_horizon, to.sky_horizon, t),
        sky_ground: lerp3(from.sky_ground, to.sky_ground, t),
        ground_bottom: lerp3(from.ground_bottom, to.ground_bottom, t),
        sky_curve: lerp(from.sky_curve, to.sky_curve, t),
        fog_density: lerp(from.fog_density, to.fog_density, t),
        fog_color: lerp3(from.fog_color, to.fog_color, t),
        fog_energy: lerp(from.fog_energy, to.fog_energy, t),
        fog_aerial: lerp(from.fog_aerial, to.fog_aerial, t),
        ambient_energy: lerp(from.ambient_energy, to.ambient_energy, t),
        sky_energy: lerp(from.sky_energy, to.sky_energy, t),
        stars: lerp(from.stars, to.stars, t),
        precipitation: PrecipKind::None,
        precip_intensity: 0.0,
        wetness: lerp(from.wetness, to.wetness, t),
        headlight: lerp(from.headlight, to.headlight, t),
    };
    if t < 0.5 {
        mixed.precipitation = from.precipitation;
        mixed.precip_intensity = from.precip_intensity * (1.0 - t * 2.0);
    } else {
        mixed.precipitation = to.precipitation;
        mixed.precip_intensity = to.precip_intensity * ((t - 0.5) * 2.0);
    }
    mixed
}

/// Hour, weather, and theme in one call. This is the entry the game should use.
#[must_use]
pub fn ride_atmosphere(
    theme_name: &str,
    time: TimeOfDay,
    weather: Weather,
    ride_s: f32,
    seed: u32,
) -> AtmosphereState {
    let hour = hour_at(time, ride_s);
    let mix = resolve_weather(weather, ride_s, seed);
    let from = atmosphere_at(theme_name, hour, mix.from);
    let to = atmosphere_at(theme_name, hour, mix.to);
    mix_atmosphere(&from, &to, mix.t)
}

fn concrete(weather: Weather) -> Weather {
    if weather == Weather::Auto {
        Weather::Clear
    } else {
        weather
    }
}

fn apply_weather(state: &mut AtmosphereState, weather: Weather, desert: bool) {
    match weather {
        Weather::Clear | Weather::Auto => {}
        Weather::Cloudy => {
            state.sun_energy *= 0.68;
            state.sky_energy *= 0.8;
            state.sky_zenith = lerp3(state.sky_zenith, [0.42, 0.48, 0.55], 0.45);
            state.sky_horizon = lerp3(state.sky_horizon, [0.62, 0.64, 0.66], 0.4);
            state.fog_density *= 2.4;
            state.fog_color = lerp3(state.fog_color, [0.62, 0.64, 0.68], 0.45);
            state.ambient_energy *= 0.85;
        }
        Weather::Rain => {
            state.sun_energy *= 0.42;
            state.sky_energy *= 0.7;
            state.sky_zenith = lerp3(state.sky_zenith, [0.22, 0.26, 0.32], 0.55);
            state.sky_horizon = lerp3(state.sky_horizon, [0.40, 0.44, 0.48], 0.5);
            state.sun_color = lerp3(state.sun_color, [0.72, 0.76, 0.80], 0.4);
            state.fog_density = state.fog_density * 5.0 + 0.00018;
            state.fog_color = lerp3(state.fog_color, [0.48, 0.52, 0.56], 0.55);
            state.ambient_energy *= 0.7;
            state.precipitation = PrecipKind::Rain;
            state.precip_intensity = 0.72;
            state.wetness = 0.78;
        }
        Weather::Snow if desert => {
            state.sun_energy *= 0.75;
            state.sky_zenith = lerp3(state.sky_zenith, [0.62, 0.52, 0.40], 0.25);
            state.sky_horizon = lerp3(state.sky_horizon, [0.78, 0.62, 0.42], 0.35);
            state.fog_density = state.fog_density * 4.0 + 0.00012;
            state.fog_color = lerp3(state.fog_color, [0.74, 0.60, 0.42], 0.5);
            state.precipitation = PrecipKind::Dust;
            state.precip_intensity = 0.5;
            state.wetness = 0.0;
        }
        Weather::Snow => {
            state.sun_energy *= 0.72;
            state.sky_energy *= 0.9;
            state.sky_zenith = lerp3(state.sky_zenith, [0.55, 0.62, 0.72], 0.35);
            state.sky_horizon = lerp3(state.sky_horizon, [0.78, 0.80, 0.84], 0.4);
            state.fog_density = state.fog_density * 4.5 + 0.00014;
            state.fog_color = lerp3(state.fog_color, [0.72, 0.76, 0.80], 0.45);
            state.precipitation = PrecipKind::Snow;
            state.precip_intensity = 0.68;
            state.wetness = 0.18;
        }
        Weather::Fog => {
            state.sun_energy *= 0.4;
            state.sky_energy *= 0.65;
            state.sky_zenith = lerp3(state.sky_zenith, state.fog_color, 0.55);
            state.sky_horizon = lerp3(state.sky_horizon, [0.70, 0.72, 0.74], 0.6);
            state.fog_density = 0.0011;
            state.fog_color = lerp3(state.fog_color, [0.70, 0.72, 0.74], 0.7);
            state.fog_aerial = 0.45;
            state.ambient_energy *= 0.8;
            state.wetness = 0.08;
        }
    }
}

struct Look {
    zenith: [f32; 3],
    horizon: [f32; 3],
    ground: [f32; 3],
    ground_bottom: [f32; 3],
    sky_energy: f32,
    sky_curve: f32,
    sun_color: [f32; 3],
    sun_energy: f32,
    azimuth: f32,
    elevation: f32,
    fog_color: [f32; 3],
    fog_density: f32,
    fog_energy: f32,
    fog_aerial: f32,
    ambient: f32,
}

fn look_for(theme: &str) -> Look {
    if theme == "desert" {
        Look {
            zenith: [0.40, 0.58, 0.88],
            horizon: [0.96, 0.74, 0.48],
            ground: [0.74, 0.56, 0.36],
            ground_bottom: [0.42, 0.28, 0.16],
            sky_energy: 1.06,
            sky_curve: 0.18,
            sun_color: [1.0, 0.90, 0.72],
            sun_energy: 1.32,
            azimuth: -48.0,
            elevation: 32.0,
            fog_color: [0.86, 0.72, 0.52],
            fog_density: 0.000055,
            fog_energy: 0.62,
            fog_aerial: 0.16,
            ambient: 0.85,
        }
    } else {
        Look {
            zenith: [0.16, 0.34, 0.78],
            horizon: [0.93, 0.78, 0.62],
            ground: [0.45, 0.50, 0.42],
            ground_bottom: [0.22, 0.26, 0.22],
            sky_energy: 1.08,
            sky_curve: 0.14,
            sun_color: [1.0, 0.95, 0.86],
            sun_energy: 1.25,
            azimuth: -62.0,
            elevation: 34.0,
            fog_color: [0.64, 0.74, 0.84],
            fog_density: 0.00004,
            fog_energy: 0.55,
            fog_aerial: 0.16,
            ambient: 0.85,
        }
    }
}

fn sun_elevation_deg(hour: f32) -> f32 {
    let angle = (hour - 6.0) / 12.0 * std::f32::consts::PI;
    angle.sin() * 34.0
}

fn azimuth_offset_deg(hour: f32) -> f32 {
    let angle = (hour - 12.0) / 12.0 * std::f32::consts::PI;
    angle.sin() * 75.0
}

fn wrap_hour(hour: f32) -> f32 {
    if !hour.is_finite() {
        return DAY_HOUR;
    }
    let mut wrapped = hour % 24.0;
    if wrapped < 0.0 {
        wrapped += 24.0;
    }
    wrapped
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        lerp(a[0], b[0], t),
        lerp(a[1], b[1], t),
        lerp(a[2], b[2], t),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.02
    }

    #[test]
    fn auto_starts_in_the_morning_and_loops_a_day_every_half_hour() {
        assert!(close(hour_at(TimeOfDay::Auto, 0.0), 8.0));
        assert!(close(
            hour_at(TimeOfDay::Auto, AUTO_DAY_SECONDS * 0.5),
            20.0
        ));
        assert!(close(hour_at(TimeOfDay::Auto, AUTO_DAY_SECONDS), 8.0));
        assert!(close(hour_at(TimeOfDay::Day, 999.0), DAY_HOUR));
    }

    #[test]
    fn clear_noon_matches_the_built_in_themes() {
        let alpine = atmosphere_at("alpine", DAY_HOUR, Weather::Clear);
        assert!(close(alpine.sun_elevation_deg, 34.0));
        assert!(close(alpine.sun_azimuth_deg, -62.0));
        assert!(close(alpine.sun_energy, 1.25));
        assert!(close(alpine.sky_zenith[2], 0.78));
        assert!(close(alpine.fog_density, 0.00004));
        assert!(close(alpine.ambient_energy, 0.85));
        assert!(close(alpine.sky_energy, 1.08));
        assert_eq!(alpine.precipitation, PrecipKind::None);
        assert!(alpine.headlight < 0.02);
        assert!(alpine.stars < 0.02);
        assert!(alpine.wetness < 0.02);

        let desert = atmosphere_at("desert", DAY_HOUR, Weather::Clear);
        assert!(close(desert.sun_elevation_deg, 32.0));
        assert!(close(desert.sun_azimuth_deg, -48.0));
        assert!(close(desert.sun_energy, 1.32));
        assert!(close(desert.sky_horizon[0], 0.96));
    }

    #[test]
    fn night_lights_the_headlight_and_dawn_stays_warm() {
        let night = atmosphere_at("alpine", hour_at(TimeOfDay::Night, 0.0), Weather::Clear);
        assert!(night.headlight > 0.85);
        assert!(night.stars > 0.85);
        assert!(night.sun_energy < 0.2);
        assert!(night.sun_elevation_deg < -10.0);

        let dawn = atmosphere_at("alpine", hour_at(TimeOfDay::Dawn, 0.0), Weather::Clear);
        let sunset = atmosphere_at("alpine", hour_at(TimeOfDay::Sunset, 0.0), Weather::Clear);
        assert!(dawn.sun_color[0] > dawn.sun_color[2]);
        assert!(sunset.sun_color[0] > sunset.sun_color[2]);
        assert!(dawn.sun_elevation_deg > 0.0 && dawn.sun_elevation_deg < 20.0);
        assert!(sunset.sun_elevation_deg > 0.0 && sunset.sun_elevation_deg < 20.0);
        assert!(dawn.headlight < 0.15);
    }

    #[test]
    fn rain_wets_the_road_and_desert_snow_is_dust() {
        let rain = atmosphere_at("alpine", DAY_HOUR, Weather::Rain);
        assert_eq!(rain.precipitation, PrecipKind::Rain);
        assert!(rain.precip_intensity > 0.5);
        assert!(rain.wetness > 0.6);
        assert!(rain.sun_energy < 1.0);

        let snow = atmosphere_at("alpine", DAY_HOUR, Weather::Snow);
        assert_eq!(snow.precipitation, PrecipKind::Snow);
        let dust = atmosphere_at("desert", DAY_HOUR, Weather::Snow);
        assert_eq!(dust.precipitation, PrecipKind::Dust);
        assert!(dust.wetness < 0.02);
    }

    #[test]
    fn neighbouring_hours_do_not_pop() {
        let mut hour = 0.0;
        let mut previous = atmosphere_at("alpine", hour, Weather::Clear);
        while hour < 24.0 {
            hour += 0.1;
            let next = atmosphere_at("alpine", hour, Weather::Clear);
            for channel in 0..3 {
                let jump = (next.sky_zenith[channel] - previous.sky_zenith[channel]).abs();
                assert!(jump < 0.12, "zenith jumped {jump} at hour {hour}");
                let sun = (next.sun_color[channel] - previous.sun_color[channel]).abs();
                assert!(sun < 0.12, "sun jumped {sun} at hour {hour}");
            }
            let elev = (next.sun_elevation_deg - previous.sun_elevation_deg).abs();
            assert!(elev < 1.2, "elevation jumped {elev} at hour {hour}");
            previous = next;
        }
    }

    #[test]
    fn auto_weather_is_stable_and_eventually_changes() {
        let start = resolve_weather(Weather::Auto, 0.0, 3);
        let again = resolve_weather(Weather::Auto, 0.0, 3);
        assert_eq!(start.from, again.from);
        assert!(close(start.t, again.t));
        assert_ne!(start.from, Weather::Auto);
        let later = resolve_weather(Weather::Auto, 6.0 * 60.0, 3);
        assert_ne!(later.from, start.from);
        let rain = resolve_weather(Weather::Rain, 500.0, 9);
        assert_eq!(rain.from, Weather::Rain);
        assert!(close(rain.t, 0.0));
    }
}
