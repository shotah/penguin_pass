//! HUD formatting.
//!
//! Plain functions with no Godot types, so they can move into a Godot-free
//! crate. `pp-godot` is a cdylib (`test = false`) and cannot host `cargo test`.

/// Em dash for a sensor that is not reporting. A real zero stays `"0"`.
pub const MISSING: &str = "—";

/// Whole watts, rounded.
pub fn format_power_w(watts: f32) -> String {
    format!("{}", watts.round() as i32)
}

/// One decimal, kilometers per hour.
pub fn format_speed_kph(kph: f32) -> String {
    format!("{kph:.1}")
}

/// Whole rpm, or [`MISSING`] when the sensor is absent.
pub fn format_cadence_rpm(rpm: Option<f32>) -> String {
    match rpm {
        Some(value) => format!("{}", value.round() as i32),
        None => MISSING.to_string(),
    }
}

/// Whole bpm, or [`MISSING`] when the sensor is absent.
pub fn format_heart_rate(bpm: Option<u8>) -> String {
    match bpm {
        Some(value) => format!("{value}"),
        None => MISSING.to_string(),
    }
}

/// Signed grade with one decimal. The percent sign is a separate unit label.
pub fn format_grade_pct(grade_pct: f32) -> String {
    format!("{grade_pct:+.1}")
}

/// Distance in kilometers, two decimals so a few meters show up on the HUD.
pub fn format_distance_km(distance_m: f64) -> String {
    let km = distance_m / 1000.0;
    format!("{km:.2}")
}

/// Whole meters of elevation.
pub fn format_elevation_m(elevation_m: f32) -> String {
    format!("{}", elevation_m.round() as i32)
}

/// Lap counter. Lap 1 is the first lap.
pub fn format_lap(lap: u32) -> String {
    format!("{lap}")
}

/// Elapsed ride time as `h:mm:ss`.
pub fn format_elapsed(elapsed_s: f64) -> String {
    let total = elapsed_s.max(0.0).floor() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    format!("{hours}:{minutes:02}:{seconds:02}")
}

/// Grade ink: blue on descents, green when flat, then yellow, orange, and red.
pub fn grade_rgb(grade_pct: f32) -> (f32, f32, f32) {
    if grade_pct <= -0.5 {
        (0.45, 0.74, 1.0)
    } else if grade_pct < 1.5 {
        (0.42, 0.86, 0.48)
    } else if grade_pct < 4.0 {
        (0.98, 0.86, 0.32)
    } else if grade_pct < 8.0 {
        (0.98, 0.62, 0.28)
    } else {
        (0.98, 0.38, 0.32)
    }
}
