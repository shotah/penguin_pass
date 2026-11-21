//! Aerodynamic draft as a CdA multiplier.
//!
//! A rider directly behind another keeps about three quarters of their drag
//! area. The saving is gone by roughly twelve metres, or by two slots of
//! lateral offset. See `docs/bots.md` for where those numbers come from.

/// CdA multiplier on the wheel, from 0 m to [`DRAFT_NEAR_M`] directly behind.
///
/// 0.73 is a 27% saving. Measured pacelines sit around a quarter to a third
/// off the drag of the rider in front; this picks the middle of that range.
pub const DRAFT_CDA: f32 = 0.73;

/// Gap, in metres, out to which the inline draft stays at [`DRAFT_CDA`].
pub const DRAFT_NEAR_M: f64 = 3.0;

/// Gap, in metres, at which the draft has faded to 1.
pub const DRAFT_FAR_M: f64 = 12.0;

/// Slot offset at which the draft has faded to 1.
///
/// One slot is the neighbouring line. Two slots is about a metre toward the
/// gutter or the centre line, and the wheel in front no longer shelters you.
pub const DRAFT_SLOT_FADE: f32 = 2.0;

/// CdA multiplier for a rider `gap_m` behind another, `slot_offset` slots to one side.
///
/// `gap_m` is the leader's odometer minus the follower's. Zero and negative
/// mean the follower is level or ahead, and the factor is 1. `slot_offset`
/// is an absolute difference of eased lane fractions. The result is in
/// `DRAFT_CDA..=1.0` for a finite, sensible gap, and 1 for anything else.
///
/// Several leaders do not stack. Call this once per leader and keep the
/// smallest factor: you draft the rider who shelters you most, not all of
/// them at once.
#[must_use]
pub fn draft(gap_m: f64, slot_offset: f32) -> f32 {
    if !gap_m.is_finite() || gap_m <= 0.0 || gap_m >= DRAFT_FAR_M {
        return 1.0;
    }
    let lateral = slot_offset.abs();
    if !lateral.is_finite() || lateral >= DRAFT_SLOT_FADE {
        return 1.0;
    }
    let along = if gap_m <= DRAFT_NEAR_M {
        0.0
    } else {
        ((gap_m - DRAFT_NEAR_M) / (DRAFT_FAR_M - DRAFT_NEAR_M)) as f32
    };
    let along = along.clamp(0.0, 1.0);
    let smooth = along * along * (3.0 - 2.0 * along);
    let inline = DRAFT_CDA + (1.0 - DRAFT_CDA) * smooth;
    let weight = 1.0 - lateral / DRAFT_SLOT_FADE;
    1.0 - (1.0 - inline) * weight
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_draft_is_in_the_published_band() {
        for gap in [0.5, 1.0, 2.0, 3.0] {
            let factor = draft(gap, 0.0);
            assert!((0.70..=0.75).contains(&factor), "gap {gap} m gave {factor}");
        }
        assert!((draft(1.0, 0.0) - DRAFT_CDA).abs() < 1.0e-6);
        assert!((draft(3.0, 0.0) - DRAFT_CDA).abs() < 1.0e-6);
    }

    #[test]
    fn draft_fades_with_gap_and_with_a_slot() {
        let close = draft(2.0, 0.0);
        let mid = draft(7.5, 0.0);
        assert!(mid > close && mid < 1.0, "mid-gap {mid}");
        assert!((draft(12.0, 0.0) - 1.0).abs() < 1.0e-6);
        assert!((draft(30.0, 0.0) - 1.0).abs() < 1.0e-6);
        assert!((draft(0.0, 0.0) - 1.0).abs() < 1.0e-6);
        assert!((draft(-4.0, 0.0) - 1.0).abs() < 1.0e-6);
        let beside = draft(2.0, 1.0);
        assert!(beside > close && beside < 1.0, "one slot over {beside}");
        assert!((draft(2.0, 2.0) - 1.0).abs() < 1.0e-6);
        assert!(draft(2.0, 0.0) < draft(2.0, 1.0));
        assert!(draft(4.0, 0.0) > close);
        assert!(draft(4.0, 0.0) < 1.0);
    }

    #[test]
    fn a_non_finite_gap_does_not_draft() {
        assert_eq!(draft(f64::NAN, 0.0), 1.0);
        assert_eq!(draft(2.0, f32::NAN), 1.0);
    }
}
