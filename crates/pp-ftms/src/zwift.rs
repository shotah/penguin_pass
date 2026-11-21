//! Zwift trainer service (`0xFC82`).
//!
//! Zwift Hub, Zwift Hub One, Zwift Ride, Wahoo KICKR CORE in Zwift One / CORE 2
//! form, and JetBlack Victory speak this service. A session starts by writing
//! [`ZWIFT_HANDSHAKE`] (`RideOn`) to the write characteristic. After that, each
//! notification and command is one opcode byte plus a protobuf message. The
//! varint and zigzag helpers live in this module; the crate does not depend
//! on a protobuf runtime.
//!
//! Riding data is opcode `0x03`. Hub commands are opcode `0x04`. Unknown
//! protobuf fields are skipped. A truncated or illegal wire type is
//! [`FtmsError::Truncated`] or [`FtmsError::Malformed`]. A non-finite command
//! float is [`FtmsError::NonFinite`].

use crate::error::FtmsError;

/// Bytes written to open a session on the trainer write characteristic.
pub const ZWIFT_HANDSHAKE: &[u8] = b"RideOn";
/// Opcode of a riding-data notification.
pub const ZWIFT_MSG_RIDING_DATA: u8 = 0x03;
/// Opcode of a hub command (target power, simulation, or physical).
pub const ZWIFT_MSG_HUB_COMMAND: u8 = 0x04;

/// Mixed Terrain virtual gear ratios, gear 1 through gear 24, times 10_000.
///
/// Zwift's two-decimal table runs from 0.75 to 5.49. These integers are the
/// `GearRatioX10000` values the app writes. Gear 1 is 7_500 and gear 24 is
/// 54_899 (5.4899). Gears 13 and 20 are 26_099 and 42_399; the two-decimal
/// table rounds those to 2.61 and 4.24.
pub const ZWIFT_VIRTUAL_GEAR_RATIOS: [u32; 24] = [
    7_500, 8_700, 9_900, 11_100, 12_300, 13_800, 15_300, 16_800, 18_600, 20_400, 22_200, 24_000,
    26_099, 28_200, 30_300, 32_400, 34_900, 37_400, 39_900, 42_399, 45_400, 48_400, 51_400, 54_899,
];

const WIRE_VARINT: u8 = 0;
const WIRE_FIXED64: u8 = 1;
const WIRE_LENGTH: u8 = 2;
const WIRE_FIXED32: u8 = 5;

/// One riding-data notification (opcode `0x03`).
///
/// Power, cadence, speed, and heart rate are protobuf fields 1–4, each a
/// varint. Speed on the wire is kilometers per hour times 100. Fields this
/// crate does not name (including 5 and 6) are skipped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZwiftRidingData {
    /// Instantaneous power, in watts. Protobuf field 1.
    pub power_w: Option<u16>,
    /// Cadence, in revolutions per minute. Protobuf field 2.
    pub cadence_rpm: Option<u8>,
    /// Speed, in kilometers per hour. Protobuf field 3, divided by 100.
    pub speed_kph: Option<f32>,
    /// Heart rate, in beats per minute. Protobuf field 4.
    pub heart_rate_bpm: Option<u8>,
}

impl ZwiftRidingData {
    /// Parse one notification. The first octet must be [`ZWIFT_MSG_RIDING_DATA`].
    ///
    /// A short buffer, a varint that does not end, a length that runs past the
    /// buffer, or a wire type other than varint, 64-bit, length-delimited, or
    /// 32-bit is an error. Known fields must be varints and must fit their
    /// Rust type. The function does not panic.
    pub fn parse(bytes: &[u8]) -> Result<Self, FtmsError> {
        if bytes.is_empty() {
            return Err(FtmsError::Truncated {
                what: "zwift riding data",
                have: 0,
                need: 1,
            });
        }
        if bytes[0] != ZWIFT_MSG_RIDING_DATA {
            return Err(FtmsError::Malformed {
                what: "zwift riding data",
                detail: "first octet is not 0x03",
            });
        }
        let mut cursor = Cursor::new(&bytes[1..]);
        let mut data = Self {
            power_w: None,
            cadence_rpm: None,
            speed_kph: None,
            heart_rate_bpm: None,
        };
        while !cursor.done() {
            let key = cursor.read_varint()?;
            let field = key >> 3;
            let wire = u8::try_from(key & 0x07).map_err(|_| FtmsError::Malformed {
                what: "zwift riding data",
                detail: "unknown wire type",
            })?;
            if field == 0 {
                return Err(FtmsError::Malformed {
                    what: "zwift riding data",
                    detail: "field number 0",
                });
            }
            match wire {
                WIRE_VARINT => data.apply_varint(field, cursor.read_varint()?)?,
                WIRE_FIXED64 => {
                    data.reject_known(field)?;
                    cursor.skip(8, "zwift fixed64")?;
                }
                WIRE_LENGTH => {
                    data.reject_known(field)?;
                    cursor.skip_length_delimited()?;
                }
                WIRE_FIXED32 => {
                    data.reject_known(field)?;
                    cursor.skip(4, "zwift fixed32")?;
                }
                _ => {
                    return Err(FtmsError::Malformed {
                        what: "zwift riding data",
                        detail: "unknown wire type",
                    });
                }
            }
        }
        Ok(data)
    }
}

impl ZwiftRidingData {
    fn apply_varint(&mut self, field: u64, value: u64) -> Result<(), FtmsError> {
        match field {
            1 => self.power_w = Some(fit("zwift power", value)?),
            2 => self.cadence_rpm = Some(fit("zwift cadence", value)?),
            3 => {
                let raw: u32 = fit("zwift speed", value)?;
                #[allow(clippy::cast_precision_loss)]
                {
                    self.speed_kph = Some(raw as f32 / 100.0);
                }
            }
            4 => self.heart_rate_bpm = Some(fit("zwift heart rate", value)?),
            _ => {}
        }
        Ok(())
    }

    fn reject_known(&self, field: u64) -> Result<(), FtmsError> {
        if (1..=4).contains(&field) {
            Err(FtmsError::Malformed {
                what: "zwift riding data",
                detail: "unexpected wire type",
            })
        } else {
            Ok(())
        }
    }
}

/// Simulation parameters for a hub command (protobuf field 4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZwiftSimulation {
    /// Wind speed, in meters per second. Positive is a headwind.
    /// Wire unit is 0.01 m/s, zigzag `sint32`.
    pub wind_speed_mps: f32,
    /// Grade, in percent. Positive is uphill. `5.0` is a 5% grade.
    /// Wire unit is 0.01%, zigzag `sint32`.
    pub grade_pct: f32,
    /// Wind resistance coefficient, in kilograms per meter.
    /// Wire unit is 0.0001 kg/m (`value * 10_000`), `uint32`.
    pub cw_kg_per_m: f32,
    /// Coefficient of rolling resistance, dimensionless.
    /// Wire unit is 0.00001 (`value * 100_000`), `uint32`.
    pub crr: f32,
}

/// Bike and rider parameters for a hub command (protobuf field 5).
///
/// `None` omits that protobuf field. `Some(0.0)` writes a zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZwiftPhysical {
    /// Gear ratio times 10_000. See [`ZWIFT_VIRTUAL_GEAR_RATIOS`]. Field 2.
    pub gear_ratio_x10000: Option<u32>,
    /// Bicycle mass, in kilograms. Wire unit is 0.01 kg. Field 4.
    pub bike_weight_kg: Option<f32>,
    /// Rider mass, in kilograms. Wire unit is 0.01 kg. Field 5.
    pub rider_weight_kg: Option<f32>,
}

/// A hub command (opcode `0x04`) this crate can send.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZwiftHubCommand {
    /// ERG mode. Protobuf field 3, a varint. Tag `0x18`.
    SetTargetPower {
        /// Target power, in watts.
        watts: u16,
    },
    /// Wind, grade, drag, and rolling resistance.
    ///
    /// Protobuf field 4, a length-delimited submessage. Tag `0x22`.
    SetSimulation(ZwiftSimulation),
    /// Gear ratio and masses.
    ///
    /// Protobuf field 5, a length-delimited submessage. Tag `0x2A`.
    SetPhysical(ZwiftPhysical),
}

impl ZwiftHubCommand {
    /// Encode the command.
    ///
    /// Signed fields saturate into `sint32`. Unsigned fields saturate into
    /// `uint32`, and a negative unsigned input saturates to 0. A non-finite
    /// float is [`FtmsError::NonFinite`].
    pub fn encode(&self) -> Result<Vec<u8>, FtmsError> {
        let mut out = Vec::new();
        out.push(ZWIFT_MSG_HUB_COMMAND);
        match *self {
            Self::SetTargetPower { watts } => {
                push_tag(&mut out, 3, WIRE_VARINT);
                push_varint(&mut out, u64::from(watts));
            }
            Self::SetSimulation(sim) => {
                let mut body = Vec::with_capacity(16);
                push_sint(&mut body, 1, scale_i32(sim.wind_speed_mps, 100.0)?);
                push_sint(&mut body, 2, scale_i32(sim.grade_pct, 100.0)?);
                push_uint(&mut body, 3, scale_u32(sim.cw_kg_per_m, 10_000.0)?);
                push_uint(&mut body, 4, scale_u32(sim.crr, 100_000.0)?);
                push_message(&mut out, 4, &body);
            }
            Self::SetPhysical(physical) => {
                let mut body = Vec::with_capacity(18);
                if let Some(gear) = physical.gear_ratio_x10000 {
                    push_uint(&mut body, 2, gear);
                }
                if let Some(bike) = physical.bike_weight_kg {
                    push_uint(&mut body, 4, scale_u32(bike, 100.0)?);
                }
                if let Some(rider) = physical.rider_weight_kg {
                    push_uint(&mut body, 5, scale_u32(rider, 100.0)?);
                }
                push_message(&mut out, 5, &body);
            }
        }
        Ok(out)
    }
}

fn push_sint(out: &mut Vec<u8>, field: u8, value: i32) {
    push_tag(out, field, WIRE_VARINT);
    push_varint(out, u64::from(zigzag_encode(value)));
}

fn push_uint(out: &mut Vec<u8>, field: u8, value: u32) {
    push_tag(out, field, WIRE_VARINT);
    push_varint(out, u64::from(value));
}

fn push_message(out: &mut Vec<u8>, field: u8, body: &[u8]) {
    push_tag(out, field, WIRE_LENGTH);
    push_varint(out, body.len() as u64);
    out.extend_from_slice(body);
}

fn push_tag(out: &mut Vec<u8>, field: u8, wire: u8) {
    out.push((field << 3) | wire);
}

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        #[allow(clippy::cast_possible_truncation)]
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Protobuf zigzag. `0` stays `0`, `-1` becomes `1`, `-1000` becomes `1999`.
fn zigzag_encode(value: i32) -> u32 {
    #[allow(clippy::cast_sign_loss)]
    let bits = value as u32;
    #[allow(clippy::cast_sign_loss)]
    {
        (bits << 1) ^ ((value >> 31) as u32)
    }
}

/// `i32::MAX as f32` is `2^31`, which does not fit in `i32`, so the `>=` arm
/// is what keeps the cast in range. A finite input whose scaled value
/// overflows to infinity saturates to the matching end.
fn scale_i32(value: f32, per_unit: f32) -> Result<i32, FtmsError> {
    if !value.is_finite() {
        return Err(FtmsError::NonFinite);
    }
    let scaled = (value * per_unit).round();
    if scaled >= i32::MAX as f32 {
        Ok(i32::MAX)
    } else if scaled <= i32::MIN as f32 {
        Ok(i32::MIN)
    } else {
        #[allow(clippy::cast_possible_truncation)]
        Ok(scaled as i32)
    }
}

fn scale_u32(value: f32, per_unit: f32) -> Result<u32, FtmsError> {
    if !value.is_finite() {
        return Err(FtmsError::NonFinite);
    }
    let scaled = (value * per_unit).round();
    if scaled <= 0.0 {
        Ok(0)
    } else if scaled >= u32::MAX as f32 {
        Ok(u32::MAX)
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok(scaled as u32)
    }
}

fn fit<T>(what: &'static str, value: u64) -> Result<T, FtmsError>
where
    T: TryFrom<u64>,
{
    T::try_from(value).map_err(|_| FtmsError::Malformed {
        what,
        detail: "does not fit",
    })
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn done(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_varint(&mut self) -> Result<u64, FtmsError> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            if self.pos >= self.data.len() {
                return Err(FtmsError::Truncated {
                    what: "zwift varint",
                    have: 0,
                    need: 1,
                });
            }
            let byte = self.data[self.pos];
            self.pos += 1;
            let bits = u64::from(byte & 0x7F);
            if shift == 63 && (byte & 0x80 != 0 || bits > 1) {
                return Err(FtmsError::Malformed {
                    what: "zwift varint",
                    detail: "too long",
                });
            }
            value |= bits << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    fn skip(&mut self, n: usize, what: &'static str) -> Result<(), FtmsError> {
        let have = self.remaining();
        if have < n {
            return Err(FtmsError::Truncated {
                what,
                have,
                need: n,
            });
        }
        self.pos += n;
        Ok(())
    }

    fn skip_length_delimited(&mut self) -> Result<(), FtmsError> {
        let len = self.read_varint()?;
        let len = match usize::try_from(len) {
            Ok(len) => len,
            Err(_) => {
                return Err(FtmsError::Malformed {
                    what: "zwift length",
                    detail: "too large",
                });
            }
        };
        self.skip(len, "zwift length")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_gears_are_the_mixed_terrain_ladder() {
        assert_eq!(ZWIFT_HANDSHAKE, b"RideOn");
        assert_eq!(ZWIFT_VIRTUAL_GEAR_RATIOS.len(), 24);
        assert_eq!(ZWIFT_VIRTUAL_GEAR_RATIOS[0], 7_500);
        assert_eq!(ZWIFT_VIRTUAL_GEAR_RATIOS[12], 26_099);
        assert_eq!(ZWIFT_VIRTUAL_GEAR_RATIOS[19], 42_399);
        assert_eq!(*ZWIFT_VIRTUAL_GEAR_RATIOS.last().unwrap(), 54_899);
        for pair in ZWIFT_VIRTUAL_GEAR_RATIOS.windows(2) {
            assert!(pair[0] < pair[1], "{pair:?}");
        }
    }

    #[test]
    fn riding_data_keeps_known_fields_and_skips_the_rest() {
        // Power 100, cadence 90, speed 30.00 km/h (3000), heart rate 140,
        // then field 5 = 2864 and field 6 = 25714 (both observed unknowns).
        let bytes = [
            0x03, 0x08, 0x64, 0x10, 0x5A, 0x18, 0xB8, 0x17, 0x20, 0x8C, 0x01, 0x28, 0xB0, 0x16,
            0x30, 0xF2, 0xC8, 0x01,
        ];
        let data = ZwiftRidingData::parse(&bytes).unwrap();
        assert_eq!(data.power_w, Some(100));
        assert_eq!(data.cadence_rpm, Some(90));
        assert_eq!(data.heart_rate_bpm, Some(140));
        assert!((data.speed_kph.unwrap() - 30.0).abs() < 1.0e-3);

        // Unknown fixed32, then power. Unknown length-delimited, then cadence.
        // A 64-bit unknown field is skipped the same way.
        let fixed32 = [0x03, 0x3D, 0x00, 0x00, 0x00, 0x00, 0x08, 0x64];
        assert_eq!(ZwiftRidingData::parse(&fixed32).unwrap().power_w, Some(100));
        let delimited = [0x03, 0x42, 0x03, 0xAA, 0xBB, 0xCC, 0x10, 0x5A];
        assert_eq!(
            ZwiftRidingData::parse(&delimited).unwrap().cadence_rpm,
            Some(90)
        );
        let fixed64 = [0x03, 0x39, 0, 0, 0, 0, 0, 0, 0, 0, 0x08, 0x01];
        assert_eq!(ZwiftRidingData::parse(&fixed64).unwrap().power_w, Some(1));

        let empty = ZwiftRidingData::parse(&[0x03]).unwrap();
        assert_eq!(empty.power_w, None);
        assert_eq!(empty.speed_kph, None);
    }

    #[test]
    fn riding_data_rejects_a_bad_frame() {
        assert!(ZwiftRidingData::parse(&[]).is_err());
        assert!(ZwiftRidingData::parse(&[0x04, 0x08, 0x01]).is_err());
        assert!(ZwiftRidingData::parse(&[0x03, 0x08, 0x80]).is_err());
        assert!(ZwiftRidingData::parse(&[0x03, 0x42, 0x05, 0xAA]).is_err());
        // Field 1 as a fixed64 is the wrong wire type for a known field.
        assert!(ZwiftRidingData::parse(&[0x03, 0x09, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
        // Wire type 7 is not a protobuf wire type.
        assert!(ZwiftRidingData::parse(&[0x03, 0x57]).is_err());
        // Ten continuation bytes never end the varint.
        assert!(
            ZwiftRidingData::parse(&[
                0x03, 0x08, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80,
            ])
            .is_err()
        );
        // Cadence does not fit in u8.
        assert!(ZwiftRidingData::parse(&[0x03, 0x10, 0xAC, 0x02]).is_err());
    }

    #[test]
    fn negative_grade_is_zigzag_encoded() {
        // Wind 0, grade −10.00% → −1000 → zigzag 1999 (`CF 0F`),
        // Cw 0.51 → 5100 (`EC 27`), Crr 0.004 → 400 (`90 03`).
        let bytes = ZwiftHubCommand::SetSimulation(ZwiftSimulation {
            wind_speed_mps: 0.0,
            grade_pct: -10.0,
            cw_kg_per_m: 0.51,
            crr: 0.004,
        })
        .encode()
        .unwrap();
        assert_eq!(
            bytes,
            vec![
                0x04, 0x22, 0x0B, 0x08, 0x00, 0x10, 0xCF, 0x0F, 0x18, 0xEC, 0x27, 0x20, 0x90, 0x03,
            ]
        );
    }

    #[test]
    fn target_power_and_physical_match_the_hand_vectors() {
        assert_eq!(
            ZwiftHubCommand::SetTargetPower { watts: 100 }
                .encode()
                .unwrap(),
            vec![0x04, 0x18, 0x64]
        );
        assert_eq!(
            ZwiftHubCommand::SetTargetPower { watts: 200 }
                .encode()
                .unwrap(),
            vec![0x04, 0x18, 0xC8, 0x01]
        );
        // Gear 24 is 54899, the three-byte varint `F3 AC 03` from trainer captures.
        assert_eq!(
            ZwiftHubCommand::SetPhysical(ZwiftPhysical {
                gear_ratio_x10000: Some(ZWIFT_VIRTUAL_GEAR_RATIOS[23]),
                bike_weight_kg: None,
                rider_weight_kg: None,
            })
            .encode()
            .unwrap(),
            vec![0x04, 0x2A, 0x04, 0x10, 0xF3, 0xAC, 0x03]
        );
        // Bike 6.41 kg → 641, rider 76.2 kg → 7620, gear present as zero.
        // Tag `0x2A`, length 8. The same bytes are the worked example for a
        // physical write.
        assert_eq!(
            ZwiftHubCommand::SetPhysical(ZwiftPhysical {
                gear_ratio_x10000: Some(0),
                bike_weight_kg: Some(6.41),
                rider_weight_kg: Some(76.2),
            })
            .encode()
            .unwrap(),
            vec![
                0x04, 0x2A, 0x08, 0x10, 0x00, 0x20, 0x81, 0x05, 0x28, 0xC4, 0x3B
            ]
        );
        assert_eq!(
            ZwiftHubCommand::SetPhysical(ZwiftPhysical {
                gear_ratio_x10000: None,
                bike_weight_kg: None,
                rider_weight_kg: None,
            })
            .encode()
            .unwrap(),
            vec![0x04, 0x2A, 0x00]
        );
    }

    #[test]
    fn grade_saturates_and_non_finite_is_an_error() {
        let high = ZwiftHubCommand::SetSimulation(ZwiftSimulation {
            wind_speed_mps: 0.0,
            grade_pct: 1.0e10,
            cw_kg_per_m: 0.0,
            crr: 0.0,
        })
        .encode()
        .unwrap();
        // sint32::MAX zigzag is 0xFFFFFFFE → FE FF FF FF 0F, tag 0x10.
        assert_eq!(&high[5..11], &[0x10, 0xFE, 0xFF, 0xFF, 0xFF, 0x0F]);
        let low = ZwiftHubCommand::SetSimulation(ZwiftSimulation {
            wind_speed_mps: f32::MIN,
            grade_pct: f32::MIN,
            cw_kg_per_m: -1.0,
            crr: -1.0,
        });
        assert!(low.encode().is_ok());
        let nan = ZwiftHubCommand::SetSimulation(ZwiftSimulation {
            wind_speed_mps: 0.0,
            grade_pct: f32::NAN,
            cw_kg_per_m: 0.51,
            crr: 0.004,
        });
        assert_eq!(nan.encode(), Err(FtmsError::NonFinite));
        let nan_mass = ZwiftHubCommand::SetPhysical(ZwiftPhysical {
            gear_ratio_x10000: Some(7_500),
            bike_weight_kg: Some(f32::INFINITY),
            rider_weight_kg: None,
        });
        assert_eq!(nan_mass.encode(), Err(FtmsError::NonFinite));
    }
}
