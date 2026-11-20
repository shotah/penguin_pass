//! Cycling Power Measurement (0x2A63).
//!
//! The flags word is little-endian. Instantaneous power (`sint16`, watts) is
//! always present. Optional fields follow in bit order. Bits 1, 3, and 12
//! change the meaning of a neighbor but do not add octets. Extreme Angles is
//! a packed pair of 12-bit values, three octets, and is skipped rather than
//! decoded.

use pp_core::TrainerSample;

use crate::error::FtmsError;
use crate::indoor_bike::clamp_power;
use crate::wire::Reader;

const PEDAL_POWER_BALANCE: u16 = 1 << 0;
const ACCUMULATED_TORQUE: u16 = 1 << 2;
const WHEEL_REVOLUTION_DATA: u16 = 1 << 4;
const CRANK_REVOLUTION_DATA: u16 = 1 << 5;
const EXTREME_FORCE: u16 = 1 << 6;
const EXTREME_TORQUE: u16 = 1 << 7;
const EXTREME_ANGLES: u16 = 1 << 8;
const TOP_DEAD_SPOT: u16 = 1 << 9;
const BOTTOM_DEAD_SPOT: u16 = 1 << 10;
const ACCUMULATED_ENERGY: u16 = 1 << 11;

/// Crank revolution counters from one Cycling Power Measurement.
///
/// Cadence needs two of these. See [`crank_cadence_rpm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrankRevolutionData {
    /// Cumulative crank revolutions. The counter wraps at `u16`.
    pub cumulative_revolutions: u16,
    /// Last crank event time, in units of 1/1024 second. The counter wraps at `u16`.
    pub last_event_time_1_1024_s: u16,
}

/// One Cycling Power Measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CyclingPowerMeasurement {
    /// Instantaneous power, in watts. Always present. May be negative.
    pub instantaneous_power_w: i16,
    /// Crank revolution data, when flag bit 5 is set.
    pub crank: Option<CrankRevolutionData>,
}

impl CyclingPowerMeasurement {
    /// Parse one measurement. Short buffers return [`FtmsError::Truncated`].
    pub fn parse(bytes: &[u8]) -> Result<Self, FtmsError> {
        let mut reader = Reader::new(bytes);
        let flags = reader.u16("cycling power flags")?;
        let instantaneous_power_w = reader.i16("instantaneous power")?;

        if flags & PEDAL_POWER_BALANCE != 0 {
            reader.skip(1, "pedal power balance")?;
        }
        if flags & ACCUMULATED_TORQUE != 0 {
            reader.skip(2, "accumulated torque")?;
        }
        if flags & WHEEL_REVOLUTION_DATA != 0 {
            reader.skip(6, "wheel revolution data")?;
        }
        let crank = if flags & CRANK_REVOLUTION_DATA != 0 {
            Some(CrankRevolutionData {
                cumulative_revolutions: reader.u16("cumulative crank revolutions")?,
                last_event_time_1_1024_s: reader.u16("last crank event time")?,
            })
        } else {
            None
        };
        if flags & EXTREME_FORCE != 0 {
            reader.skip(4, "extreme force magnitudes")?;
        }
        if flags & EXTREME_TORQUE != 0 {
            reader.skip(4, "extreme torque magnitudes")?;
        }
        if flags & EXTREME_ANGLES != 0 {
            reader.skip(3, "extreme angles")?;
        }
        if flags & TOP_DEAD_SPOT != 0 {
            reader.skip(2, "top dead spot angle")?;
        }
        if flags & BOTTOM_DEAD_SPOT != 0 {
            reader.skip(2, "bottom dead spot angle")?;
        }
        if flags & ACCUMULATED_ENERGY != 0 {
            reader.skip(2, "accumulated energy")?;
        }

        Ok(Self {
            instantaneous_power_w,
            crank,
        })
    }

    /// Power, plus a cadence the caller computed from two crank readings.
    ///
    /// Negative power becomes `Some(0)` on the [`TrainerSample`]. Speed and
    /// heart rate are not in this characteristic.
    #[must_use]
    pub fn to_trainer_sample(&self, cadence_rpm: Option<f32>) -> TrainerSample {
        TrainerSample {
            power_w: Some(clamp_power(self.instantaneous_power_w)),
            cadence_rpm,
            speed_kph: None,
            heart_rate_bpm: None,
        }
    }
}

/// Cadence, in revolutions per minute, between two crank readings.
///
/// Both counters are `u16` and wrap. A zero time delta means the crank event
/// clock did not advance, which the sensor does when the rider is not pedaling,
/// so the result is `Some(0.0)`. A zero time delta with a non-zero revolution
/// delta is not a usable rate and returns `None`.
#[must_use]
pub fn crank_cadence_rpm(
    previous: CrankRevolutionData,
    current: CrankRevolutionData,
) -> Option<f32> {
    let revolutions = u32::from(
        current
            .cumulative_revolutions
            .wrapping_sub(previous.cumulative_revolutions),
    );
    let ticks = u32::from(
        current
            .last_event_time_1_1024_s
            .wrapping_sub(previous.last_event_time_1_1024_s),
    );
    if ticks == 0 {
        return if revolutions == 0 { Some(0.0) } else { None };
    }
    Some(revolutions as f32 * 1024.0 * 60.0 / ticks as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_only_and_negative_power() {
        let data = CyclingPowerMeasurement::parse(&[0x00, 0x00, 0xF6, 0xFF]).unwrap();
        assert_eq!(data.instantaneous_power_w, -10);
        assert_eq!(data.crank, None);
        assert_eq!(data.to_trainer_sample(None).power_w, Some(0));
    }

    #[test]
    fn crank_cadence_handles_wrap_of_both_counters() {
        let previous = CrankRevolutionData {
            cumulative_revolutions: 65_530,
            last_event_time_1_1024_s: 65_500,
        };
        let current = CrankRevolutionData {
            cumulative_revolutions: 4,
            last_event_time_1_1024_s: 65_500u16.wrapping_add(1024),
        };
        let rpm = crank_cadence_rpm(previous, current).unwrap();
        assert!((rpm - 600.0).abs() < 0.01, "{rpm}");

        let stopped = crank_cadence_rpm(previous, previous).unwrap();
        assert!((stopped - 0.0).abs() < f32::EPSILON);
        assert_eq!(
            crank_cadence_rpm(
                previous,
                CrankRevolutionData {
                    cumulative_revolutions: previous.cumulative_revolutions.wrapping_add(1),
                    last_event_time_1_1024_s: previous.last_event_time_1_1024_s,
                }
            ),
            None
        );
    }

    #[test]
    fn every_optional_field_is_skipped_at_its_width() {
        // Bits that add octets: 0, 2, 4, 5, 6, 7, 8, 9, 10, 11.
        let flags: u16 = (1 << 0)
            | (1 << 2)
            | (1 << 4)
            | (1 << 5)
            | (1 << 6)
            | (1 << 7)
            | (1 << 8)
            | (1 << 9)
            | (1 << 10)
            | (1 << 11);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&(-5i16).to_le_bytes());
        bytes.push(0x32); // balance, 1
        bytes.extend_from_slice(&[0x11, 0x00]); // torque, 2
        bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // wheel, 6
        bytes.extend_from_slice(&1000u16.to_le_bytes());
        bytes.extend_from_slice(&200u16.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0]); // force, 4
        bytes.extend_from_slice(&[0, 0, 0, 0]); // torque magnitudes, 4
        bytes.extend_from_slice(&[0xAB, 0xC1, 0x23]); // angles, 3
        bytes.extend_from_slice(&[0, 0]); // top, 2
        bytes.extend_from_slice(&[0, 0]); // bottom, 2
        bytes.extend_from_slice(&[0, 0]); // energy, 2
        assert_eq!(bytes.len(), 34);

        let data = CyclingPowerMeasurement::parse(&bytes).unwrap();
        assert_eq!(data.instantaneous_power_w, -5);
        assert_eq!(
            data.crank,
            Some(CrankRevolutionData {
                cumulative_revolutions: 1000,
                last_event_time_1_1024_s: 200,
            })
        );
        let mut short = bytes.clone();
        short.pop();
        assert!(CyclingPowerMeasurement::parse(&short).is_err());
        assert!(CyclingPowerMeasurement::parse(&[0x20, 0x00, 0x01, 0x00, 0x02, 0x00]).is_err());
    }

    #[test]
    fn truncated_power_is_an_error() {
        assert!(CyclingPowerMeasurement::parse(&[]).is_err());
        assert!(CyclingPowerMeasurement::parse(&[0x00, 0x00, 0x01]).is_err());
    }
}
