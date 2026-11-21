//! Text for the sky, the bots, workouts, and Strava.

const TIMES: [&str; 5] = ["day", "auto", "dawn", "sunset", "night"];
const WEATHERS: [&str; 6] = ["clear", "auto", "cloudy", "rain", "snow", "fog"];
const BOT_COUNTS: [u8; 4] = [0, 4, 8, 12];

/// Label for a time-of-day id. Unknown ids read as Day.
#[must_use]
pub fn time_label(id: &str) -> &'static str {
    match id {
        "auto" => "Auto",
        "dawn" => "Dawn",
        "sunset" => "Sunset",
        "night" => "Night",
        _ => "Day",
    }
}

/// The next or previous time-of-day id.
#[must_use]
pub fn cycle_time(id: &str, next: bool) -> &'static str {
    step(&TIMES[..], id, next, "day")
}

/// Label for a weather id. Unknown ids read as Clear.
#[must_use]
pub fn weather_label(id: &str) -> &'static str {
    match id {
        "auto" => "Auto",
        "cloudy" => "Cloudy",
        "rain" => "Rain",
        "snow" => "Snow",
        "fog" => "Fog",
        _ => "Clear",
    }
}

/// The next or previous weather id.
#[must_use]
pub fn cycle_weather(id: &str, next: bool) -> &'static str {
    step(&WEATHERS[..], id, next, "clear")
}

/// `Off`, `4`, `8`, or `12`.
#[must_use]
pub fn bots_label(count: u8) -> String {
    if count == 0 {
        "Off".to_owned()
    } else {
        count.to_string()
    }
}

/// Step the other-riders row through Off, 4, 8, and 12.
#[must_use]
pub fn cycle_bots(count: u8, next: bool) -> u8 {
    let pos = BOT_COUNTS
        .iter()
        .position(|value| *value == snapped_bots(count))
        .unwrap_or(2);
    let next_pos = if next {
        (pos + 1) % BOT_COUNTS.len()
    } else {
        (pos + BOT_COUNTS.len() - 1) % BOT_COUNTS.len()
    };
    BOT_COUNTS[next_pos]
}

/// Map a stored count onto the row's four choices.
#[must_use]
pub fn snapped_bots(count: u8) -> u8 {
    BOT_COUNTS
        .iter()
        .copied()
        .min_by_key(|value| value.abs_diff(count))
        .unwrap_or(8)
}

/// `Draft -24%` when the saving is at least 5. Otherwise nothing.
#[must_use]
pub fn draft_chip(percent: u8) -> Option<String> {
    if percent < 5 {
        None
    } else {
        Some(format!("Draft -{percent}%"))
    }
}

/// Floating name. Pace riders lead with watts per kilogram.
#[must_use]
pub fn bot_tag(name: &str, pace_wkg: Option<f32>) -> String {
    match pace_wkg {
        Some(wkg) if wkg.is_finite() => format!("{wkg:.1} W/kg — {name}"),
        _ => name.to_owned(),
    }
}

/// `Sweet spot  2 of 3` when the plan has more than one step.
#[must_use]
pub fn workout_title(label: &str, index: usize, count: usize) -> String {
    if count > 1 {
        format!("{label}  {} of {count}", index.saturating_add(1))
    } else if label.is_empty() {
        "Workout".to_owned()
    } else {
        label.to_owned()
    }
}

/// `176 W`, or `Open` when the step has no target.
#[must_use]
pub fn workout_target(watts: Option<f32>) -> String {
    match watts {
        Some(watts) if watts.is_finite() => format!("{:.0} W", watts.round()),
        _ => "Open".to_owned(),
    }
}

/// `m:ss` left in a step. Absurd values are an em dash.
#[must_use]
pub fn workout_clock(seconds: f32) -> String {
    if !seconds.is_finite() || seconds < 0.0 || seconds > 24.0 * 60.0 * 60.0 {
        return "—".to_owned();
    }
    let whole = seconds.round() as u32;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// `+5%` or `-5%`. Zero is empty.
#[must_use]
pub fn bias_label(percent: i8) -> String {
    if percent == 0 {
        String::new()
    } else if percent > 0 {
        format!("+{percent}%")
    } else {
        format!("{percent}%")
    }
}

/// `Next  Spin-up  120 W`.
#[must_use]
pub fn next_interval(label: Option<&str>, watts: Option<f32>) -> String {
    let Some(label) = label.filter(|label| !label.is_empty()) else {
        return String::new();
    };
    match watts {
        Some(watts) if watts.is_finite() => format!("Next  {label}  {:.0} W", watts.round()),
        _ => format!("Next  {label}"),
    }
}

/// Planned length for the picker, such as `45 min`.
#[must_use]
pub fn workout_length(seconds: f32) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "—".to_owned();
    }
    let minutes = (seconds / 60.0).round().max(1.0) as u32;
    format!("{minutes} min")
}

/// Picker subtitle: `45 min · 58 TSS · IF 0.88 · 540 kJ`.
///
/// `tss`, `intensity_factor`, and `kj` come from the workout plan. `kj` is
/// the estimate at the rider's FTP.
#[must_use]
pub fn workout_metrics(duration_s: f32, tss: u16, intensity_factor: f32, kj: u16) -> String {
    let length = workout_length(duration_s);
    let factor = if intensity_factor.is_finite() {
        intensity_factor.max(0.0)
    } else {
        0.0
    };
    format!("{length} · {tss} TSS · IF {factor:.2} · {kj} kJ")
}

/// `72% in range  ·  186 W avg`.
#[must_use]
pub fn workout_result_line(in_range: f32, average_w: f32) -> String {
    let pct = if in_range.is_finite() {
        (in_range.clamp(0.0, 1.0) * 100.0).round() as u32
    } else {
        0
    };
    let watts = if average_w.is_finite() {
        average_w.round() as u32
    } else {
        0
    };
    format!("{pct}% in range    {watts} W avg")
}

/// What the Strava row should say.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum StravaRow {
    /// No login yet.
    Connect,
    /// The browser page is open.
    Waiting,
    /// A saved athlete.
    Connected(String),
    /// The last attempt failed.
    Failed(String),
}

/// Settings line for the Strava row.
#[must_use]
pub fn strava_line(row: &StravaRow) -> String {
    match row {
        StravaRow::Connect => "Strava    Connect Strava".to_owned(),
        StravaRow::Waiting => "Strava    Waiting for browser...".to_owned(),
        StravaRow::Connected(name) => format!("Strava    Connected: {name} (Disconnect)"),
        StravaRow::Failed(reason) => format!("Strava    {reason}"),
    }
}

fn step(items: &[&'static str], id: &str, next: bool, fallback: &'static str) -> &'static str {
    let pos = items.iter().position(|item| *item == id).unwrap_or(0);
    let next_pos = if next {
        (pos + 1) % items.len()
    } else {
        (pos + items.len() - 1) % items.len()
    };
    items.get(next_pos).copied().unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_and_weather_step_and_wrap() {
        assert_eq!(time_label("dawn"), "Dawn");
        assert_eq!(time_label("nope"), "Day");
        assert_eq!(cycle_time("night", true), "day");
        assert_eq!(cycle_time("day", false), "night");
        assert_eq!(weather_label("fog"), "Fog");
        assert_eq!(cycle_weather("fog", true), "clear");
        assert_eq!(cycle_weather("clear", false), "fog");
    }

    #[test]
    fn other_riders_use_four_stops_and_drafting_starts_at_five() {
        assert_eq!(bots_label(0), "Off");
        assert_eq!(bots_label(8), "8");
        assert_eq!(cycle_bots(8, true), 12);
        assert_eq!(cycle_bots(0, false), 12);
        assert_eq!(snapped_bots(6), 4);
        assert_eq!(snapped_bots(7), 8);
        assert_eq!(draft_chip(4), None);
        assert_eq!(draft_chip(24), Some("Draft -24%".to_owned()));
    }

    #[test]
    fn workout_and_strava_text_stay_plain() {
        assert_eq!(bot_tag("Maya", Some(2.2)), "2.2 W/kg — Maya");
        assert_eq!(bot_tag("Noah", None), "Noah");
        assert_eq!(workout_title("Sweet spot", 1, 3), "Sweet spot  2 of 3");
        assert_eq!(workout_target(Some(176.4)), "176 W");
        assert_eq!(workout_target(None), "Open");
        assert_eq!(workout_clock(65.0), "1:05");
        assert_eq!(bias_label(0), "");
        assert_eq!(bias_label(5), "+5%");
        assert_eq!(bias_label(-5), "-5%");
        assert_eq!(
            next_interval(Some("Spin-up"), Some(120.0)),
            "Next  Spin-up  120 W"
        );
        assert_eq!(workout_length(45.0 * 60.0), "45 min");
        assert_eq!(
            workout_metrics(45.0 * 60.0, 58, 0.88, 540),
            "45 min · 58 TSS · IF 0.88 · 540 kJ"
        );
        assert_eq!(
            workout_metrics(0.0, 0, f32::NAN, 0),
            "— · 0 TSS · IF 0.00 · 0 kJ"
        );
        assert_eq!(
            workout_result_line(0.72, 186.2),
            "72% in range    186 W avg"
        );
        assert_eq!(
            strava_line(&StravaRow::Connected("Ada".to_owned())),
            "Strava    Connected: Ada (Disconnect)"
        );
        assert_eq!(
            strava_line(&StravaRow::Waiting),
            "Strava    Waiting for browser..."
        );
    }
}
