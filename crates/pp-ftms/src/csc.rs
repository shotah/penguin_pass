//! Cycling Speed and Cadence Measurement (0x2A5B).
//!
//! The flags octet is first. Bit 0 adds wheel revolution data (`uint32`
//! cumulative revolutions, then `uint16` last-event time in units of 1/1024
//! second). Bit 1 adds crank revolution data, the same two `uint16`s as
//! Cycling Power. Other flag bits are reserved and add no octets. Crank
//! cadence uses [`crate::crank_cadence_rpm`] on the crank field.

use crate::cycling_power::CrankRevolutionData;
use crate::error::FtmsError;
use crate::wire::Reader;

const WHEEL_REVOLUTION_DATA: u8 = 1 << 0;
const CRANK_REVOLUTION_DATA: u8 = 1 << 1;

/// Wheel revolution counters from one CSC Measurement.
///
/// Speed needs two of these. See [`wheel_speed_kph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WheelRevolutionData {
    /// Cumulative wheel revolutions. The counter wraps at `u32`.
    pub cumulative_revolutions: u32,
    /// Last wheel event time, in units of 1/1024 second. The counter wraps at `u16`.
    pub last_event_time_1024: u16,
}

/// One CSC Measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CscMeasurement {
    /// Wheel revolution data, when flag bit 0 is set.
    pub wheel: Option<WheelRevolutionData>,
    /// Crank revolution data, when flag bit 1 is set.
    ///
    /// The type is the Cycling Power crank record, so [`crate::crank_cadence_rpm`]
    /// applies unchanged.
    pub crank: Option<CrankRevolutionData>,
}

impl CscMeasurement {
    /// Parse one measurement. Short buffers return [`FtmsError::Truncated`].
    ///
    /// Trailing bytes are ignored. The function does not panic.
    pub fn parse(bytes: &[u8]) -> Result<Self, FtmsError> {
        let mut reader = Reader::new(bytes);
        let flags = reader.u8("csc flags")?;
        let wheel = if flags & WHEEL_REVOLUTION_DATA != 0 {
            Some(WheelRevolutionData {
                cumulative_revolutions: reader.u32("cumulative wheel revolutions")?,
                last_event_time_1024: reader.u16("last wheel event time")?,
            })
        } else {
            None
        };
        let crank = if flags & CRANK_REVOLUTION_DATA != 0 {
            Some(CrankRevolutionData {
                cumulative_revolutions: reader.u16("cumulative crank revolutions")?,
                last_event_time_1_1024_s: reader.u16("last crank event time")?,
            })
        } else {
            None
        };
        Ok(Self { wheel, crank })
    }
}

/// Speed, in kilometers per hour, between two wheel readings.
///
/// Both counters wrap: revolutions at `u32`, event time at `u16` (1/1024 s).
/// A zero time delta is not a usable rate and returns `None`, including when
/// the wheel is stopped. A non-finite or non-positive circumference, or a
/// non-finite result, returns `None`.
#[must_use]
pub fn wheel_speed_kph(
    previous: WheelRevolutionData,
    current: WheelRevolutionData,
    wheel_circumference_m: f32,
) -> Option<f32> {
    if !wheel_circumference_m.is_finite() || wheel_circumference_m <= 0.0 {
        return None;
    }
    let revolutions = current
        .cumulative_revolutions
        .wrapping_sub(previous.cumulative_revolutions);
    let ticks = current
        .last_event_time_1024
        .wrapping_sub(previous.last_event_time_1024);
    if ticks == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let speed_kph = revolutions as f32 * wheel_circumference_m * 1024.0 * 3.6 / f32::from(ticks);
    speed_kph.is_finite().then_some(speed_kph)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_and_crank_flags() {
        // Flags 0x03. Wheel revolutions 1, wheel time 2, crank revolutions 3,
        // crank time 4. All little-endian.
        let both = CscMeasurement::parse(&[
            0x03, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04, 0x00,
        ])
        .unwrap();
        assert_eq!(
            both.wheel,
            Some(WheelRevolutionData {
                cumulative_revolutions: 1,
                last_event_time_1024: 2,
            })
        );
        assert_eq!(
            both.crank,
            Some(CrankRevolutionData {
                cumulative_revolutions: 3,
                last_event_time_1_1024_s: 4,
            })
        );

        let wheel_only =
            CscMeasurement::parse(&[0x01, 0x02, 0x00, 0x00, 0x00, 0x05, 0x00]).unwrap();
        assert_eq!(wheel_only.wheel.unwrap().cumulative_revolutions, 2);
        assert!(wheel_only.crank.is_none());

        let crank_only = CscMeasurement::parse(&[0x02, 0x0A, 0x00, 0x0B, 0x00]).unwrap();
        assert!(crank_only.wheel.is_none());
        assert_eq!(crank_only.crank.unwrap().cumulative_revolutions, 10);
        assert_eq!(crank_only.crank.unwrap().last_event_time_1_1024_s, 11);

        let empty = CscMeasurement::parse(&[0x00, 0xFF]).unwrap();
        assert!(empty.wheel.is_none());
        assert!(empty.crank.is_none());
    }

    #[test]
    fn truncated_measurement_is_an_error() {
        assert!(CscMeasurement::parse(&[]).is_err());
        assert!(CscMeasurement::parse(&[0x01, 0x01, 0x00, 0x00, 0x00, 0x02]).is_err());
        assert!(CscMeasurement::parse(&[0x02, 0x01, 0x00, 0x02]).is_err());
        assert!(
            CscMeasurement::parse(&[0x03, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x03, 0x00]).is_err()
        );
    }

    #[test]
    fn crank_cadence_uses_the_cycling_power_counter() {
        let previous = CrankRevolutionData {
            cumulative_revolutions: 1_000,
            last_event_time_1_1024_s: 0,
        };
        let bytes = {
            let mut packet = vec![0x02];
            packet.extend_from_slice(&1003u16.to_le_bytes());
            packet.extend_from_slice(&2048u16.to_le_bytes());
            packet
        };
        let current = CscMeasurement::parse(&bytes).unwrap().crank.unwrap();
        let rpm = crate::crank_cadence_rpm(previous, current).unwrap();
        assert!((rpm - 90.0).abs() < 0.01, "{rpm}");
    }

    #[test]
    fn wheel_speed_wraps_both_counters() {
        let previous = WheelRevolutionData {
            cumulative_revolutions: u32::MAX - 1,
            last_event_time_1024: 65_000,
        };
        let current = WheelRevolutionData {
            cumulative_revolutions: 2,
            last_event_time_1024: 65_000u16.wrapping_add(1024),
        };
        // 4 revolutions of a 2 m wheel in 1 s is 8 m/s, 28.8 km/h.
        let kph = wheel_speed_kph(previous, current, 2.0).unwrap();
        assert!((kph - 28.8).abs() < 0.01, "{kph}");
        assert_eq!(wheel_speed_kph(previous, previous, 2.0), None);
        assert_eq!(wheel_speed_kph(previous, current, 0.0), None);
        assert_eq!(wheel_speed_kph(previous, current, -1.0), None);
        assert_eq!(wheel_speed_kph(previous, current, f32::NAN), None);
    }
}
