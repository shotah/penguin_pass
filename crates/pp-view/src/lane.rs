//! Right-hand lane. The bike sits beside the centre line, never on it.
//!
//! [`SLOTS`] slots, evenly spaced from just clear of the centre line to just
//! clear of the edge line. A tap moves one slot. The offset eases over
//! [`MOVE_S`].

/// How long a slot change takes, in seconds.
pub const MOVE_S: f32 = 0.3;

/// A bike's half-width, in metres, kept clear of the painted lines.
const BODY_M: f32 = 0.4;

/// How many positions the rider can hold across the lane.
pub const SLOTS: u8 = 5;

/// The slot a ride starts in: the middle of the lane.
pub const START_SLOT: u8 = SLOTS / 2;

/// Where the lane sits, in metres to the rider's right of the centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneSpec {
    /// Just clear of the centre line.
    pub inner_m: f32,
    /// Middle of the right lane.
    pub middle_m: f32,
    /// Just clear of the edge line.
    pub outer_m: f32,
}

impl LaneSpec {
    /// Right lane of a road.
    ///
    /// `asphalt_half_m` is half the paved width. `center_half_m` is half the
    /// centre line. `edge_m` is the edge line, measured from the centre.
    /// The slots stay on the asphalt and never cross the centre line.
    #[must_use]
    pub fn right_lane(asphalt_half_m: f32, center_half_m: f32, edge_m: f32) -> Self {
        let inner = (center_half_m + BODY_M).max(0.2);
        let paved = asphalt_half_m.max(inner + 0.4);
        let outer = (edge_m - BODY_M).clamp(inner + 0.4, paved - 0.05);
        let middle = (inner + outer) * 0.5;
        Self {
            inner_m: inner,
            middle_m: middle,
            outer_m: outer,
        }
    }

    /// Metres right of centre for a slot. 0 is the centre side.
    #[must_use]
    pub fn offset_for_slot(self, slot: u8) -> f32 {
        self.offset(slot)
    }

    /// Arcade slot fraction, 0 at the centre side and 4 at the edge.
    #[must_use]
    pub fn slot_fraction(self, offset_m: f32) -> f32 {
        let span = (self.outer_m - self.inner_m).max(0.001);
        ((offset_m - self.inner_m) / span).clamp(0.0, 1.0) * f32::from(SLOTS - 1)
    }

    fn offset(self, slot: u8) -> f32 {
        let last = SLOTS - 1;
        let t = f32::from(slot.min(last)) / f32::from(last);
        self.inner_m + (self.outer_m - self.inner_m) * t
    }
}

/// One frame of the lane move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LanePose {
    /// Metres to the rider's right of the road centre.
    pub offset_m: f32,
    /// Lean into the scoot, radians. Positive rolls to the rider's right.
    pub lean_rad: f32,
    /// Slot index. 0 is nearest the centre line, `SLOTS - 1` the edge.
    pub slot: u8,
}

/// Slot and the ease toward it.
#[derive(Clone, Debug)]
pub struct Lane {
    spec: LaneSpec,
    slot: u8,
    from_m: f32,
    to_m: f32,
    elapsed: f32,
    last_m: f32,
}

impl Lane {
    /// Start in the middle of the right lane.
    #[must_use]
    pub fn new(spec: LaneSpec) -> Self {
        let middle = spec.middle_m;
        Self {
            spec,
            slot: START_SLOT,
            from_m: middle,
            to_m: middle,
            elapsed: MOVE_S,
            last_m: middle,
        }
    }

    /// One slot toward the centre line (`false`) or the edge (`true`).
    ///
    /// A tap at the end of the lane does nothing.
    pub fn nudge(&mut self, toward_edge: bool) {
        let next = if toward_edge {
            self.slot.saturating_add(1).min(SLOTS - 1)
        } else {
            self.slot.saturating_sub(1)
        };
        if next == self.slot {
            return;
        }
        self.from_m = self.offset_now();
        self.slot = next;
        self.to_m = self.spec.offset(next);
        self.elapsed = 0.0;
    }

    /// Advance the ease. `dt` is seconds.
    pub fn step(&mut self, dt: f32) -> LanePose {
        let dt = dt.max(0.0);
        self.elapsed = (self.elapsed + dt).min(MOVE_S);
        let offset = self.offset_now();
        let lean = if dt > 1.0e-4 {
            ((offset - self.last_m) / dt * 0.12).clamp(-0.2, 0.2)
        } else {
            0.0
        };
        self.last_m = offset;
        LanePose {
            offset_m: offset,
            lean_rad: lean,
            slot: self.slot,
        }
    }

    /// The lane the rider is easing across.
    #[must_use]
    pub fn spec(&self) -> LaneSpec {
        self.spec
    }

    /// Metres right of centre, including a move that is still easing.
    #[must_use]
    pub fn offset_m(&self) -> f32 {
        self.offset_now()
    }

    fn offset_now(&self) -> f32 {
        let t = (self.elapsed / MOVE_S).clamp(0.0, 1.0);
        let s = t * t * (3.0 - 2.0 * t);
        self.from_m + (self.to_m - self.from_m) * s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> LaneSpec {
        // 6 m asphalt, centre dash 0.072 m half-width, edge line at 2.7 m.
        LaneSpec::right_lane(3.0, 0.072, 2.7)
    }

    #[test]
    fn the_middle_slot_starts_between_the_lines() {
        let lane = Lane::new(spec());
        let pose = lane_at_rest();
        assert_eq!(pose.slot, START_SLOT);
        assert!(pose.offset_m > spec().inner_m);
        assert!(pose.offset_m < spec().outer_m);
        assert!((pose.offset_m - spec().middle_m).abs() < 0.001);
        assert!(pose.lean_rad.abs() < 0.001);
        let _ = lane;
    }

    fn lane_at_rest() -> LanePose {
        let mut lane = Lane::new(spec());
        lane.step(0.0)
    }

    #[test]
    fn taps_visit_every_slot_and_stop_at_the_lines() {
        let mut lane = Lane::new(spec());
        let mut last_m = settle(&mut lane).offset_m;
        for slot in START_SLOT + 1..SLOTS {
            lane.nudge(true);
            let pose = settle(&mut lane);
            assert_eq!(pose.slot, slot);
            assert!(pose.offset_m > last_m + 0.3, "slots too close");
            last_m = pose.offset_m;
        }
        let edge = settle(&mut lane);
        assert_eq!(edge.slot, SLOTS - 1);
        assert!((edge.offset_m - spec().outer_m).abs() < 0.001);
        lane.nudge(true);
        let still = settle(&mut lane);
        assert_eq!(still.slot, SLOTS - 1);
        assert!((still.offset_m - edge.offset_m).abs() < 0.001);
        for _ in 0..SLOTS - 1 {
            lane.nudge(false);
        }
        let inner = settle(&mut lane);
        assert_eq!(inner.slot, 0);
        assert!((inner.offset_m - spec().inner_m).abs() < 0.001);
        assert!(inner.offset_m > 0.072);
        lane.nudge(false);
        assert_eq!(settle(&mut lane).slot, 0);
    }

    #[test]
    fn the_move_eases_and_leans_then_stands_up() {
        let mut lane = Lane::new(spec());
        let start = lane.offset_m();
        lane.nudge(true);
        let early = lane.step(0.05);
        assert!(early.offset_m > start);
        assert!(early.offset_m < spec().outer_m);
        assert!(early.lean_rad > 0.0);
        let done = settle(&mut lane);
        assert!(done.lean_rad.abs() < 0.02);
        assert_eq!(done.slot, START_SLOT + 1);
        assert!(done.offset_m > start && done.offset_m < spec().outer_m);
    }

    fn settle(lane: &mut Lane) -> LanePose {
        let mut pose = lane.step(0.0);
        for _ in 0..10 {
            pose = lane.step(0.05);
        }
        pose
    }
}
