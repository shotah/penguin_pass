//! HUD strings. Telemetry and the session status go in; text comes out.

use pp_core::Telemetry;

use crate::format::{
    format_cadence_rpm, format_distance_km, format_elapsed, format_elevation_m, format_grade_pct,
    format_heart_rate, format_lap, format_power_w, format_speed_kph, grade_rgb,
};

/// Status line while the keyboard rider is the source and the hint has gone.
pub const SIM_STATUS: &str = "Simulated rider — Up/Down to change power";

const CONNECT_HINT: &str = "Press Esc or Start to connect a trainer";

/// One frame of HUD text.
#[derive(Clone, Debug, PartialEq)]
pub struct HudText {
    /// Watts.
    pub power: String,
    /// km/h.
    pub speed: String,
    /// rpm, or an em dash.
    pub cadence: String,
    /// bpm, or an em dash.
    pub heart: String,
    /// Signed grade.
    pub grade: String,
    /// Grade ink.
    pub grade_rgb: (f32, f32, f32),
    /// Kilometres.
    pub distance: String,
    /// Lap number.
    pub lap: String,
    /// `h:mm:ss`.
    pub elapsed: String,
    /// Metres.
    pub elevation: String,
    /// Bottom status line.
    pub status: String,
    /// TV slot. Empty means the HUD prints a dash.
    pub tv: String,
}

/// Build the HUD strings for one snapshot.
pub fn hud_text(
    telemetry: &Telemetry,
    status_line: &str,
    warning: &str,
    never_paired: bool,
    tv_url: &str,
) -> HudText {
    HudText {
        power: format_power_w(telemetry.power_w),
        speed: format_speed_kph(telemetry.speed_kph()),
        cadence: format_cadence_rpm(telemetry.cadence_rpm),
        heart: format_heart_rate(telemetry.heart_rate_bpm),
        grade: format_grade_pct(telemetry.grade_pct),
        grade_rgb: grade_rgb(telemetry.grade_pct),
        distance: format_distance_km(telemetry.distance_m),
        lap: format_lap(telemetry.lap),
        elapsed: format_elapsed(telemetry.elapsed_s),
        elevation: format_elevation_m(telemetry.elevation_m),
        status: status_sentence(telemetry.elapsed_s, status_line, warning, never_paired),
        tv: tv_url.to_owned(),
    }
}

/// Bottom line: a settings warning, the connect hint, the keyboard hint, or the trainer.
pub fn status_sentence(
    elapsed_s: f64,
    status_line: &str,
    warning: &str,
    never_paired: bool,
) -> String {
    if !warning.is_empty() && elapsed_s < 10.0 {
        return warning.to_owned();
    }
    if status_line == "Simulated rider" && never_paired && elapsed_s < 8.0 {
        return CONNECT_HINT.to_owned();
    }
    if status_line == "Simulated rider" {
        return SIM_STATUS.to_owned();
    }
    status_line.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_warning_and_the_connect_hint_take_turns() {
        assert_eq!(
            status_sentence(1.0, "Simulated rider", "could not parse", true),
            "could not parse"
        );
        assert_eq!(
            status_sentence(11.0, "Simulated rider", "could not parse", true),
            SIM_STATUS
        );
        assert_eq!(
            status_sentence(9.0, "Simulated rider", "", true),
            SIM_STATUS
        );
        assert_eq!(
            status_sentence(3.0, "Simulated rider", "", true),
            CONNECT_HINT
        );
        assert_eq!(
            status_sentence(1.0, "Fake Neo 2T: connected", "", true),
            "Fake Neo 2T: connected"
        );
    }

    #[test]
    fn a_snapshot_fills_every_slot() {
        let telemetry = Telemetry {
            power_w: 210.0,
            cadence_rpm: Some(88.0),
            heart_rate_bpm: Some(142),
            grade_pct: 4.2,
            distance_m: 250.0,
            elapsed_s: 65.0,
            elevation_m: 12.2,
            lap: 1,
            ..Telemetry::default()
        };
        let text = hud_text(
            &telemetry,
            "Simulated rider",
            "",
            false,
            "http://127.0.0.1:9/",
        );
        assert_eq!(text.power, "210");
        assert_eq!(text.cadence, "88");
        assert_eq!(text.heart, "142");
        assert_eq!(text.distance, "0.25");
        assert_eq!(text.elapsed, "0:01:05");
        assert_eq!(text.tv, "http://127.0.0.1:9/");
        assert_eq!(text.status, SIM_STATUS);
    }
}
