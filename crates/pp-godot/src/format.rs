//! HUD formatting lives in `pp-view`. These names stay so the HUD can keep calling them.

pub use pp_view::format::{
    format_cadence_rpm, format_distance_km, format_elapsed, format_elevation_m, format_grade_pct,
    format_heart_rate, format_lap, format_power_w, format_speed_kph, grade_rgb,
};
