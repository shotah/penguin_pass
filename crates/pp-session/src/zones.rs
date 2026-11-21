//! Coggan power zones used by the HUD ladder and the TV page.

/// One training zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerZone {
    /// Zone number, from 1 to 7.
    pub index: u8,
    /// Short name, such as `Recovery` or `Sprint`.
    pub name: &'static str,
}

/// Zone for `power_w` given a functional threshold power.
///
/// The boundaries are Andrew Coggan's seven levels, as stated in
/// `docs/ui.md`: a power is in the highest zone whose lower bound it
/// exceeds. Z2 starts above 55 % of FTP, Z3 above 75 %, Z4 above 90 %,
/// Z5 above 105 %, Z6 above 120 %, and Z7 above 150 %. An exact boundary
/// stays in the lower zone: 55 % of FTP is still Z1, and 75 % is still Z2.
/// The table's "56 to 75 %" wording is that same rule rounded to whole
/// percents.
///
/// A non-finite power, a negative power, or an FTP of 0 is Z1. Zero watts
/// is Z1; the HUD hides the ladder itself when the rider is coasting.
#[must_use]
pub fn power_zone(power_w: f32, ftp_w: u16) -> PowerZone {
    let ratio = if ftp_w == 0 || !power_w.is_finite() || power_w <= 0.0 {
        0.0
    } else {
        power_w / f32::from(ftp_w)
    };
    const ZONES: [(f32, u8, &str); 7] = [
        (1.50, 7, "Sprint"),
        (1.20, 6, "Anaerobic"),
        (1.05, 5, "VO2 max"),
        (0.90, 4, "Threshold"),
        (0.75, 3, "Tempo"),
        (0.55, 2, "Endurance"),
        (0.0, 1, "Recovery"),
    ];
    for (bound, index, name) in ZONES {
        if ratio > bound {
            return PowerZone { index, name };
        }
    }
    PowerZone {
        index: 1,
        name: "Recovery",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_stay_in_the_lower_zone() {
        let ftp = 200_u16;
        let cases = [
            (0.0, 1, "Recovery"),
            (110.0, 1, "Recovery"),
            (111.0, 2, "Endurance"),
            (150.0, 2, "Endurance"),
            (151.0, 3, "Tempo"),
            (180.0, 3, "Tempo"),
            (181.0, 4, "Threshold"),
            (210.0, 4, "Threshold"),
            (211.0, 5, "VO2 max"),
            (240.0, 5, "VO2 max"),
            (241.0, 6, "Anaerobic"),
            (300.0, 6, "Anaerobic"),
            (301.0, 7, "Sprint"),
            (900.0, 7, "Sprint"),
        ];
        for (power, index, name) in cases {
            let zone = power_zone(power, ftp);
            assert_eq!(zone.index, index, "{power} W");
            assert_eq!(zone.name, name, "{power} W");
        }
        assert_eq!(power_zone(f32::NAN, ftp).index, 1);
        assert_eq!(power_zone(-5.0, ftp).index, 1);
        assert_eq!(power_zone(500.0, 0).index, 1);
    }
}
