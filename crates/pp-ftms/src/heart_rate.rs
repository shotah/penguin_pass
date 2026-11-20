//! Heart Rate Measurement (0x2A37).
//!
//! The flags octet selects an 8-bit or 16-bit heart rate, then an optional
//! energy field (`uint16`) and a list of RR intervals (`uint16` each,
//! 1/1024 second). The Heart Rate Service specification gives energy in
//! kilojoules. The GATT supplement names the unit joule and does not state a
//! multiplier; the width is `uint16` either way.

use crate::error::FtmsError;
use crate::wire::Reader;

const FORMAT_U16: u8 = 1 << 0;
const CONTACT_DETECTED: u8 = 1 << 1;
const CONTACT_SUPPORTED: u8 = 1 << 2;
const ENERGY_EXPENDED: u8 = 1 << 3;
const RR_INTERVAL: u8 = 1 << 4;

/// One Heart Rate Measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct HeartRateMeasurement {
    /// Heart rate, in beats per minute. The 16-bit wire format is preserved.
    pub heart_rate_bpm: u16,
    /// Flag bit 2: the sensor reports contact.
    pub sensor_contact_supported: bool,
    /// Flag bit 1: contact is detected. Meaningful when the sensor supports it.
    pub sensor_contact_detected: bool,
    /// Accumulated energy, in kilojoules, when flag bit 3 is set.
    pub energy_expended_kj: Option<u16>,
    /// RR intervals, in seconds. Each wire value is 1/1024 second.
    pub rr_intervals_s: Vec<f32>,
}

impl HeartRateMeasurement {
    /// Parse one measurement. An odd leftover byte in the RR list is truncated.
    pub fn parse(bytes: &[u8]) -> Result<Self, FtmsError> {
        let mut reader = Reader::new(bytes);
        let flags = reader.u8("heart rate flags")?;
        let heart_rate_bpm = if flags & FORMAT_U16 == 0 {
            u16::from(reader.u8("heart rate")?)
        } else {
            reader.u16("heart rate")?
        };
        let energy_expended_kj = if flags & ENERGY_EXPENDED != 0 {
            Some(reader.u16("energy expended")?)
        } else {
            None
        };
        let mut rr_intervals_s = Vec::new();
        if flags & RR_INTERVAL != 0 {
            if !reader.remaining().is_multiple_of(2) {
                return Err(FtmsError::Truncated {
                    what: "rr interval",
                    have: reader.remaining(),
                    need: reader.remaining() + 1,
                });
            }
            while reader.remaining() >= 2 {
                let raw = reader.u16("rr interval")?;
                rr_intervals_s.push(f32::from(raw) / 1024.0);
            }
        }
        Ok(Self {
            heart_rate_bpm,
            sensor_contact_supported: flags & CONTACT_SUPPORTED != 0,
            sensor_contact_detected: flags & CONTACT_DETECTED != 0,
            energy_expended_kj,
            rr_intervals_s,
        })
    }

    /// Heart rate saturated into the `u8` range used by [`pp_core::TrainerSample`].
    #[must_use]
    pub fn heart_rate_bpm_u8(&self) -> u8 {
        u8::try_from(self.heart_rate_bpm).unwrap_or(u8::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_and_sixteen_bit_formats() {
        let narrow = HeartRateMeasurement::parse(&[0x00, 72]).unwrap();
        assert_eq!(narrow.heart_rate_bpm, 72);
        assert_eq!(narrow.heart_rate_bpm_u8(), 72);
        assert!(narrow.rr_intervals_s.is_empty());
        assert_eq!(narrow.energy_expended_kj, None);

        // Flag bit 0 set, value 300 = 0x012C.
        let wide = HeartRateMeasurement::parse(&[0x01, 0x2C, 0x01]).unwrap();
        assert_eq!(wide.heart_rate_bpm, 300);
        assert_eq!(wide.heart_rate_bpm_u8(), 255);
    }

    #[test]
    fn energy_and_rr_intervals() {
        // Bits 3 and 4, 8-bit heart rate 60, energy 9 kJ, two RR values of 1024 ticks (1 s).
        let bytes = [0x18, 60, 0x09, 0x00, 0x00, 0x04, 0x00, 0x04];
        let data = HeartRateMeasurement::parse(&bytes).unwrap();
        assert_eq!(data.heart_rate_bpm, 60);
        assert_eq!(data.energy_expended_kj, Some(9));
        assert_eq!(data.rr_intervals_s.len(), 2);
        assert!((data.rr_intervals_s[0] - 1.0).abs() < 0.001);
        assert!(HeartRateMeasurement::parse(&[0x10, 60, 0x01]).is_err());
    }

    #[test]
    fn truncated_input_is_an_error() {
        assert!(HeartRateMeasurement::parse(&[]).is_err());
        assert!(HeartRateMeasurement::parse(&[0x00]).is_err());
        assert!(HeartRateMeasurement::parse(&[0x01, 0x2C]).is_err());
        assert!(HeartRateMeasurement::parse(&[0x08, 60, 0x01]).is_err());
    }
}
