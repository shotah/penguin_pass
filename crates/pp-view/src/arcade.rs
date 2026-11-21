//! Text for fish, segments, the ghost, powerups, and badges.

/// Fish counter beside the lane hint. A combo of 1 is just the total.
#[must_use]
pub fn fish_counter(count: u32, combo: u32) -> String {
    if combo > 1 {
        format!("{count}  x{combo}")
    } else {
        format!("{count}")
    }
}

/// `m:ss` for a segment clock. Absurd values are an em dash.
#[must_use]
pub fn segment_clock(seconds: f64) -> String {
    let Some(whole) = sane_seconds(seconds) else {
        return "—".to_owned();
    };
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// Split against the best. Positive `split_s` means the rider is ahead.
///
/// `None` is the first attempt. The bool is true when the rider is ahead.
#[must_use]
pub fn split_text(split_s: Option<f64>) -> (String, bool) {
    let Some(split) = split_s else {
        return ("first try".to_owned(), true);
    };
    if !split.is_finite() {
        return ("—".to_owned(), true);
    }
    let ahead = split >= 0.0;
    let text = format!(
        "{}{}",
        if ahead { "-" } else { "+" },
        segment_clock(split.abs())
    );
    (text, ahead)
}

/// Ghost chip. Positive gap means the rider is ahead.
#[must_use]
pub fn ghost_chip(gap_m: f32, gap_s: f32) -> String {
    if gap_m.is_finite() && gap_m.abs() >= 8.0 {
        let sign = if gap_m >= 0.0 { "+" } else { "−" };
        format!("Ghost  {sign}{:.0} m", gap_m.abs())
    } else if gap_s.is_finite() {
        let sign = if gap_s >= 0.0 { "+" } else { "−" };
        format!("Ghost  {sign}{:.0} s", gap_s.abs())
    } else {
        "Ghost".to_owned()
    }
}

/// Name drawn on the powerup slot.
#[must_use]
pub fn powerup_name(kind: &str) -> &'static str {
    match kind {
        "feather" | "Feather" => "Feather",
        "aero" | "Aero" => "Aero",
        "burst" | "Burst" => "Burst",
        _ => "Powerup",
    }
}

/// `current / target` for a badge that is still open.
#[must_use]
pub fn badge_progress(current: f64, target: f64) -> String {
    if !current.is_finite() || !target.is_finite() || target <= 0.0 {
        return "—".to_owned();
    }
    let frac = (current / target).clamp(0.0, 1.0);
    format!("{:.0}%", frac * 100.0)
}

fn sane_seconds(seconds: f64) -> Option<u64> {
    if !seconds.is_finite() || seconds < 0.0 || seconds > 60.0 * 60.0 * 24.0 {
        return None;
    }
    Some(seconds.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fish_counter_shows_a_combo_only_after_the_first() {
        assert_eq!(fish_counter(3, 1), "3");
        assert_eq!(fish_counter(12, 4), "12  x4");
    }

    #[test]
    fn a_split_ahead_is_green_and_a_first_try_has_no_gap() {
        assert_eq!(split_text(Some(5.0)), ("-0:05".to_owned(), true));
        assert_eq!(split_text(Some(-3.0)), ("+0:03".to_owned(), false));
        assert_eq!(split_text(None), ("first try".to_owned(), true));
        assert_eq!(segment_clock(12.0 * 60.0 + 41.0), "12:41");
    }

    #[test]
    fn ghost_and_badge_text_stay_plain() {
        assert_eq!(ghost_chip(12.4, 1.0), "Ghost  +12 m");
        assert_eq!(ghost_chip(-2.0, -3.2), "Ghost  −3 s");
        assert_eq!(powerup_name("Aero"), "Aero");
        assert_eq!(badge_progress(25.0, 100.0), "25%");
        assert_eq!(badge_progress(f64::NAN, 10.0), "—");
    }
}
