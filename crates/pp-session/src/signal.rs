//! Signal bars for the pairing list.
//!
//! The Fitness Machine spec does not define bars. These thresholds are the
//! usual 10 dBm BLE proximity steps: -60 dBm is close, and -90 dBm is about
//! the edge of a reliable link.

/// RSSI, in dBm, at or above which the pairing list shows 4 bars (strong).
pub const SIGNAL_STRONG_DBM: i16 = -60;
/// RSSI, in dBm, at or above which the pairing list shows 3 bars (good).
pub const SIGNAL_GOOD_DBM: i16 = -70;
/// RSSI, in dBm, at or above which the pairing list shows 2 bars (fair).
pub const SIGNAL_FAIR_DBM: i16 = -80;
/// RSSI, in dBm, at or above which the pairing list shows 1 bar (weak).
pub const SIGNAL_WEAK_DBM: i16 = -90;

/// Map a received signal strength to 0..=4 bars.
///
/// `None` (the adapter did not report RSSI) is 0 bars, so an unknown signal
/// sorts with the weakest trainers instead of the strongest.
#[must_use]
pub fn signal_bars(rssi_dbm: Option<i16>) -> u8 {
    let Some(rssi) = rssi_dbm else {
        return 0;
    };
    if rssi >= SIGNAL_STRONG_DBM {
        4
    } else if rssi >= SIGNAL_GOOD_DBM {
        3
    } else if rssi >= SIGNAL_FAIR_DBM {
        2
    } else if rssi >= SIGNAL_WEAK_DBM {
        1
    } else {
        0
    }
}

/// Word that belongs next to [`signal_bars`], matching the pairing screen.
#[must_use]
pub fn signal_word(bars: u8) -> &'static str {
    match bars {
        4 => "Strong",
        3 => "Good",
        2 => "Fair",
        1 => "Weak",
        _ => "Very weak",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_follow_the_ten_dbm_steps_and_unknown_is_zero() {
        assert_eq!(signal_bars(None), 0);
        assert_eq!(signal_bars(Some(-91)), 0);
        assert_eq!(signal_bars(Some(-90)), 1);
        assert_eq!(signal_bars(Some(-80)), 2);
        assert_eq!(signal_bars(Some(-70)), 3);
        assert_eq!(signal_bars(Some(-60)), 4);
        assert_eq!(signal_bars(Some(-40)), 4);
        assert_eq!(signal_word(4), "Strong");
        assert_eq!(signal_word(0), "Very weak");
        assert_eq!(signal_word(9), "Very weak");
    }
}
