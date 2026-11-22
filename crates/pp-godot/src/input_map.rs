//! Keyboard and Steam Deck bindings.
//!
//! Installed from code so a headless run and a player build share one map.
//! Steam Deck controls are a gamepad: D-pad, triggers, Y, and Start.

use godot::classes::{InputEventJoypadButton, InputEventJoypadMotion, InputEventKey, InputMap};
use godot::global::{JoyAxis, JoyButton, Key};
use godot::obj::Gd;
use godot::prelude::*;

/// Ensure the ride actions exist and carry our events.
pub fn install() {
    let mut map = InputMap::singleton();
    bind_keys(
        &mut map,
        "power_up",
        &[Key::UP, Key::W],
        &[JoyButton::DPAD_UP],
        Some(JoyAxis::TRIGGER_RIGHT),
    );
    bind_keys(
        &mut map,
        "power_down",
        &[Key::DOWN, Key::S],
        &[JoyButton::DPAD_DOWN],
        Some(JoyAxis::TRIGGER_LEFT),
    );
    bind_keys(&mut map, "toggle_camera", &[Key::C], &[JoyButton::Y], None);
    bind_keys(&mut map, "toggle_fullscreen", &[Key::F11], &[], None);
    bind_keys(
        &mut map,
        "pause_menu",
        &[Key::ESCAPE],
        &[JoyButton::START],
        None,
    );
    bind_keys(
        &mut map,
        "menu_up",
        &[Key::UP, Key::W],
        &[JoyButton::DPAD_UP],
        None,
    );
    // Left stick up is the negative half of the same axis.
    let mut stick_up = InputEventJoypadMotion::new_gd();
    stick_up.set_axis(JoyAxis::LEFT_Y);
    stick_up.set_axis_value(-1.0);
    map.action_add_event("menu_up", &stick_up);
    bind_keys(
        &mut map,
        "menu_down",
        &[Key::DOWN, Key::S],
        &[JoyButton::DPAD_DOWN],
        None,
    );
    let mut stick_down = InputEventJoypadMotion::new_gd();
    stick_down.set_axis(JoyAxis::LEFT_Y);
    stick_down.set_axis_value(1.0);
    map.action_add_event("menu_down", &stick_down);
    bind_keys(
        &mut map,
        "menu_left",
        &[Key::LEFT, Key::A],
        &[JoyButton::DPAD_LEFT],
        None,
    );
    let mut stick_left = InputEventJoypadMotion::new_gd();
    stick_left.set_axis(JoyAxis::LEFT_X);
    stick_left.set_axis_value(-1.0);
    map.action_add_event("menu_left", &stick_left);
    bind_keys(
        &mut map,
        "menu_right",
        &[Key::RIGHT, Key::D],
        &[JoyButton::DPAD_RIGHT],
        None,
    );
    let mut stick_right = InputEventJoypadMotion::new_gd();
    stick_right.set_axis(JoyAxis::LEFT_X);
    stick_right.set_axis_value(1.0);
    map.action_add_event("menu_right", &stick_right);
    bind_keys(
        &mut map,
        "menu_accept",
        &[Key::ENTER, Key::KP_ENTER, Key::SPACE],
        &[JoyButton::A],
        None,
    );
    bind_keys(&mut map, "menu_back", &[Key::ESCAPE], &[JoyButton::B], None);
    bind_keys(
        &mut map,
        "lane_left",
        &[Key::LEFT, Key::A],
        &[JoyButton::DPAD_LEFT],
        None,
    );
    let mut lane_stick_left = InputEventJoypadMotion::new_gd();
    lane_stick_left.set_axis(JoyAxis::LEFT_X);
    lane_stick_left.set_axis_value(-1.0);
    map.action_add_event("lane_left", &lane_stick_left);
    bind_keys(
        &mut map,
        "lane_right",
        &[Key::RIGHT, Key::D],
        &[JoyButton::DPAD_RIGHT],
        None,
    );
    let mut lane_stick_right = InputEventJoypadMotion::new_gd();
    lane_stick_right.set_axis(JoyAxis::LEFT_X);
    lane_stick_right.set_axis_value(1.0);
    map.action_add_event("lane_right", &lane_stick_right);
    bind_keys(
        &mut map,
        "activate_powerup",
        &[Key::SPACE, Key::E],
        &[JoyButton::X],
        None,
    );
    bind_keys(&mut map, "workout_easier", &[Key::BRACKETLEFT], &[], None);
    bind_keys(&mut map, "workout_harder", &[Key::BRACKETRIGHT], &[], None);
    bind_keys(
        &mut map,
        "music_next",
        &[Key::N],
        &[JoyButton::RIGHT_SHOULDER],
        None,
    );
    bind_keys(&mut map, "gear_down", &[Key::COMMA], &[], None);
    bind_keys(&mut map, "gear_up", &[Key::PERIOD], &[], None);
}

fn bind_keys(
    map: &mut Gd<InputMap>,
    action: &str,
    keys: &[Key],
    buttons: &[JoyButton],
    trigger: Option<JoyAxis>,
) {
    if map.has_action(action) {
        map.action_erase_events(action);
    } else {
        map.add_action(action);
    }
    // Triggers are analog. Half a pull counts; a resting trigger does not.
    map.action_set_deadzone(action, 0.5);
    for key in keys {
        let mut event = InputEventKey::new_gd();
        event.set_physical_keycode(*key);
        map.action_add_event(action, &event);
    }
    for button in buttons {
        let mut event = InputEventJoypadButton::new_gd();
        event.set_button_index(*button);
        map.action_add_event(action, &event);
    }
    if let Some(axis) = trigger {
        let mut event = InputEventJoypadMotion::new_gd();
        event.set_axis(axis);
        event.set_axis_value(1.0);
        map.action_add_event(action, &event);
    }
}
