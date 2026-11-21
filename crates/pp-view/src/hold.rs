//! Hold-to-repeat timing shared by the menu and the keyboard power keys.

/// How long a direction must be held before it starts repeating.
pub const HOLD_DELAY_S: f64 = 0.35;

/// Repeating menu direction.
///
/// The first press is reported by the caller. This only answers "should the
/// held direction fire again this frame?".
#[derive(Clone, Debug, Default)]
pub struct MenuHold {
    held_s: f64,
}

impl MenuHold {
    /// `held` is true while a direction stays down. Returns whether to repeat.
    pub fn poll(&mut self, dt: f64, held: bool) -> bool {
        if !held {
            self.held_s = 0.0;
            return false;
        }
        let dt = dt.max(0.0);
        self.held_s += dt;
        if self.held_s < HOLD_DELAY_S {
            return false;
        }
        let interval = if self.held_s > 1.2 { 0.06 } else { 0.12 };
        self.held_s % interval < dt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_does_not_repeat_and_a_hold_does() {
        let mut hold = MenuHold::default();
        assert!(!hold.poll(0.05, true));
        assert!(!hold.poll(0.2, true));
        assert!(hold.held_s < HOLD_DELAY_S || !hold.poll(0.0, true));
        let mut fired = false;
        for _ in 0..20 {
            if hold.poll(0.05, true) {
                fired = true;
                break;
            }
        }
        assert!(fired);
        assert!(!hold.poll(0.05, false));
        assert_eq!(hold.held_s, 0.0);
    }
}
