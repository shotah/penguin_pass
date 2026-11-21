//! Ride-console strings, zone, and trainer chip. No Godot types.

use std::fmt::Write;

use pp_session::{
    ActiveSource, ConnectionStatus, LinkPhase, ResistanceMode, SIM_POWER_STEP_W, power_zone,
};

use crate::format::MISSING;

/// Trainer chip on the ride screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChipKind {
    /// Keyboard or gamepad rider.
    #[default]
    Simulated,
    /// Grade commands are being sent.
    Controlled,
    /// The trainer is connected and only reporting.
    ReadingOnly,
    /// The link dropped and the session is bringing it back.
    Reconnecting,
    /// A scan or a connect attempt is in progress.
    Searching,
    /// The link failed and nothing is retrying.
    Failed,
}

impl ChipKind {
    /// Spinner states. The arc turns once per second.
    #[must_use]
    pub fn spins(self) -> bool {
        matches!(self, Self::Reconnecting | Self::Searching)
    }
}

/// Owned chip text. The strings keep their capacity across frames.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Chip {
    /// Which mark to draw.
    pub kind: ChipKind,
    /// Trainer name, or `Simulated rider`.
    pub name: String,
    /// State words. Empty for the simulated rider.
    pub detail: String,
}

/// One frame of console text. Fields keep their capacity across frames.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ConsoleModel {
    /// Instantaneous watts, for the smoke-test getter.
    pub power_instant: String,
    /// Hero digits. The 3-second average when one exists.
    pub power_hero: String,
    /// `0` means no ladder segment is lit.
    pub zone_index: u8,
    /// `Z6 ANAEROBIC`, `COASTING`, or `NO SIGNAL`.
    pub zone_label: String,
    /// Cadence digits, or an em dash.
    pub cadence: String,
    /// Heart-rate digits, or an em dash.
    pub heart: String,
    /// Speed. One decimal under 100 km/h.
    pub speed: String,
    /// Grade with a U+2212 minus, for the screen.
    pub grade: String,
    /// The same grade with an ASCII hyphen, for GDScript `float()`.
    pub grade_ascii: String,
    /// Ramp step `0..6` for the grade colour.
    pub grade_ramp: u8,
    /// `h:mm:ss`.
    pub elapsed: String,
    /// Kilometres ridden.
    pub distance: String,
    /// Whole metres.
    pub elevation: String,
    /// `LAP` on a loop, `LEFT` on a route that ends.
    pub last_caption: String,
    /// Lap number, or kilometres remaining.
    pub last_value: String,
    /// Empty for a lap, `km` for distance left.
    pub last_unit: String,
    /// Connection chip.
    pub chip: Chip,
}

/// Physics and sensor inputs for [`refresh`].
#[derive(Clone, Copy, Debug)]
pub struct ConsoleInput {
    /// Instant power. `None` is no signal, not 0 W.
    pub power_w: Option<f32>,
    /// Three-second average. `None` until a sample exists.
    pub power_3s_w: Option<f32>,
    /// Cadence. `None` is an em dash. `Some(0.0)` is coasting.
    pub cadence_rpm: Option<f32>,
    /// Heart rate. `None` is an em dash.
    pub heart_rate_bpm: Option<u8>,
    /// Speed in kilometres per hour.
    pub speed_kph: f32,
    /// Grade in percent. Positive is uphill.
    pub grade_pct: f32,
    /// Distance ridden, in metres.
    pub distance_m: f64,
    /// Elevation, in metres.
    pub elevation_m: f32,
    /// Elapsed ride time, in seconds.
    pub elapsed_s: f64,
    /// Lap number. `0` before the first metre is shown as lap 1.
    pub lap: u32,
    /// Functional threshold power, in watts.
    pub ftp_w: u16,
    /// Metres still to ride. Used when `is_loop` is false.
    pub distance_left_m: f64,
    /// Closed course. The last row is the lap; otherwise distance left.
    pub is_loop: bool,
}

/// Fill `model` from `input` and `chip`. Returns whether any string changed.
///
/// `scratch` is cleared and reused so a steady frame does not allocate.
pub fn refresh(
    model: &mut ConsoleModel,
    scratch: &mut String,
    input: &ConsoleInput,
    chip: &Chip,
) -> bool {
    let mut dirty = false;
    dirty |= write_power(&mut model.power_instant, scratch, input.power_w);
    let hero = match (input.power_w, input.power_3s_w) {
        (None, _) => None,
        (Some(instant), None) => Some(instant),
        (Some(_), Some(average)) => Some(average),
    };
    dirty |= write_power(&mut model.power_hero, scratch, hero);
    let zone = zone_index(hero, input.ftp_w);
    if model.zone_index != zone {
        model.zone_index = zone;
        dirty = true;
    }
    let label = zone_label(input.power_w, hero, input.ftp_w);
    dirty |= assign(&mut model.zone_label, label);
    dirty |= write_optional_count(&mut model.cadence, scratch, input.cadence_rpm);
    dirty |= write_heart(&mut model.heart, scratch, input.heart_rate_bpm);
    dirty |= write_speed(&mut model.speed, scratch, input.speed_kph);
    dirty |= write_grade(
        &mut model.grade,
        &mut model.grade_ascii,
        scratch,
        input.grade_pct,
    );
    let ramp = crate::profile::grade_ramp(input.grade_pct);
    if model.grade_ramp != ramp {
        model.grade_ramp = ramp;
        dirty = true;
    }
    dirty |= write_elapsed(&mut model.elapsed, scratch, input.elapsed_s);
    dirty |= write_km(
        &mut model.distance,
        scratch,
        input.distance_m / 1000.0,
        false,
    );
    dirty |= write_whole(&mut model.elevation, scratch, input.elevation_m);
    dirty |= write_last_row(model, scratch, input);
    dirty |= assign_chip(&mut model.chip, chip);
    dirty
}

/// Map a session link onto the chip, reusing `chip`'s string capacity.
pub fn write_connection(chip: &mut Chip, status: &ConnectionStatus) -> bool {
    let (kind, name, detail) = match &status.source {
        ActiveSource::Simulated => (ChipKind::Simulated, "Simulated rider", ""),
        ActiveSource::Trainer { name, .. } => {
            let shown = status.trainer_name.as_deref().unwrap_or(name.as_str());
            match status.phase {
                LinkPhase::Reconnecting => (ChipKind::Reconnecting, shown, "Reconnecting"),
                LinkPhase::Connected => match status.resistance {
                    ResistanceMode::ReadingOnly => (ChipKind::ReadingOnly, shown, "Reading only"),
                    ResistanceMode::Controlled => (ChipKind::Controlled, shown, "Resistance on"),
                },
                LinkPhase::Searching | LinkPhase::Connecting => {
                    (ChipKind::Searching, shown, "Searching")
                }
                LinkPhase::Failed => (ChipKind::Failed, shown, "Not connected"),
                LinkPhase::Idle => (ChipKind::Simulated, "Simulated rider", ""),
            }
        }
    };
    let mut dirty = false;
    if chip.kind != kind {
        chip.kind = kind;
        dirty = true;
    }
    dirty |= assign(&mut chip.name, name);
    dirty |= assign(&mut chip.detail, detail);
    dirty
}

/// Chip implied by the status sentence the game already shows.
///
/// Used until the HUD is given a [`ConnectionStatus`].
pub fn write_status_line(chip: &mut Chip, line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let (kind, name, detail) = if lower.contains("simulated")
        || lower.contains("connect a trainer")
        || lower.contains("change power")
    {
        (ChipKind::Simulated, "Simulated rider", "")
    } else if lower.contains("reconnect") {
        let trainer = line.split(':').next().unwrap_or(line).trim();
        (ChipKind::Reconnecting, trainer, "Reconnecting")
    } else if lower.contains("reading only")
        || lower.contains("read-only")
        || lower.contains("read only")
    {
        let trainer = line.split(':').next().unwrap_or(line).trim();
        (ChipKind::ReadingOnly, trainer, "Reading only")
    } else if lower.contains("not connected")
        || lower.contains("failed")
        || lower.contains("unavailable")
    {
        let trainer = line.split(':').next().unwrap_or(line).trim();
        (ChipKind::Failed, trainer, "Not connected")
    } else if let Some((trainer, _)) = line.split_once(':') {
        (ChipKind::Controlled, trainer.trim(), "Resistance on")
    } else if line.trim().is_empty() {
        (ChipKind::Simulated, "Simulated rider", "")
    } else {
        (ChipKind::Controlled, line.trim(), "")
    };
    let mut dirty = false;
    if chip.kind != kind {
        chip.kind = kind;
        dirty = true;
    }
    dirty |= assign(&mut chip.name, name);
    dirty |= assign(&mut chip.detail, detail);
    dirty
}

/// `±25 W`, from [`SIM_POWER_STEP_W`].
#[must_use]
pub fn power_hint() -> String {
    format!("±{SIM_POWER_STEP_W} W")
}

/// Ladder index. `0` is unlit: no signal, or coasting at 0 W.
#[must_use]
pub fn zone_index(power_w: Option<f32>, ftp_w: u16) -> u8 {
    match power_w {
        Some(power) if power > 0.5 => power_zone(power, ftp_w).index,
        _ => 0,
    }
}

fn zone_label(instant: Option<f32>, hero: Option<f32>, ftp_w: u16) -> &'static str {
    if instant.is_none() {
        return "NO SIGNAL";
    }
    match zone_index(hero, ftp_w) {
        1 => "Z1 RECOVERY",
        2 => "Z2 ENDURANCE",
        3 => "Z3 TEMPO",
        4 => "Z4 THRESHOLD",
        5 => "Z5 VO2 MAX",
        6 => "Z6 ANAEROBIC",
        7 => "Z7 SPRINT",
        _ => "COASTING",
    }
}

fn write_last_row(model: &mut ConsoleModel, scratch: &mut String, input: &ConsoleInput) -> bool {
    if input.is_loop {
        let mut dirty = assign(&mut model.last_caption, "LAP");
        dirty |= write_u32(&mut model.last_value, scratch, input.lap.max(1));
        dirty |= assign(&mut model.last_unit, "");
        dirty
    } else {
        let mut dirty = assign(&mut model.last_caption, "LEFT");
        dirty |= write_km(
            &mut model.last_value,
            scratch,
            input.distance_left_m / 1000.0,
            true,
        );
        dirty |= assign(&mut model.last_unit, "km");
        dirty
    }
}

fn write_power(slot: &mut String, scratch: &mut String, watts: Option<f32>) -> bool {
    scratch.clear();
    match watts {
        Some(watts) => {
            let shown = watts.round().clamp(0.0, 9999.0) as i32;
            let _ = write!(scratch, "{shown}");
        }
        None => scratch.push_str(MISSING),
    }
    assign_scratch(slot, scratch)
}

fn write_optional_count(slot: &mut String, scratch: &mut String, value: Option<f32>) -> bool {
    scratch.clear();
    match value {
        Some(value) => {
            let shown = value.round().max(0.0) as i32;
            let _ = write!(scratch, "{shown}");
        }
        None => scratch.push_str(MISSING),
    }
    assign_scratch(slot, scratch)
}

fn write_heart(slot: &mut String, scratch: &mut String, bpm: Option<u8>) -> bool {
    scratch.clear();
    match bpm {
        Some(bpm) => {
            let _ = write!(scratch, "{bpm}");
        }
        None => scratch.push_str(MISSING),
    }
    assign_scratch(slot, scratch)
}

fn write_speed(slot: &mut String, scratch: &mut String, kph: f32) -> bool {
    scratch.clear();
    let kph = if kph.is_finite() { kph.max(0.0) } else { 0.0 };
    if kph >= 99.95 {
        let _ = write!(scratch, "{}", kph.round() as i32);
    } else {
        let _ = write!(scratch, "{kph:.1}");
    }
    assign_scratch(slot, scratch)
}

fn write_grade(shown: &mut String, ascii: &mut String, scratch: &mut String, grade: f32) -> bool {
    let grade = if grade.is_finite() { grade } else { 0.0 };
    let (sign, ascii_sign, magnitude) = if grade.abs() < 0.05 {
        ("", "", 0.0)
    } else if grade > 0.0 {
        ("+", "+", grade)
    } else {
        ("\u{2212}", "-", -grade)
    };
    scratch.clear();
    let _ = write!(scratch, "{sign}{magnitude:.1}");
    let mut dirty = assign_scratch(shown, scratch);
    scratch.clear();
    let _ = write!(scratch, "{ascii_sign}{magnitude:.1}");
    dirty |= assign_scratch(ascii, scratch);
    dirty
}

fn write_elapsed(slot: &mut String, scratch: &mut String, elapsed_s: f64) -> bool {
    let total = elapsed_s.max(0.0).floor() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    scratch.clear();
    let _ = write!(scratch, "{hours}:{minutes:02}:{seconds:02}");
    assign_scratch(slot, scratch)
}

fn write_km(slot: &mut String, scratch: &mut String, km: f64, one_decimal: bool) -> bool {
    scratch.clear();
    let km = if km.is_finite() { km.max(0.0) } else { 0.0 };
    if one_decimal || km >= 99.995 {
        let _ = write!(scratch, "{km:.1}");
    } else {
        let _ = write!(scratch, "{km:.2}");
    }
    assign_scratch(slot, scratch)
}

fn write_whole(slot: &mut String, scratch: &mut String, metres: f32) -> bool {
    scratch.clear();
    let metres = if metres.is_finite() { metres } else { 0.0 };
    let _ = write!(scratch, "{}", metres.round() as i32);
    assign_scratch(slot, scratch)
}

fn write_u32(slot: &mut String, scratch: &mut String, value: u32) -> bool {
    scratch.clear();
    let _ = write!(scratch, "{value}");
    assign_scratch(slot, scratch)
}

fn assign(slot: &mut String, text: &str) -> bool {
    if slot == text {
        return false;
    }
    slot.clear();
    slot.push_str(text);
    true
}

fn assign_scratch(slot: &mut String, scratch: &str) -> bool {
    assign(slot, scratch)
}

fn assign_chip(slot: &mut Chip, chip: &Chip) -> bool {
    let mut dirty = false;
    if slot.kind != chip.kind {
        slot.kind = chip.kind;
        dirty = true;
    }
    dirty |= assign(&mut slot.name, &chip.name);
    dirty |= assign(&mut slot.detail, &chip.detail);
    dirty
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ConsoleInput {
        ConsoleInput {
            power_w: Some(200.0),
            power_3s_w: Some(200.0),
            cadence_rpm: Some(85.0),
            heart_rate_bpm: None,
            speed_kph: 28.04,
            grade_pct: 4.2,
            distance_m: 2100.0,
            elevation_m: 134.2,
            elapsed_s: 452.0,
            lap: 1,
            ftp_w: 200,
            distance_left_m: 2900.0,
            is_loop: true,
        }
    }

    fn simulated() -> Chip {
        Chip {
            kind: ChipKind::Simulated,
            name: "Simulated rider".to_owned(),
            detail: String::new(),
        }
    }

    #[test]
    fn a_steady_tempo_frame_formats_once() {
        let mut model = ConsoleModel::default();
        let mut scratch = String::new();
        assert!(refresh(&mut model, &mut scratch, &input(), &simulated()));
        assert_eq!(model.power_instant, "200");
        assert_eq!(model.power_hero, "200");
        assert_eq!(model.zone_index, 4);
        assert_eq!(model.zone_label, "Z4 THRESHOLD");
        assert_eq!(model.cadence, "85");
        assert_eq!(model.heart, "—");
        assert_eq!(model.speed, "28.0");
        assert_eq!(model.grade, "+4.2");
        assert_eq!(model.grade_ascii, "+4.2");
        assert_eq!(model.elapsed, "0:07:32");
        assert_eq!(model.distance, "2.10");
        assert_eq!(model.elevation, "134");
        assert_eq!(model.last_caption, "LAP");
        assert_eq!(model.last_value, "1");
        assert!(!refresh(&mut model, &mut scratch, &input(), &simulated()));
    }

    #[test]
    fn the_hero_uses_the_three_second_average_and_the_getter_does_not() {
        let mut sample = input();
        sample.power_w = Some(210.0);
        sample.power_3s_w = Some(201.4);
        let mut model = ConsoleModel::default();
        let mut scratch = String::new();
        refresh(&mut model, &mut scratch, &sample, &simulated());
        assert_eq!(model.power_instant, "210");
        assert_eq!(model.power_hero, "201");
    }

    #[test]
    fn coasting_is_zero_and_a_missing_sensor_is_a_dash() {
        let mut sample = input();
        sample.power_w = Some(0.0);
        sample.power_3s_w = Some(0.0);
        sample.cadence_rpm = Some(0.0);
        let mut model = ConsoleModel::default();
        let mut scratch = String::new();
        refresh(&mut model, &mut scratch, &sample, &simulated());
        assert_eq!(model.power_hero, "0");
        assert_eq!(model.zone_index, 0);
        assert_eq!(model.zone_label, "COASTING");
        assert_eq!(model.cadence, "0");

        sample.power_w = None;
        sample.power_3s_w = None;
        sample.cadence_rpm = None;
        sample.heart_rate_bpm = None;
        refresh(&mut model, &mut scratch, &sample, &simulated());
        assert_eq!(model.power_instant, "—");
        assert_eq!(model.power_hero, "—");
        assert_eq!(model.zone_label, "NO SIGNAL");
        assert_eq!(model.cadence, "—");
        assert_eq!(model.heart, "—");
    }

    #[test]
    fn sprint_descent_and_a_route_that_ends() {
        let mut sample = input();
        sample.power_w = Some(450.0);
        sample.power_3s_w = Some(450.0);
        sample.grade_pct = -9.84;
        sample.speed_kph = 102.2;
        sample.distance_m = 100_400.0;
        sample.is_loop = false;
        sample.distance_left_m = 12_400.0;
        sample.lap = 0;
        let mut model = ConsoleModel::default();
        let mut scratch = String::new();
        refresh(&mut model, &mut scratch, &sample, &simulated());
        assert_eq!(model.zone_index, 7);
        assert_eq!(model.zone_label, "Z7 SPRINT");
        assert_eq!(model.grade, "\u{2212}9.8");
        assert_eq!(model.grade_ascii, "-9.8");
        assert_eq!(model.speed, "102");
        assert_eq!(model.distance, "100.4");
        assert_eq!(model.last_caption, "LEFT");
        assert_eq!(model.last_value, "12.4");
        assert_eq!(model.last_unit, "km");
        assert_eq!(power_hint(), "±25 W");
    }

    #[test]
    fn the_status_line_and_the_link_pick_the_same_chip() {
        let mut chip = Chip::default();
        assert!(write_status_line(
            &mut chip,
            "Simulated rider — Up/Down to change power"
        ));
        assert_eq!(chip.kind, ChipKind::Simulated);
        assert_eq!(chip.name, "Simulated rider");
        assert!(!write_status_line(
            &mut chip,
            "Press Esc or Start to connect a trainer"
        ));

        assert!(write_status_line(
            &mut chip,
            "Fake Neo 2T: connected, resistance follows the road"
        ));
        assert_eq!(chip.kind, ChipKind::Controlled);
        assert_eq!(chip.name, "Fake Neo 2T");
        assert_eq!(chip.detail, "Resistance on");

        let status = ConnectionStatus {
            source: ActiveSource::Trainer {
                name: "Fake Neo 2T".to_owned(),
                state: pp_session::ConnectionState::Connected,
            },
            trainer_name: Some("Fake Neo 2T".to_owned()),
            phase: LinkPhase::Reconnecting,
            resistance: ResistanceMode::Controlled,
            bluetooth_available: true,
            failure_reason: None,
            failure_detail: None,
        };
        write_connection(&mut chip, &status);
        assert_eq!(chip.kind, ChipKind::Reconnecting);
        assert_eq!(chip.detail, "Reconnecting");
        assert!(chip.kind.spins());
    }
}
