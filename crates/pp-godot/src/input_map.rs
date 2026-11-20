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
