//! Zwift controller events mapped onto the ride and the menus.
//!
//! A paddle sample and the shift button it also produces must not both
//! change power. The caller passes whether this batch already used a paddle.

use pp_controllers::{Button, ControllerEvent, PaddleAxis, RIDE_PADDLE_PRESS, Side};

/// What the game should do with one event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadAction {
    /// One slot toward the centre line.
    LaneLeft,
    /// One slot toward the edge line.
    LaneRight,
    /// Simulated power up, or a repeat while held.
    PowerUp,
    /// Simulated power down, or a repeat while held.
    PowerDown,
    /// Chase camera or handlebars.
    Camera,
    /// Next music track.
    NextTrack,
    /// Open or close the pause menu.
    Pause,
    /// Menu highlight up.
    MenuUp,
    /// Menu highlight down.
    MenuDown,
    /// Menu highlight left.
    MenuLeft,
    /// Menu highlight right.
    MenuRight,
    /// Confirm the highlighted row.
    Accept,
    /// Leave the current menu.
    Back,
}

/// Map one controller event.
///
/// `riding` is false while a menu is open. `skip_shift` is true when this
/// batch already applied a paddle, so the matching shift button is ignored.
#[must_use]
pub fn map_event(event: &ControllerEvent, riding: bool, skip_shift: bool) -> Option<PadAction> {
    match event {
        ControllerEvent::Pressed { button, .. } => {
            if skip_shift && is_shift(*button) {
                return None;
            }
            map_button(*button, riding)
        }
        ControllerEvent::Paddle {
            side, axis, raw, ..
        } => {
            if !riding || *axis != PaddleAxis::Lateral || raw.abs() < RIDE_PADDLE_PRESS {
                return None;
            }
            Some(if *side == Side::Right {
                PadAction::PowerUp
            } else {
                PadAction::PowerDown
            })
        }
        _ => None,
    }
}

/// True when `events` already contains a paddle pull that changes power.
#[must_use]
pub fn batch_has_paddle(events: &[ControllerEvent]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            ControllerEvent::Paddle {
                axis: PaddleAxis::Lateral,
                raw,
                ..
            } if raw.abs() >= RIDE_PADDLE_PRESS
        )
    })
}

fn is_shift(button: Button) -> bool {
    matches!(
        button,
        Button::ShiftUp
            | Button::ShiftDown
            | Button::ShiftUpLeft
            | Button::ShiftDownLeft
            | Button::ShiftUpRight
            | Button::ShiftDownRight
            | Button::PowerUpLeft
            | Button::PowerUpRight
            | Button::Plus
            | Button::Minus
    )
}

fn map_button(button: Button, riding: bool) -> Option<PadAction> {
    if riding {
        return Some(match button {
            Button::Left => PadAction::LaneLeft,
            Button::Right => PadAction::LaneRight,
            Button::Up => PadAction::PowerUp,
            Button::Down => PadAction::PowerDown,
            Button::Y => PadAction::Camera,
            Button::Z => PadAction::NextTrack,
            Button::OnOff | Button::OnOffLeft | Button::OnOffRight => PadAction::Pause,
            Button::Plus
            | Button::ShiftUp
            | Button::ShiftUpRight
            | Button::ShiftDownRight
            | Button::PowerUpRight => PadAction::PowerUp,
            Button::Minus
            | Button::ShiftDown
            | Button::ShiftDownLeft
            | Button::ShiftUpLeft
            | Button::PowerUpLeft => PadAction::PowerDown,
            Button::A | Button::B => return None,
        });
    }
    Some(match button {
        Button::Up => PadAction::MenuUp,
        Button::Down => PadAction::MenuDown,
        Button::Left => PadAction::MenuLeft,
        Button::Right => PadAction::MenuRight,
        Button::A => PadAction::Accept,
        Button::B => PadAction::Back,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pp_controllers::ControllerEvent;

    fn press(button: Button) -> ControllerEvent {
        ControllerEvent::Pressed {
            id: "AA".to_owned(),
            side: Some(Side::Left),
            button,
        }
    }

    #[test]
    fn riding_and_menus_use_the_pad_differently() {
        assert_eq!(
            map_event(&press(Button::Left), true, false),
            Some(PadAction::LaneLeft)
        );
        assert_eq!(
            map_event(&press(Button::Left), false, false),
            Some(PadAction::MenuLeft)
        );
        assert_eq!(
            map_event(&press(Button::Y), true, false),
            Some(PadAction::Camera)
        );
        assert_eq!(map_event(&press(Button::Y), false, false), None);
        assert_eq!(
            map_event(&press(Button::A), false, false),
            Some(PadAction::Accept)
        );
        assert_eq!(map_event(&press(Button::A), true, false), None);
        assert_eq!(
            map_event(&press(Button::OnOff), true, false),
            Some(PadAction::Pause)
        );
    }

    #[test]
    fn a_paddle_suppresses_the_shift_button() {
        let paddle = ControllerEvent::Paddle {
            id: "AA".to_owned(),
            side: Side::Right,
            axis: PaddleAxis::Lateral,
            raw: RIDE_PADDLE_PRESS,
        };
        let batch = [paddle.clone(), press(Button::ShiftUp)];
        assert!(batch_has_paddle(&batch));
        assert_eq!(map_event(&paddle, true, false), Some(PadAction::PowerUp));
        assert_eq!(map_event(&press(Button::ShiftUp), true, true), None);
        assert_eq!(
            map_event(&press(Button::ShiftUp), true, false),
            Some(PadAction::PowerUp)
        );
    }
}
