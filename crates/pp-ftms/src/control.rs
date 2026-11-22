//! Fitness Machine Control Point (0x2AD9).
//!
//! Requests are little-endian. Grade, wind speed, rolling resistance, and the
//! wind coefficient are saturated into the wire integer instead of wrapped.
//! A non-finite input is [`FtmsError::NonFinite`].

use pp_core::TrainerCommand;

use crate::error::FtmsError;
use crate::wire::{push_i16, push_u8, saturate_i16, saturate_u8};

/// Opcode: request control of the trainer.
pub const OPCODE_REQUEST_CONTROL: u8 = 0x00;
/// Opcode: reset control parameters.
pub const OPCODE_RESET: u8 = 0x01;
/// Opcode: set target power, `sint16` watts.
pub const OPCODE_SET_TARGET_POWER: u8 = 0x05;
/// Opcode: start or resume the training session.
pub const OPCODE_START_OR_RESUME: u8 = 0x07;
/// Opcode: stop or pause. Followed by one parameter octet.
pub const OPCODE_STOP_OR_PAUSE: u8 = 0x08;
/// Opcode: set indoor bike simulation parameters.
pub const OPCODE_SET_INDOOR_BIKE_SIMULATION: u8 = 0x11;
/// Opcode of a control-point response indication.
pub const OPCODE_RESPONSE: u8 = 0x80;

/// Still air, in meters per second. The usual indoor-bike simulation default.
pub const DEFAULT_WIND_SPEED_MPS: f32 = 0.0;
/// Coefficient of rolling resistance, dimensionless.
///
/// 0.004 is a typical road tire and the value training apps send when the
/// rider has not picked one. On the wire that is 40 (`uint8`, resolution 0.0001).
pub const DEFAULT_CRR: f32 = 0.004;
/// Wind resistance coefficient, in kilograms per meter.
///
/// 0.51 kg/m is the usual FTMS indoor-bike default. On the wire that is 51
/// (`uint8`, resolution 0.01 kg/m).
pub const DEFAULT_CW_KG_PER_M: f32 = 0.51;

/// Parameter of Stop or Pause (FTMS Table 4.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOrPause {
    /// End the session. Wire value `0x01`.
    Stop,
    /// Pause the session. Wire value `0x02`.
    Pause,
}

/// Indoor bike simulation parameters (FTMS Table 4.20).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndoorBikeSimulation {
    /// Wind speed, in meters per second. Positive is a headwind.
    /// Wire unit is 0.001 m/s, `sint16`.
    pub wind_speed_mps: f32,
    /// Grade, in percent. Positive is uphill. `5.0` is a 5% grade.
    /// Wire unit is 0.01%, `sint16`.
    pub grade_pct: f32,
    /// Coefficient of rolling resistance, dimensionless.
    /// Wire unit is 0.0001, `uint8` (0 to 0.0255).
    pub crr: f32,
    /// Wind resistance coefficient, in kilograms per meter.
    /// Wire unit is 0.01 kg/m, `uint8` (0 to 2.55).
    pub cw_kg_per_m: f32,
}

impl IndoorBikeSimulation {
    /// Still air, [`DEFAULT_CRR`], [`DEFAULT_CW_KG_PER_M`], and the given grade.
    #[must_use]
    pub const fn standard(grade_pct: f32) -> Self {
        Self {
            wind_speed_mps: DEFAULT_WIND_SPEED_MPS,
            grade_pct,
            crr: DEFAULT_CRR,
            cw_kg_per_m: DEFAULT_CW_KG_PER_M,
        }
    }
}

/// A control-point procedure this crate can send.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlPointRequest {
    /// Ask the trainer for the control permission. No parameter.
    RequestControl,
    /// Reset targets. No parameter.
    Reset,
    /// ERG mode. `watts` is in watts and already in the `sint16` range.
    SetTargetPower {
        /// Target power, in watts.
        watts: i16,
    },
    /// Start or resume. No parameter.
    StartOrResume,
    /// Stop or pause.
    StopOrPause(StopOrPause),
    /// Indoor bike simulation (grade, wind, rolling resistance, drag).
    SetIndoorBikeSimulation(IndoorBikeSimulation),
}

impl ControlPointRequest {
    /// Map a game command onto a control-point request.
    ///
    /// [`TrainerCommand::SetGrade`] keeps `simulation`'s wind, `crr`, and `cw`,
    /// and replaces the grade. [`TrainerCommand::SetTargetPower`] saturates
    /// into `sint16` watts (the wire type); values above 32767 become 32767.
    /// [`TrainerCommand::SetVirtualGear`] and [`TrainerCommand::SetRoadFeel`]
    /// are not FTMS control-point opcodes; they return `None`.
    #[must_use]
    pub fn from_trainer_command(
        command: TrainerCommand,
        simulation: IndoorBikeSimulation,
    ) -> Option<Self> {
        match command {
            TrainerCommand::SetGrade { grade_pct } => {
                Some(Self::SetIndoorBikeSimulation(IndoorBikeSimulation {
                    grade_pct,
                    ..simulation
                }))
            }
            TrainerCommand::SetTargetPower { watts } => Some(Self::SetTargetPower {
                watts: i16::try_from(watts).unwrap_or(i16::MAX),
            }),
            TrainerCommand::SetVirtualGear { .. } | TrainerCommand::SetRoadFeel { .. } => None,
        }
    }

    /// Encode the request. Grade and the other simulation fields saturate.
    pub fn encode(&self) -> Result<Vec<u8>, FtmsError> {
        let mut out = Vec::with_capacity(7);
        match *self {
            Self::RequestControl => push_u8(&mut out, OPCODE_REQUEST_CONTROL),
            Self::Reset => push_u8(&mut out, OPCODE_RESET),
            Self::SetTargetPower { watts } => {
                push_u8(&mut out, OPCODE_SET_TARGET_POWER);
                push_i16(&mut out, watts);
            }
            Self::StartOrResume => push_u8(&mut out, OPCODE_START_OR_RESUME),
            Self::StopOrPause(action) => {
                push_u8(&mut out, OPCODE_STOP_OR_PAUSE);
                push_u8(
                    &mut out,
                    match action {
                        StopOrPause::Stop => 0x01,
                        StopOrPause::Pause => 0x02,
                    },
                );
            }
            Self::SetIndoorBikeSimulation(sim) => {
                push_u8(&mut out, OPCODE_SET_INDOOR_BIKE_SIMULATION);
                push_i16(&mut out, saturate_i16(sim.wind_speed_mps, 1000.0)?);
                push_i16(&mut out, saturate_i16(sim.grade_pct, 100.0)?);
                push_u8(&mut out, saturate_u8(sim.crr, 10_000.0)?);
                push_u8(&mut out, saturate_u8(sim.cw_kg_per_m, 100.0)?);
            }
        }
        Ok(out)
    }
}

/// Result code from a control-point response (FTMS Table 4.24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlResult {
    /// `0x01`. The procedure completed.
    Success,
    /// `0x02`. The server does not implement that opcode.
    NotSupported,
    /// `0x03`. A parameter was out of range or the wrong size.
    InvalidParameter,
    /// `0x04`. The server rejected the procedure.
    OperationFailed,
    /// `0x05`. This client does not hold the control permission.
    ControlNotPermitted,
    /// A code this crate does not name, including the reserved range.
    Reserved(u8),
}

impl ControlResult {
    fn from_code(code: u8) -> Self {
        match code {
            0x01 => Self::Success,
            0x02 => Self::NotSupported,
            0x03 => Self::InvalidParameter,
            0x04 => Self::OperationFailed,
            0x05 => Self::ControlNotPermitted,
            other => Self::Reserved(other),
        }
    }
}

impl std::fmt::Display for ControlResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::NotSupported => write!(f, "not supported"),
            Self::InvalidParameter => write!(f, "invalid parameter"),
            Self::OperationFailed => write!(f, "operation failed"),
            Self::ControlNotPermitted => write!(f, "control not permitted"),
            Self::Reserved(code) => write!(f, "reserved result {code:#04x}"),
        }
    }
}

/// A control-point response indication.
///
/// The wire form is response opcode `0x80`, the request opcode, and a result
/// code. Extra parameter bytes (spin-down speeds, for example) are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlPointResponse {
    /// Opcode of the procedure this response answers.
    pub request_opcode: u8,
    /// Whether the procedure succeeded.
    pub result: ControlResult,
}

impl ControlPointResponse {
    /// Parse a response indication.
    pub fn parse(bytes: &[u8]) -> Result<Self, FtmsError> {
        if bytes.len() < 3 {
            return Err(FtmsError::Truncated {
                what: "control point response",
                have: bytes.len(),
                need: 3,
            });
        }
        if bytes[0] != OPCODE_RESPONSE {
            return Err(FtmsError::Malformed {
                what: "control point response",
                detail: "first octet is not 0x80",
            });
        }
        Ok(Self {
            request_opcode: bytes[1],
            result: ControlResult::from_code(bytes[2]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_opcodes_match_the_spec() {
        assert_eq!(
            ControlPointRequest::RequestControl.encode().unwrap(),
            vec![0x00]
        );
        assert_eq!(ControlPointRequest::Reset.encode().unwrap(), vec![0x01]);
        assert_eq!(
            ControlPointRequest::StartOrResume.encode().unwrap(),
            vec![0x07]
        );
        assert_eq!(
            ControlPointRequest::StopOrPause(StopOrPause::Stop)
                .encode()
                .unwrap(),
            vec![0x08, 0x01]
        );
        assert_eq!(
            ControlPointRequest::StopOrPause(StopOrPause::Pause)
                .encode()
                .unwrap(),
            vec![0x08, 0x02]
        );
    }

    #[test]
    fn negative_power_and_power_saturation() {
        assert_eq!(
            ControlPointRequest::SetTargetPower { watts: -20 }
                .encode()
                .unwrap(),
            vec![0x05, 0xEC, 0xFF]
        );
        let wide = ControlPointRequest::from_trainer_command(
            TrainerCommand::SetTargetPower { watts: 40_000 },
            IndoorBikeSimulation::standard(0.0),
        )
        .unwrap();
        assert_eq!(wide.encode().unwrap(), vec![0x05, 0xFF, 0x7F]);
    }

    #[test]
    fn negative_grade_is_minus_ten_percent() {
        let bytes = IndoorBikeSimulation::standard(-10.0).pipe_encode();
        // Wind 0, grade -1000 = 0xFC18, Crr 40, Cw 51.
        assert_eq!(bytes, vec![0x11, 0x00, 0x00, 0x18, 0xFC, 0x28, 0x33]);
    }

    #[test]
    fn grade_saturates_at_sint16_limits() {
        let high = ControlPointRequest::SetIndoorBikeSimulation(IndoorBikeSimulation {
            wind_speed_mps: 0.0,
            grade_pct: 10_000.0,
            crr: 0.0,
            cw_kg_per_m: 0.0,
        });
        assert_eq!(
            high.encode().unwrap(),
            vec![0x11, 0x00, 0x00, 0xFF, 0x7F, 0x00, 0x00]
        );
        let low = ControlPointRequest::SetIndoorBikeSimulation(IndoorBikeSimulation {
            wind_speed_mps: -100.0,
            grade_pct: -10_000.0,
            crr: -1.0,
            cw_kg_per_m: 9.0,
        });
        // Wind saturates to i16::MIN (00 80), grade to i16::MIN, Crr to 0, Cw to 255.
        assert_eq!(
            low.encode().unwrap(),
            vec![0x11, 0x00, 0x80, 0x00, 0x80, 0x00, 0xFF]
        );
    }

    #[test]
    fn non_finite_grade_is_an_error() {
        let request = ControlPointRequest::SetIndoorBikeSimulation(IndoorBikeSimulation {
            wind_speed_mps: 0.0,
            grade_pct: f32::NAN,
            crr: 0.004,
            cw_kg_per_m: 0.51,
        });
        assert_eq!(request.encode(), Err(FtmsError::NonFinite));
    }

    #[test]
    fn trainer_command_grade_uses_simulation_context() {
        let request = ControlPointRequest::from_trainer_command(
            TrainerCommand::SetGrade { grade_pct: 5.0 },
            IndoorBikeSimulation::standard(0.0),
        )
        .unwrap();
        // 5.00% = 500 = 0x01F4.
        assert_eq!(
            request.encode().unwrap(),
            vec![0x11, 0x00, 0x00, 0xF4, 0x01, 0x28, 0x33]
        );
    }

    #[test]
    fn virtual_gear_does_not_map_to_control_point() {
        assert_eq!(
            ControlPointRequest::from_trainer_command(
                TrainerCommand::SetVirtualGear { gear: 12 },
                IndoorBikeSimulation::standard(0.0),
            ),
            None,
        );
    }

    #[test]
    fn response_codes() {
        let cases = [
            ([0x80, 0x00, 0x01], ControlResult::Success, 0x00),
            ([0x80, 0x05, 0x02], ControlResult::NotSupported, 0x05),
            ([0x80, 0x11, 0x03], ControlResult::InvalidParameter, 0x11),
            ([0x80, 0x07, 0x04], ControlResult::OperationFailed, 0x07),
            ([0x80, 0x00, 0x05], ControlResult::ControlNotPermitted, 0x00),
            ([0x80, 0x01, 0x06], ControlResult::Reserved(0x06), 0x01),
        ];
        for (bytes, result, opcode) in cases {
            let parsed = ControlPointResponse::parse(&bytes).unwrap();
            assert_eq!(parsed.result, result);
            assert_eq!(parsed.request_opcode, opcode);
        }
        // Trailing spin-down parameter is ignored.
        let extra = ControlPointResponse::parse(&[0x80, 0x13, 0x01, 0x00, 0x10]).unwrap();
        assert_eq!(extra.result, ControlResult::Success);
        assert!(ControlPointResponse::parse(&[0x80, 0x00]).is_err());
        assert!(ControlPointResponse::parse(&[0x00, 0x00, 0x01]).is_err());
        assert!(ControlPointResponse::parse(&[]).is_err());
    }

    trait EncodeSim {
        fn pipe_encode(self) -> Vec<u8>;
    }

    impl EncodeSim for IndoorBikeSimulation {
        fn pipe_encode(self) -> Vec<u8> {
            ControlPointRequest::SetIndoorBikeSimulation(self)
                .encode()
                .unwrap()
        }
    }
}
