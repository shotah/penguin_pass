//! Power zones and the gradient colour scale from the interface design.
//!
//! One table is served to the page at `/api/zones`. The bounds are Coggan's
//! seven levels: a power is in the highest zone whose lower bound it exceeds.

use serde::Serialize;

/// One step of the effort ramp, used both as a power zone and as a colour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ZoneStep {
    /// Zone number, 1 through 7. Zone 1 is at the bottom of the ladder.
    pub zone: u8,
    /// Short name, for example `Recovery`.
    pub name: &'static str,
    /// Caption drawn under the hero power, for example `Z6 ANAEROBIC`.
    pub label: &'static str,
    /// Power is in this zone when `power / ftp` is greater than this value
    /// and not greater than a higher zone's bound. Zone 1 uses `0.0`.
    pub above_ftp: f32,
    /// CSS colour for the ladder segment and the zone caption.
    pub color: &'static str,
}

/// One band of the road-gradient scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GradeBand {
    /// Grade, in percent, at which this band starts.
    ///
    /// A grade belongs to the last band whose `from_pct` it has reached.
    /// The first band is the floor for everything below the next band.
    pub from_pct: f32,
    /// Name of the band.
    pub name: &'static str,
    /// CSS colour for the profile and the grade number.
    pub color: &'static str,
}

const POWER_ZONES: [ZoneStep; 7] = [
    ZoneStep {
        zone: 1,
        name: "Recovery",
        label: "Z1 RECOVERY",
        above_ftp: 0.0,
        color: "#9AA8BD",
    },
    ZoneStep {
        zone: 2,
        name: "Endurance",
        label: "Z2 ENDURANCE",
        above_ftp: 0.55,
        color: "#4FA8FF",
    },
    ZoneStep {
        zone: 3,
        name: "Tempo",
        label: "Z3 TEMPO",
        above_ftp: 0.75,
        color: "#3DD68C",
    },
    ZoneStep {
        zone: 4,
        name: "Threshold",
        label: "Z4 THRESHOLD",
        above_ftp: 0.90,
        color: "#FFD166",
    },
    ZoneStep {
        zone: 5,
        name: "VO2 max",
        label: "Z5 VO2 MAX",
        above_ftp: 1.05,
        color: "#FF9A3C",
    },
    ZoneStep {
        zone: 6,
        name: "Anaerobic",
        label: "Z6 ANAEROBIC",
        above_ftp: 1.20,
        color: "#FF5A52",
    },
    ZoneStep {
        zone: 7,
        name: "Sprint",
        label: "Z7 SPRINT",
        above_ftp: 1.50,
        color: "#D277FF",
    },
];

const GRADE_BANDS: [GradeBand; 7] = [
    GradeBand {
        from_pct: -1000.0,
        name: "downhill",
        color: "#4FA8FF",
    },
    GradeBand {
        from_pct: -1.0,
        name: "flat",
        color: "#9AA8BD",
    },
    GradeBand {
        from_pct: 1.0,
        name: "gentle",
        color: "#3DD68C",
    },
    GradeBand {
        from_pct: 3.0,
        name: "steady",
        color: "#FFD166",
    },
    GradeBand {
        from_pct: 6.0,
        name: "hard",
        color: "#FF9A3C",
    },
    GradeBand {
        from_pct: 9.0,
        name: "steep",
        color: "#FF5A52",
    },
    GradeBand {
        from_pct: 12.0,
        name: "wall",
        color: "#D277FF",
    },
];

/// The seven power zones, zone 1 first.
#[must_use]
pub fn power_zones() -> &'static [ZoneStep] {
    &POWER_ZONES
}

/// The gradient scale, from downhill to the steepest climb.
#[must_use]
pub fn grade_bands() -> &'static [GradeBand] {
    &GRADE_BANDS
}

/// Zone for `power_w` at this FTP.
///
/// Returns `None` when FTP is zero, power is not finite, or power is at or
/// below zero (the page reads that as coasting, not zone 1).
#[must_use]
pub fn zone_for_power(power_w: f32, ftp_w: u16) -> Option<&'static ZoneStep> {
    if ftp_w == 0 || !power_w.is_finite() || power_w <= 0.0 {
        return None;
    }
    let ratio = power_w / f32::from(ftp_w);
    POWER_ZONES.iter().rev().find(|step| ratio > step.above_ftp)
}

/// Colour for a road grade, in percent.
#[must_use]
pub fn grade_color(grade_pct: f32) -> &'static str {
    let grade = if grade_pct.is_finite() {
        grade_pct
    } else {
        0.0
    };
    let mut color = GRADE_BANDS[0].color;
    for band in &GRADE_BANDS {
        if grade >= band.from_pct {
            color = band.color;
        }
    }
    color
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_follow_the_coggan_bounds() {
        let ftp = 200_u16;
        assert!(zone_for_power(0.0, ftp).is_none());
        assert_eq!(zone_for_power(110.0, ftp).map(|z| z.zone), Some(1));
        assert_eq!(zone_for_power(110.1, ftp).map(|z| z.zone), Some(2));
        assert_eq!(zone_for_power(150.0, ftp).map(|z| z.zone), Some(2));
        assert_eq!(zone_for_power(150.1, ftp).map(|z| z.zone), Some(3));
        assert_eq!(zone_for_power(180.0, ftp).map(|z| z.zone), Some(3));
        assert_eq!(zone_for_power(180.1, ftp).map(|z| z.zone), Some(4));
        assert_eq!(zone_for_power(210.0, ftp).map(|z| z.zone), Some(4));
        assert_eq!(zone_for_power(210.1, ftp).map(|z| z.zone), Some(5));
        assert_eq!(zone_for_power(240.0, ftp).map(|z| z.zone), Some(5));
        assert_eq!(zone_for_power(240.1, ftp).map(|z| z.zone), Some(6));
        assert_eq!(zone_for_power(300.0, ftp).map(|z| z.zone), Some(6));
        assert_eq!(zone_for_power(300.1, ftp).map(|z| z.zone), Some(7));
        assert!(zone_for_power(400.0, 0).is_none());
    }

    #[test]
    fn gradient_bands_match_the_design_scale() {
        assert_eq!(grade_color(-1.1), "#4FA8FF");
        assert_eq!(grade_color(-1.0), "#9AA8BD");
        assert_eq!(grade_color(0.9), "#9AA8BD");
        assert_eq!(grade_color(1.0), "#3DD68C");
        assert_eq!(grade_color(2.9), "#3DD68C");
        assert_eq!(grade_color(3.0), "#FFD166");
        assert_eq!(grade_color(6.0), "#FF9A3C");
        assert_eq!(grade_color(9.0), "#FF5A52");
        assert_eq!(grade_color(10.0), "#FF5A52");
        assert_eq!(grade_color(12.0), "#D277FF");
    }
}
