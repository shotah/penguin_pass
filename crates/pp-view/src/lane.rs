//! Full road plus the gravel right shoulder.
//!
//! [`SLOTS`] positions run from the left asphalt edge to the right shoulder.
//! Slots `0..=PAVED_SLOTS - 1` stay on the paved asphalt. The last slot is
//! the gravel right shoulder. A ride starts at [`START_SLOT`], the middle of
//! the right-hand lane. A tap moves one slot. The offset eases over
//! [`MOVE_S`].

/// How long a slot change takes, in seconds.
pub const MOVE_S: f32 = 0.3;

/// How many positions the rider can hold, from the left edge to the shoulder.
pub const SLOTS: u8 = 9;

/// Paved positions. Slots `0..=7` stay on the asphalt. Slot `8` is gravel.
pub const PAVED_SLOTS: u8 = 8;

/// The slot a ride starts in: the middle of the right-hand lane.
pub const START_SLOT: u8 = 6;

/// Where the rideable ribbon sits, in metres to the rider's right of centre.
///
/// Negative is the rider's left. The ribbon covers the full asphalt and the
/// gravel right shoulder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneSpec {
    /// Left edge of the rideable ribbon, just inside the left white line.
    pub inner_m: f32,
    /// Offset of [`START_SLOT`], the middle of the right-hand lane.
    pub middle_m: f32,
    /// Gravel right shoulder, past the right edge of the asphalt.
    pub outer_m: f32,
}

impl LaneSpec {
    /// Full road plus the gravel right shoulder.
    ///
    /// `asphalt_half_m` is half the paved width. `center_half_m` is half the
    /// centre line and `edge_m` is the edge line, both measured from the
    /// centre. The slots span the asphalt and the right shoulder; the painted
    /// lines are not a hard stop.
    #[must_use]
    pub fn right_lane(asphalt_half_m: f32, center_half_m: f32, edge_m: f32) -> Self {
        let _ = (center_half_m, edge_m);
        let inner = -(asphalt_half_m - 0.35).max(0.4);
        let outer = (asphalt_half_m + 0.35).max(inner + 0.4);
        let last = f32::from(SLOTS - 1).max(1.0);
        let middle = inner + (outer - inner) * (f32::from(START_SLOT.min(SLOTS - 1)) / last);
        Self {
            inner_m: inner,
            middle_m: middle,
            outer_m: outer,
        }
    }

    /// Metres right of centre for a slot. 0 is the left edge.
    #[must_use]
    pub fn offset_for_slot(self, slot: u8) -> f32 {
        self.offset(slot)
    }

    /// Metres right of centre for a fractional slot.
    ///
    /// `0.0` is the left asphalt edge and [`SLOTS`]`- 1` is the gravel right
    /// shoulder. Values outside that range clamp.
    #[must_use]
    pub fn offset_for_slot_fraction(self, slot: f32) -> f32 {
        let last = f32::from(SLOTS - 1);
        let t = if last > 0.0 {
            slot.clamp(0.0, last) / last
        } else {
            0.0
        };
        self.inner_m + (self.outer_m - self.inner_m) * t
    }

    /// Lane fraction for an offset in metres.
    ///
    /// `inner_m` maps to `0.0` and `outer_m` maps to [`SLOTS`]`- 1` (`8.0`
    /// on the full road). Offsets outside the ribbon clamp.
    #[must_use]
    pub fn slot_fraction(self, offset_m: f32) -> f32 {
        let span = (self.outer_m - self.inner_m).max(0.001);
        ((offset_m - self.inner_m) / span).clamp(0.0, 1.0) * f32::from(SLOTS - 1)
    }

    fn offset(self, slot: u8) -> f32 {
        self.offset_for_slot_fraction(f32::from(slot))
    }
}

/// One frame of the lane move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LanePose {
    /// Metres to the rider's right of the road centre.
    pub offset_m: f32,
    /// Lean into the scoot, radians. Positive rolls to the rider's right.
    pub lean_rad: f32,
    /// Slot index. 0 is the left asphalt edge, `SLOTS - 1` the right shoulder.
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

    /// One slot to the left (`false`) or the right (`true`).
    ///
    /// Right walks toward the gravel shoulder. A tap at either end does nothing.
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
    fn the_ride_starts_in_the_right_lane() {
        let mut lane = Lane::new(spec());
        let pose = lane.step(0.0);
        assert_eq!(pose.slot, START_SLOT);
        assert_eq!(START_SLOT, 6);
        assert!(pose.offset_m > 1.0 && pose.offset_m < 2.7);
        assert!((pose.offset_m - spec().middle_m).abs() < 0.001);
        assert!((spec().middle_m - spec().offset_for_slot(START_SLOT)).abs() < 0.001);
        assert!((spec().inner_m - -2.65).abs() < 0.001);
        assert!((spec().outer_m - 3.35).abs() < 0.001);
        assert!((pose.offset_m - 1.85).abs() < 0.001);
        assert!(pose.lean_rad.abs() < 0.001);
        let _ = lane;
    }

    #[test]
    fn nudging_right_reaches_the_edge_then_the_shoulder() {
        let mut lane = Lane::new(spec());
        lane.nudge(true);
        let edge = settle(&mut lane);
        assert_eq!(edge.slot, 7);
        assert!(
            edge.offset_m > 2.4 && edge.offset_m < 3.0,
            "slot 7 should sit near the right asphalt edge, got {}",
            edge.offset_m
        );
        lane.nudge(true);
        let shoulder = settle(&mut lane);
        assert_eq!(shoulder.slot, 8);
        assert!(
            shoulder.offset_m > 3.0,
            "slot 8 is the gravel shoulder, got {}",
            shoulder.offset_m
        );
        assert!((shoulder.offset_m - spec().outer_m).abs() < 0.001);
        lane.nudge(true);
        let still = settle(&mut lane);
        assert_eq!(still.slot, 8);
        assert!((still.offset_m - shoulder.offset_m).abs() < 0.001);
    }

    #[test]
    fn nudging_left_visits_every_slot_and_stops_at_the_left_edge() {
        let mut lane = Lane::new(spec());
        lane.nudge(true);
        lane.nudge(true);
        let mut pose = settle(&mut lane);
        assert_eq!(pose.slot, 8);
        for slot in (0..8).rev() {
            lane.nudge(false);
            pose = settle(&mut lane);
            assert_eq!(pose.slot, slot, "expected slot {slot}");
        }
        assert!(
            pose.offset_m < -2.0,
            "slot 0 should be near the left edge, got {}",
            pose.offset_m
        );
        assert!((pose.offset_m - spec().inner_m).abs() < 0.001);
        lane.nudge(false);
        assert_eq!(settle(&mut lane).slot, 0);
    }

    #[test]
    fn gravel_shoulder_slot_matches_lane_geometry() {
        assert_eq!(pp_core::GRAVEL_SHOULDER_SLOT, f32::from(PAVED_SLOTS) - 0.5);
    }

    #[test]
    fn fractional_slots_round_trip_across_the_road() {
        let spec = spec();
        assert_eq!(PAVED_SLOTS, 8);
        for slot in 0..=8 {
            let offset = spec.offset_for_slot_fraction(f32::from(slot));
            let back = spec.slot_fraction(offset);
            assert!(
                (back - f32::from(slot)).abs() < 1.0e-4,
                "slot {slot} -> {offset} -> {back}"
            );
            assert!((offset - spec.offset_for_slot(slot)).abs() < 1.0e-5);
        }
        assert!(spec.offset_for_slot(7) < 3.0, "slot 7 stays on the asphalt");
        assert!(spec.offset_for_slot(8) > 3.0, "slot 8 is gravel");
        let mid = spec.offset_for_slot_fraction(6.5);
        let fraction = spec.slot_fraction(mid);
        assert!((fraction - 6.5).abs() < 1.0e-4);
        assert!((spec.offset_for_slot_fraction(fraction) - mid).abs() < 1.0e-4);
        assert!((spec.offset_for_slot_fraction(-2.0) - spec.inner_m).abs() < 1.0e-5);
        assert!((spec.offset_for_slot_fraction(20.0) - spec.outer_m).abs() < 1.0e-5);
        assert!(spec.slot_fraction(spec.inner_m - 5.0).abs() < 1.0e-5);
        assert!((spec.slot_fraction(spec.outer_m + 5.0) - 8.0).abs() < 1.0e-5);
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
