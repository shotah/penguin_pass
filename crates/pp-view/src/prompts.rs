//! Button names for the device the rider used last.

/// Which device the prompts should name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Keyboard and mouse.
    #[default]
    Keyboard,
    /// A gamepad or a Steam Deck.
    Gamepad,
    /// A Zwift Play, Click, or Ride.
    Zwift,
}

/// Parse `--input-mode`. Unknown text is `None`.
#[must_use]
pub fn parse_input_mode(text: &str) -> Option<InputMode> {
    match text.trim().to_ascii_lowercase().as_str() {
        "keyboard" | "keys" => Some(InputMode::Keyboard),
        "gamepad" | "pad" | "deck" => Some(InputMode::Gamepad),
        "zwift" => Some(InputMode::Zwift),
        _ => None,
    }
}

/// The ride chips. Powerup and the workout nudge are empty when they do not apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RidePrompts {
    /// Power chip.
    pub power: String,
    /// Lane chip.
    pub lane: String,
    /// Menu chip.
    pub menu: String,
    /// Shown only while a powerup is held.
    pub powerup: String,
    /// Shown only while a workout is running.
    pub workout: String,
}

/// Prompts for one mode.
#[must_use]
pub fn ride_prompts(mode: InputMode, powerup_held: bool, workout: bool) -> RidePrompts {
    let (power, lane, powerup, menu, workout_hint) = match mode {
        InputMode::Keyboard => (
            "[Up/Down] Power ±25 W",
            "[A/D] Lane",
            "[Space] Powerup",
            "[Esc] Menu",
            "[ / ] Intensity",
        ),
        InputMode::Gamepad => (
            "[LT/RT] Power",
            "[D-Pad] Lane",
            "[X] Powerup",
            "[Start] Menu",
            "Pause menu",
        ),
        InputMode::Zwift => (
            "[+/-] Power",
            "[Left/Right] Lane",
            "[A] Powerup",
            "[B] Menu",
            "Pause menu",
        ),
    };
    RidePrompts {
        power: power.to_owned(),
        lane: lane.to_owned(),
        menu: menu.to_owned(),
        powerup: if powerup_held {
            powerup.to_owned()
        } else {
            String::new()
        },
        workout: if workout {
            workout_hint.to_owned()
        } else {
            String::new()
        },
    }
}

/// Footer under a menu.
#[must_use]
pub fn menu_footer(mode: InputMode) -> String {
    match mode {
        InputMode::Keyboard => "Arrows Move · Enter Select · Esc Back".to_owned(),
        InputMode::Gamepad => "D-Pad Move · A Select · B Back".to_owned(),
        InputMode::Zwift => "Arrows Move · A Select · B Back".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_device_names_its_own_buttons() {
        let keys = ride_prompts(InputMode::Keyboard, true, true);
        assert_eq!(keys.power, "[Up/Down] Power ±25 W");
        assert_eq!(keys.lane, "[A/D] Lane");
        assert_eq!(keys.powerup, "[Space] Powerup");
        assert_eq!(keys.menu, "[Esc] Menu");
        assert_eq!(keys.workout, "[ / ] Intensity");
        let pad = ride_prompts(InputMode::Gamepad, false, true);
        assert_eq!(pad.power, "[LT/RT] Power");
        assert_eq!(pad.lane, "[D-Pad] Lane");
        assert!(pad.powerup.is_empty());
        assert_eq!(pad.menu, "[Start] Menu");
        assert_eq!(pad.workout, "Pause menu");
        let zwift = ride_prompts(InputMode::Zwift, true, false);
        assert_eq!(zwift.power, "[+/-] Power");
        assert_eq!(zwift.lane, "[Left/Right] Lane");
        assert_eq!(zwift.powerup, "[A] Powerup");
        assert_eq!(zwift.menu, "[B] Menu");
        assert!(zwift.workout.is_empty());
    }

    #[test]
    fn menu_footers_and_the_flag() {
        assert_eq!(
            menu_footer(InputMode::Keyboard),
            "Arrows Move · Enter Select · Esc Back"
        );
        assert_eq!(
            menu_footer(InputMode::Gamepad),
            "D-Pad Move · A Select · B Back"
        );
        assert_eq!(
            menu_footer(InputMode::Zwift),
            "Arrows Move · A Select · B Back"
        );
        assert_eq!(parse_input_mode("Gamepad"), Some(InputMode::Gamepad));
        assert_eq!(parse_input_mode("deck"), Some(InputMode::Gamepad));
        assert_eq!(parse_input_mode("nope"), None);
    }
}
