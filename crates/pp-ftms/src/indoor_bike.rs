//! Indoor Bike Data (0x2AD2).
//!
//! Field order and widths follow the GATT Specification Supplement. The flags
//! word is little-endian. Bit 0 (`More Data`) is inverted: instantaneous speed
//! is present only when that bit is 0. Every later field is present only when
//! its bit is 1, and fields are packed in bit order.
//!
//! FTMS v1.0 Table 4.10 prints the Instantaneous Cadence row with the columns
//! swapped (0 = present). The supplement, which FTMS points at for the
//! characteristic layout, says the field is present when bit 2 is 1. This
//! parser follows the supplement. That is also what trainers actually send.
//!
//! Resistance Level is the one field where the two documents disagree, and it
//! sits immediately before Instantaneous Power. The GATT Specification
//! Supplement says `uint8` (1 octet). FTMS v1.0 and the trainers built against
//! it use `sint16` (2 octets). When bit 5 is set, the parser measures the
//! buffer against both layouts and uses the width that matches exactly. Extra
//! trailing bytes fall back to `sint16`, which is what qdomyos-zwift,
//! GoldenCheetah, pycycling, and SmartSpin2k send. A buffer that is short for
//! both widths is an error.

use pp_core::TrainerSample;

use crate::error::FtmsError;
use crate::wire::Reader;

/// Bit 0, inverted: 0 means instantaneous speed is present.
const MORE_DATA: u16 = 1 << 0;
const AVERAGE_SPEED: u16 = 1 << 1;
const INSTANTANEOUS_CADENCE: u16 = 1 << 2;
const AVERAGE_CADENCE: u16 = 1 << 3;
const TOTAL_DISTANCE: u16 = 1 << 4;
const RESISTANCE_LEVEL: u16 = 1 << 5;
const INSTANTANEOUS_POWER: u16 = 1 << 6;
const AVERAGE_POWER: u16 = 1 << 7;
const EXPENDED_ENERGY: u16 = 1 << 8;
const HEART_RATE: u16 = 1 << 9;
const METABOLIC_EQUIVALENT: u16 = 1 << 10;
const ELAPSED_TIME: u16 = 1 << 11;
const REMAINING_TIME: u16 = 1 << 12;

/// One Indoor Bike Data notification.
///
/// Absent optional fields are `None`. Resolutions match the supplement:
/// speed is 0.01 km/h, cadence is 0.5 rpm, distance is 1 m, power is 1 W,
/// heart rate is 1 bpm, metabolic equivalent is 0.1, time is 1 s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndoorBikeData {
    /// Instantaneous speed, in kilometers per hour.
    pub instantaneous_speed_kph: Option<f32>,
    /// Average speed since the session started, in kilometers per hour.
    pub average_speed_kph: Option<f32>,
    /// Instantaneous cadence, in revolutions per minute.
    pub instantaneous_cadence_rpm: Option<f32>,
    /// Average cadence since the session started, in revolutions per minute.
    pub average_cadence_rpm: Option<f32>,
    /// Total distance since the session started, in meters.
    pub total_distance_m: Option<u32>,
    /// Current resistance level, unitless, resolution 1.
    ///
    /// `sint16` on the wire for FTMS v1.0 trainers. A packet whose length
    /// matches the one-octet supplement layout is stored in the same `i16`.
    pub resistance_level: Option<i16>,
    /// Instantaneous power, in watts. Negative means the trainer is driving the cranks.
    pub instantaneous_power_w: Option<i16>,
    /// Average power since the session started, in watts.
    pub average_power_w: Option<i16>,
    /// Total expended energy, in kilocalories. `0xFFFF` means the server has no value.
    pub total_energy_kcal: Option<u16>,
    /// Expended energy per hour, in kilocalories. `0xFFFF` means the server has no value.
    pub energy_per_hour_kcal: Option<u16>,
    /// Expended energy per minute, in kilocalories. `0xFF` means the server has no value.
    pub energy_per_minute_kcal: Option<u8>,
    /// Heart rate, in beats per minute.
    pub heart_rate_bpm: Option<u8>,
    /// Metabolic equivalent, in METs.
    pub metabolic_equivalent: Option<f32>,
    /// Elapsed session time, in seconds.
    pub elapsed_time_s: Option<u16>,
    /// Remaining session time, in seconds.
    pub remaining_time_s: Option<u16>,
}

impl IndoorBikeData {
    /// Parse one notification. Short buffers return [`FtmsError::Truncated`].
    ///
    /// When Resistance Level is present, a length that matches the 2-octet
    /// layout or the 1-octet layout selects that width. Any longer buffer is
    /// read as 2 octets and the extra bytes are ignored.
    pub fn parse(bytes: &[u8]) -> Result<Self, FtmsError> {
        if bytes.len() < 2 {
            return Err(FtmsError::Truncated {
                what: "indoor bike flags",
                have: bytes.len(),
                need: 2,
            });
        }
        let flags = u16::from_le_bytes([bytes[0], bytes[1]]);
        let resistance_octets = resistance_width(flags, bytes.len())?;
        let mut reader = Reader::new(bytes);
        let _ = reader.u16("indoor bike flags")?;

        let instantaneous_speed_kph = optional_speed(&mut reader, flags & MORE_DATA == 0)?;
        let average_speed_kph = optional_speed(&mut reader, flags & AVERAGE_SPEED != 0)?;
        let instantaneous_cadence_rpm =
            optional_cadence(&mut reader, flags & INSTANTANEOUS_CADENCE != 0)?;
        let average_cadence_rpm = optional_cadence(&mut reader, flags & AVERAGE_CADENCE != 0)?;
        let total_distance_m = if flags & TOTAL_DISTANCE != 0 {
            Some(reader.u24("total distance")?)
        } else {
            None
        };
        let resistance_level = match resistance_octets {
            0 => None,
            1 => Some(i16::from(reader.u8("resistance level")?)),
            _ => Some(reader.i16("resistance level")?),
        };
        let instantaneous_power_w = optional_power(&mut reader, flags & INSTANTANEOUS_POWER != 0)?;
        let average_power_w = optional_power(&mut reader, flags & AVERAGE_POWER != 0)?;

        let (total_energy_kcal, energy_per_hour_kcal, energy_per_minute_kcal) =
            if flags & EXPENDED_ENERGY != 0 {
                (
                    Some(reader.u16("total energy")?),
                    Some(reader.u16("energy per hour")?),
                    Some(reader.u8("energy per minute")?),
                )
            } else {
                (None, None, None)
            };

        let heart_rate_bpm = if flags & HEART_RATE != 0 {
            Some(reader.u8("heart rate")?)
        } else {
            None
        };
        let metabolic_equivalent = if flags & METABOLIC_EQUIVALENT != 0 {
            Some(f32::from(reader.u8("metabolic equivalent")?) / 10.0)
        } else {
            None
        };
        let elapsed_time_s = if flags & ELAPSED_TIME != 0 {
            Some(reader.u16("elapsed time")?)
        } else {
            None
        };
        let remaining_time_s = if flags & REMAINING_TIME != 0 {
            Some(reader.u16("remaining time")?)
        } else {
            None
        };

        Ok(Self {
            instantaneous_speed_kph,
            average_speed_kph,
            instantaneous_cadence_rpm,
            average_cadence_rpm,
            total_distance_m,
            resistance_level,
            instantaneous_power_w,
            average_power_w,
            total_energy_kcal,
            energy_per_hour_kcal,
            energy_per_minute_kcal,
            heart_rate_bpm,
            metabolic_equivalent,
            elapsed_time_s,
            remaining_time_s,
        })
    }

    /// Map the fields [`TrainerSample`] can carry.
    ///
    /// Negative power becomes `Some(0)`. [`TrainerSample::power_w`] is unsigned;
    /// a negative reading means the trainer is not reporting rider output.
    /// Distance, resistance, and the energy fields have no place on the sample.
    #[must_use]
    pub fn to_trainer_sample(&self) -> TrainerSample {
        TrainerSample {
            power_w: self.instantaneous_power_w.map(clamp_power),
            cadence_rpm: self.instantaneous_cadence_rpm,
            speed_kph: self.instantaneous_speed_kph,
            heart_rate_bpm: self.heart_rate_bpm,
        }
    }
}

/// Octets of payload that do not include Resistance Level.
fn body_without_resistance(flags: u16) -> usize {
    let mut octets = 2;
    if flags & MORE_DATA == 0 {
        octets += 2;
    }
    if flags & AVERAGE_SPEED != 0 {
        octets += 2;
    }
    if flags & INSTANTANEOUS_CADENCE != 0 {
        octets += 2;
    }
    if flags & AVERAGE_CADENCE != 0 {
        octets += 2;
    }
    if flags & TOTAL_DISTANCE != 0 {
        octets += 3;
    }
    if flags & INSTANTANEOUS_POWER != 0 {
        octets += 2;
    }
    if flags & AVERAGE_POWER != 0 {
        octets += 2;
    }
    if flags & EXPENDED_ENERGY != 0 {
        octets += 5;
    }
    if flags & HEART_RATE != 0 {
        octets += 1;
    }
    if flags & METABOLIC_EQUIVALENT != 0 {
        octets += 1;
    }
    if flags & ELAPSED_TIME != 0 {
        octets += 2;
    }
    if flags & REMAINING_TIME != 0 {
        octets += 2;
    }
    octets
}

fn resistance_width(flags: u16, len: usize) -> Result<usize, FtmsError> {
    if flags & RESISTANCE_LEVEL == 0 {
        return Ok(0);
    }
    let base = body_without_resistance(flags);
    let narrow = base + 1;
    let wide = base + 2;
    if len >= wide {
        Ok(2)
    } else if len == narrow {
        Ok(1)
    } else {
        Err(FtmsError::Truncated {
            what: "indoor bike data",
            have: len,
            need: narrow,
        })
    }
}

fn optional_speed(reader: &mut Reader<'_>, present: bool) -> Result<Option<f32>, FtmsError> {
    if present {
        Ok(Some(f32::from(reader.u16("speed")?) / 100.0))
    } else {
        Ok(None)
    }
}

fn optional_cadence(reader: &mut Reader<'_>, present: bool) -> Result<Option<f32>, FtmsError> {
    if present {
        Ok(Some(f32::from(reader.u16("cadence")?) / 2.0))
    } else {
        Ok(None)
    }
}

fn optional_power(reader: &mut Reader<'_>, present: bool) -> Result<Option<i16>, FtmsError> {
    if present {
        Ok(Some(reader.i16("power")?))
    } else {
        Ok(None)
    }
}

pub(crate) fn clamp_power(watts: i16) -> u16 {
    u16::try_from(watts).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
    }

    #[test]
    fn speed_cadence_and_power_from_a_concrete_vector() {
        // Flags 0x0044: speed present (bit 0 clear), cadence (bit 2), power (bit 6).
        // Speed 3000 -> 30.00 km/h, cadence 181 -> 90.5 rpm, power 250 W.
        let bytes = [0x44, 0x00, 0xB8, 0x0B, 0xB5, 0x00, 0xFA, 0x00];
        let data = IndoorBikeData::parse(&bytes).unwrap();
        assert_close(data.instantaneous_speed_kph.unwrap(), 30.0);
        assert_close(data.instantaneous_cadence_rpm.unwrap(), 90.5);
        assert_eq!(data.instantaneous_power_w, Some(250));
        assert_eq!(data.heart_rate_bpm, None);
        assert_eq!(data.total_distance_m, None);
        assert_eq!(data.resistance_level, None);
        let sample = data.to_trainer_sample();
        assert_eq!(sample.power_w, Some(250));
        assert_close(sample.cadence_rpm.unwrap(), 90.5);
        assert_close(sample.speed_kph.unwrap(), 30.0);
    }

    #[test]
    fn negative_power_and_inverted_more_data_bit() {
        // Flags 0x0040: speed present, power present, cadence absent.
        // Speed 0x2710 = 10000 -> 100.00 km/h. Power 0xFFF1 = -15 W.
        let bytes = [0x40, 0x00, 0x10, 0x27, 0xF1, 0xFF];
        let data = IndoorBikeData::parse(&bytes).unwrap();
        assert_close(data.instantaneous_speed_kph.unwrap(), 100.0);
        assert_eq!(data.instantaneous_cadence_rpm, None);
        assert_eq!(data.instantaneous_power_w, Some(-15));
        assert_eq!(data.to_trainer_sample().power_w, Some(0));
    }

    #[test]
    fn more_data_set_skips_speed_and_still_reads_power() {
        // Bit 0 set (no speed) and bit 6 set (power). Power must not be eaten as speed.
        let bytes = [0x41, 0x00, 0x2C, 0x01];
        let data = IndoorBikeData::parse(&bytes).unwrap();
        assert_eq!(data.instantaneous_speed_kph, None);
        assert_eq!(data.instantaneous_power_w, Some(300));
    }

    #[test]
    fn every_field_is_parsed_in_spec_order() {
        // Bit 0 set so speed is absent. Bits 1 through 12 set.
        let mut bytes = vec![0xFF, 0x1F];
        bytes.extend_from_slice(&1000u16.to_le_bytes()); // average speed 10.00 km/h
        bytes.extend_from_slice(&160u16.to_le_bytes()); // instantaneous cadence 80 rpm
        bytes.extend_from_slice(&150u16.to_le_bytes()); // average cadence 75 rpm
        bytes.extend_from_slice(&[0x03, 0x02, 0x01]); // distance 0x010203 = 66051 m
        bytes.extend_from_slice(&7i16.to_le_bytes()); // resistance, FTMS sint16
        bytes.extend_from_slice(&(-15i16).to_le_bytes());
        bytes.extend_from_slice(&100i16.to_le_bytes());
        bytes.extend_from_slice(&50u16.to_le_bytes()); // total energy
        bytes.extend_from_slice(&400u16.to_le_bytes()); // per hour
        bytes.push(6); // per minute
        bytes.push(140); // heart rate
        bytes.push(80); // 8.0 MET
        bytes.extend_from_slice(&90u16.to_le_bytes());
        bytes.extend_from_slice(&30u16.to_le_bytes());

        let data = IndoorBikeData::parse(&bytes).unwrap();
        assert_eq!(data.instantaneous_speed_kph, None);
        assert_close(data.average_speed_kph.unwrap(), 10.0);
        assert_close(data.instantaneous_cadence_rpm.unwrap(), 80.0);
        assert_close(data.average_cadence_rpm.unwrap(), 75.0);
        assert_eq!(data.total_distance_m, Some(0x010203));
        assert_eq!(data.resistance_level, Some(7));
        assert_eq!(data.instantaneous_power_w, Some(-15));
        assert_eq!(data.average_power_w, Some(100));
        assert_eq!(data.total_energy_kcal, Some(50));
        assert_eq!(data.energy_per_hour_kcal, Some(400));
        assert_eq!(data.energy_per_minute_kcal, Some(6));
        assert_eq!(data.heart_rate_bpm, Some(140));
        assert_close(data.metabolic_equivalent.unwrap(), 8.0);
        assert_eq!(data.elapsed_time_s, Some(90));
        assert_eq!(data.remaining_time_s, Some(30));

        let mut short = bytes.clone();
        // One byte short of the 2-octet layout is a valid 1-octet layout.
        // Dropping the whole remaining-time field is short for both widths.
        short.truncate(short.len() - 2);
        assert!(IndoorBikeData::parse(&short).is_err());
    }

    #[test]
    fn two_octet_resistance_keeps_the_following_power() {
        // Bit 0 set (no speed), bit 5 resistance, bit 6 power. Resistance -5, power 250.
        let bytes = [0x61, 0x00, 0xFB, 0xFF, 0xFA, 0x00];
        let data = IndoorBikeData::parse(&bytes).unwrap();
        assert_eq!(data.resistance_level, Some(-5));
        assert_eq!(data.instantaneous_power_w, Some(250));
    }

    #[test]
    fn one_octet_resistance_keeps_the_following_power() {
        // Same flags as the 2-octet case, one octet of resistance (10), power 180.
        let bytes = [0x61, 0x00, 0x0A, 0xB4, 0x00];
        let data = IndoorBikeData::parse(&bytes).unwrap();
        assert_eq!(data.resistance_level, Some(10));
        assert_eq!(data.instantaneous_power_w, Some(180));
    }

    #[test]
    fn trailing_bytes_fall_back_to_sint16_resistance() {
        // 2-octet resistance 10, power 250, plus a spare octet. Reading the
        // resistance as one byte would turn the power into 0xFA00.
        let bytes = [0x61, 0x00, 0x0A, 0x00, 0xFA, 0x00, 0xFF];
        let data = IndoorBikeData::parse(&bytes).unwrap();
        assert_eq!(data.resistance_level, Some(10));
        assert_eq!(data.instantaneous_power_w, Some(250));
    }

    #[test]
    fn flag_combinations_and_truncated_input() {
        let cases: &[(&str, &[u8], bool)] = &[
            ("flags only, more data, no fields", &[0x01, 0x00], true),
            ("empty", &[], false),
            ("one flag byte", &[0x00], false),
            ("speed flagged but missing", &[0x00, 0x00], false),
            ("speed present", &[0x00, 0x00, 0xE8, 0x03], true),
            (
                "distance and heart rate, no speed",
                &[0x11, 0x02, 0x39, 0x30, 0x00, 0x48],
                true,
            ),
        ];
        for (name, bytes, ok) in cases {
            assert_eq!(IndoorBikeData::parse(bytes).is_ok(), *ok, "{name}");
        }
        let distance = IndoorBikeData::parse(&[0x11, 0x02, 0x39, 0x30, 0x00, 0x48]).unwrap();
        // Flags 0x0211: more data, total distance, heart rate.
        assert_eq!(distance.total_distance_m, Some(0x3039));
        assert_eq!(distance.heart_rate_bpm, Some(0x48));
        assert_eq!(distance.instantaneous_speed_kph, None);
        assert!(IndoorBikeData::parse(&[0x11, 0x02, 0x39, 0x30]).is_err());
    }
}
