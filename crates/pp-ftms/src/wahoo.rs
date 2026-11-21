//! Wahoo Cycling Power Service extension (WCPS).
//!
//! Legacy KICKR v1, v2, and v3, KICKR SNAP, and KICKR BIKE take these commands
//! on characteristic `A026E005-0A7D-4AB3-97FA-F1500F9FEB8B`, under the standard
//! Cycling Power Service (`0x1818`). The trainer is unlocked before any other
//! command. A response is the result code followed by the request opcode.
//!
//! Grade and wind use an offset `u16`, not the signed FTMS units. Simulation
//! parameters are three little-endian `u16`s. Negative inputs saturate to 0
//! and large inputs saturate to `u16::MAX`. A non-finite input is
//! [`FtmsError::NonFinite`].

use crate::error::FtmsError;

/// Unlock the trainer. The payload is the fixed key `EE FC`.
pub const WAHOO_OP_UNLOCK: u8 = 0x20;
/// Set a basic resistance level.
pub const WAHOO_OP_SET_LEVEL: u8 = 0x41;
/// Set target power. The payload is little-endian watts.
pub const WAHOO_OP_SET_ERG_POWER: u8 = 0x42;
/// Set rider weight, rolling resistance, and the wind coefficient.
pub const WAHOO_OP_SET_SIM_PARAMETERS: u8 = 0x43;
/// Set simulation grade. `0x8000` is flat.
pub const WAHOO_OP_SET_SIM_GRADE: u8 = 0x46;
/// Set simulation wind speed. `0x8000` is still air.
pub const WAHOO_OP_SET_SIM_WIND_SPEED: u8 = 0x47;
/// The trainer accepted the command.
pub const WAHOO_RESULT_SUCCESS: u8 = 0x01;

/// Rider and bike parameters for opcode `0x43`.
///
/// On the wire each field is a little-endian `u16`:
///
/// * weight is kilograms times 100 (0.01 kg)
/// * rolling resistance is `crr * 10_000`
/// * wind resistance is kilograms per meter times 1_000 (0.001 kg/m)
///
/// Rolling resistance is the scale write-ups disagree on. Some multiply by
/// 1_000. A captured WCPS frame (StephenDone) sends `28 00` for a coefficient
/// of 0.004, and `0.004 * 10_000` is 40 (`0x0028`). This codec uses 10_000.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WahooSimParameters {
    /// Rider mass, in kilograms.
    pub weight_kg: f32,
    /// Coefficient of rolling resistance, dimensionless.
    pub crr: f32,
    /// Wind resistance coefficient, in kilograms per meter.
    pub wind_resistance_kg_per_m: f32,
}

/// A WCPS command this crate can send.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WahooCommand {
    /// Unlock. Wire form `[0x20, 0xEE, 0xFC]`.
    Unlock,
    /// ERG mode. `watts` is already in the `u16` range.
    SetErgPower {
        /// Target power, in watts.
        watts: u16,
    },
    /// Weight, rolling resistance, and wind resistance.
    SetSimParameters(WahooSimParameters),
    /// Grade, in percent. Positive is uphill. The wire range is −100% to +100%.
    SetSimGrade {
        /// Grade, in percent. `5.0` is a 5% grade.
        grade_pct: f32,
    },
    /// Wind speed, in meters per second. Positive is a headwind.
    SetSimWindSpeed {
        /// Wind speed, in meters per second.
        wind_speed_mps: f32,
    },
}

impl WahooCommand {
    /// Encode the command.
    ///
    /// Grade is clamped to −100%…+100% and then stored as
    /// `(((grade / 100) + 1) * 32768)`, so 0% is `0x8000`, −100% is `0x0000`,
    /// and +100% is `0xFFFF` (the unclamped product is 65536, one past `u16`).
    /// Wind is `(wind_mps + 32.768) * 1000`, so still air is `0x8000`.
    /// Simulation parameters clamp negatives to 0 and saturate at `u16::MAX`.
    /// A non-finite float is [`FtmsError::NonFinite`].
    pub fn encode(&self) -> Result<Vec<u8>, FtmsError> {
        match *self {
            Self::Unlock => Ok(vec![WAHOO_OP_UNLOCK, 0xEE, 0xFC]),
            Self::SetErgPower { watts } => Ok(opcode_u16(WAHOO_OP_SET_ERG_POWER, watts)),
            Self::SetSimParameters(params) => {
                let mut out = Vec::with_capacity(7);
                out.push(WAHOO_OP_SET_SIM_PARAMETERS);
                out.extend_from_slice(&scale_u16(params.weight_kg, 100.0)?.to_le_bytes());
                // `crr * 10_000`: 0.004 is 40 (`0x0028`), matching the captured frame.
                out.extend_from_slice(&scale_u16(params.crr, 10_000.0)?.to_le_bytes());
                out.extend_from_slice(
                    &scale_u16(params.wind_resistance_kg_per_m, 1_000.0)?.to_le_bytes(),
                );
                Ok(out)
            }
            Self::SetSimGrade { grade_pct } => {
                if !grade_pct.is_finite() {
                    return Err(FtmsError::NonFinite);
                }
                let grade = grade_pct.clamp(-100.0, 100.0);
                let scaled = (((grade / 100.0) + 1.0) * 32_768.0)
                    .round()
                    .clamp(0.0, f32::from(u16::MAX));
                Ok(opcode_u16(WAHOO_OP_SET_SIM_GRADE, unit_u16(scaled)))
            }
            Self::SetSimWindSpeed { wind_speed_mps } => {
                if !wind_speed_mps.is_finite() {
                    return Err(FtmsError::NonFinite);
                }
                let scaled = ((wind_speed_mps + 32.768) * 1_000.0)
                    .round()
                    .clamp(0.0, f32::from(u16::MAX));
                Ok(opcode_u16(WAHOO_OP_SET_SIM_WIND_SPEED, unit_u16(scaled)))
            }
        }
    }
}

/// A trainer response. The first octet is the result, the second is the opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WahooResponse {
    /// `0x01` when the trainer accepted the command. See [`WAHOO_RESULT_SUCCESS`].
    pub result_code: u8,
    /// Opcode of the command this response answers.
    pub request_opcode: u8,
}

impl WahooResponse {
    /// Parse a response. Short buffers return [`FtmsError::Truncated`].
    ///
    /// Bytes after the opcode are ignored. The function does not panic.
    pub fn parse(bytes: &[u8]) -> Result<Self, FtmsError> {
        if bytes.len() < 2 {
            return Err(FtmsError::Truncated {
                what: "wahoo response",
                have: bytes.len(),
                need: 2,
            });
        }
        Ok(Self {
            result_code: bytes[0],
            request_opcode: bytes[1],
        })
    }

    /// `true` when [`Self::result_code`] is [`WAHOO_RESULT_SUCCESS`].
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.result_code == WAHOO_RESULT_SUCCESS
    }
}

fn opcode_u16(opcode: u8, value: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(3);
    out.push(opcode);
    out.extend_from_slice(&value.to_le_bytes());
    out
}

/// Scale by an exact integer and saturate to `u16`. Negatives become 0.
fn scale_u16(value: f32, per_unit: f32) -> Result<u16, FtmsError> {
    if !value.is_finite() {
        return Err(FtmsError::NonFinite);
    }
    let scaled = (value * per_unit).round();
    if scaled <= 0.0 {
        Ok(0)
    } else if scaled >= f32::from(u16::MAX) {
        Ok(u16::MAX)
    } else {
        Ok(unit_u16(scaled))
    }
}

fn unit_u16(scaled: f32) -> u16 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        scaled as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_and_erg_match_the_hand_vectors() {
        assert_eq!(
            WahooCommand::Unlock.encode().unwrap(),
            vec![0x20, 0xEE, 0xFC]
        );
        assert_eq!(
            WahooCommand::SetErgPower { watts: 100 }.encode().unwrap(),
            vec![0x42, 0x64, 0x00]
        );
        assert_eq!(
            WahooCommand::SetErgPower { watts: 0 }.encode().unwrap(),
            vec![0x42, 0x00, 0x00]
        );
        assert_eq!(
            WahooCommand::SetErgPower { watts: u16::MAX }
                .encode()
                .unwrap(),
            vec![0x42, 0xFF, 0xFF]
        );
    }

    #[test]
    fn flat_grade_and_grade_endpoints() {
        assert_eq!(
            WahooCommand::SetSimGrade { grade_pct: 0.0 }
                .encode()
                .unwrap(),
            vec![0x46, 0x00, 0x80]
        );
        assert_eq!(
            WahooCommand::SetSimGrade { grade_pct: -100.0 }
                .encode()
                .unwrap(),
            vec![0x46, 0x00, 0x00]
        );
        assert_eq!(
            WahooCommand::SetSimGrade { grade_pct: 100.0 }
                .encode()
                .unwrap(),
            vec![0x46, 0xFF, 0xFF]
        );
        // 50% → 1.5 * 32768 = 49152 = 0xC000. −50% → 16384 = 0x4000.
        assert_eq!(
            WahooCommand::SetSimGrade { grade_pct: 50.0 }
                .encode()
                .unwrap(),
            vec![0x46, 0x00, 0xC0]
        );
        assert_eq!(
            WahooCommand::SetSimGrade { grade_pct: -50.0 }
                .encode()
                .unwrap(),
            vec![0x46, 0x00, 0x40]
        );
    }

    #[test]
    fn grade_and_wind_saturate() {
        assert_eq!(
            WahooCommand::SetSimGrade {
                grade_pct: 10_000.0
            }
            .encode()
            .unwrap(),
            vec![0x46, 0xFF, 0xFF]
        );
        assert_eq!(
            WahooCommand::SetSimGrade {
                grade_pct: -10_000.0
            }
            .encode()
            .unwrap(),
            vec![0x46, 0x00, 0x00]
        );
        assert_eq!(
            WahooCommand::SetSimWindSpeed {
                wind_speed_mps: 0.0
            }
            .encode()
            .unwrap(),
            vec![0x47, 0x00, 0x80]
        );
        // 1 m/s → (1 + 32.768) * 1000 = 33768 = 0x83E8.
        assert_eq!(
            WahooCommand::SetSimWindSpeed {
                wind_speed_mps: 1.0
            }
            .encode()
            .unwrap(),
            vec![0x47, 0xE8, 0x83]
        );
        assert_eq!(
            WahooCommand::SetSimWindSpeed {
                wind_speed_mps: 10_000.0
            }
            .encode()
            .unwrap(),
            vec![0x47, 0xFF, 0xFF]
        );
        assert_eq!(
            WahooCommand::SetSimWindSpeed {
                wind_speed_mps: -10_000.0
            }
            .encode()
            .unwrap(),
            vec![0x47, 0x00, 0x00]
        );
    }

    #[test]
    fn sim_parameters_use_crr_times_ten_thousand() {
        // 75.00 kg → 7500 = 0x1D4C. crr 0.004 → 40 = 0x0028 (the captured
        // `28 00` frame). Wind coefficient 0.51 kg/m → 510 = 0x01FE.
        let bytes = WahooCommand::SetSimParameters(WahooSimParameters {
            weight_kg: 75.0,
            crr: 0.004,
            wind_resistance_kg_per_m: 0.51,
        })
        .encode()
        .unwrap();
        assert_eq!(bytes, vec![0x43, 0x4C, 0x1D, 0x28, 0x00, 0xFE, 0x01]);
    }

    #[test]
    fn sim_parameters_clamp_and_reject_non_finite() {
        let low = WahooCommand::SetSimParameters(WahooSimParameters {
            weight_kg: -5.0,
            crr: -1.0,
            wind_resistance_kg_per_m: -0.2,
        });
        assert_eq!(
            low.encode().unwrap(),
            vec![0x43, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
        let high = WahooCommand::SetSimParameters(WahooSimParameters {
            weight_kg: 100_000.0,
            crr: 80.0,
            wind_resistance_kg_per_m: 900.0,
        });
        assert_eq!(
            high.encode().unwrap(),
            vec![0x43, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        let nan = WahooCommand::SetSimParameters(WahooSimParameters {
            weight_kg: 75.0,
            crr: f32::NAN,
            wind_resistance_kg_per_m: 0.51,
        });
        assert_eq!(nan.encode(), Err(FtmsError::NonFinite));
        assert_eq!(
            WahooCommand::SetSimGrade {
                grade_pct: f32::INFINITY
            }
            .encode(),
            Err(FtmsError::NonFinite)
        );
        assert_eq!(
            WahooCommand::SetSimWindSpeed {
                wind_speed_mps: f32::NAN
            }
            .encode(),
            Err(FtmsError::NonFinite)
        );
    }

    #[test]
    fn response_codes() {
        let ok = WahooResponse::parse(&[0x01, 0x42]).unwrap();
        assert_eq!(ok.result_code, WAHOO_RESULT_SUCCESS);
        assert_eq!(ok.request_opcode, WAHOO_OP_SET_ERG_POWER);
        assert!(ok.is_success());
        let failed = WahooResponse::parse(&[0x00, 0x46, 0xFF]).unwrap();
        assert!(!failed.is_success());
        assert_eq!(failed.request_opcode, WAHOO_OP_SET_SIM_GRADE);
        assert!(WahooResponse::parse(&[0x01]).is_err());
        assert!(WahooResponse::parse(&[]).is_err());
    }
}
