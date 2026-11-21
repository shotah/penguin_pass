//! Bluetooth GATT codecs for a smart trainer.
//!
//! This crate turns bytes into values and values into bytes. It does not open
//! a socket, start a runtime, or talk to BlueZ. The `pp-trainer` crate does that.
//!
//! Field widths follow the Bluetooth GATT Specification Supplement, the
//! Fitness Machine Service 1.0 specification, and the ANT+ FE-C profile for
//! the Tacx BLE frames. Wahoo's legacy trainer control, the Zwift trainer
//! service, and Cycling Speed and Cadence are the other layouts. See the
//! module docs on each message for the places those documents disagree.

#![forbid(unsafe_code)]

mod control;
mod csc;
mod cycling_power;
mod error;
mod feature;
mod fec;
mod heart_rate;
mod indoor_bike;
mod uuid;
mod wahoo;
mod wire;
mod zwift;

pub use control::{
    ControlPointRequest, ControlPointResponse, ControlResult, DEFAULT_CRR, DEFAULT_CW_KG_PER_M,
    DEFAULT_WIND_SPEED_MPS, IndoorBikeSimulation, OPCODE_REQUEST_CONTROL, OPCODE_RESET,
    OPCODE_RESPONSE, OPCODE_SET_INDOOR_BIKE_SIMULATION, OPCODE_SET_TARGET_POWER,
    OPCODE_START_OR_RESUME, OPCODE_STOP_OR_PAUSE, StopOrPause,
};
pub use csc::{CscMeasurement, WheelRevolutionData, wheel_speed_kph};
pub use cycling_power::{CrankRevolutionData, CyclingPowerMeasurement, crank_cadence_rpm};
pub use error::FtmsError;
pub use feature::{
    FitnessMachineFeature, SupportedPowerRange, TARGET_FEATURE_INDOOR_BIKE_SIMULATION,
    TARGET_FEATURE_POWER,
};
pub use fec::{
    FecChecksum, FecData, FecMessage, GeneralFeData, SpecificTrainerData, TargetPower,
    TrackResistance, UserConfiguration,
};
pub use heart_rate::HeartRateMeasurement;
pub use indoor_bike::IndoorBikeData;
pub use uuid::{
    CSC_MEASUREMENT_UUID, CSC_MEASUREMENT_UUID16, CSC_SERVICE_UUID, CSC_SERVICE_UUID16,
    CYCLING_POWER_MEASUREMENT_UUID, CYCLING_POWER_MEASUREMENT_UUID16, CYCLING_POWER_SERVICE_UUID,
    CYCLING_POWER_SERVICE_UUID16, FEC_NOTIFY_UUID, FEC_SERVICE_UUID, FEC_WRITE_UUID,
    FITNESS_MACHINE_CONTROL_POINT_UUID, FITNESS_MACHINE_CONTROL_POINT_UUID16,
    FITNESS_MACHINE_FEATURE_UUID, FITNESS_MACHINE_FEATURE_UUID16, FITNESS_MACHINE_STATUS_UUID,
    FITNESS_MACHINE_STATUS_UUID16, FTMS_SERVICE_UUID, FTMS_SERVICE_UUID16,
    HEART_RATE_MEASUREMENT_UUID, HEART_RATE_MEASUREMENT_UUID16, HEART_RATE_SERVICE_UUID,
    HEART_RATE_SERVICE_UUID16, INDOOR_BIKE_DATA_UUID, INDOOR_BIKE_DATA_UUID16,
    SUPPORTED_POWER_RANGE_UUID, SUPPORTED_POWER_RANGE_UUID16,
    SUPPORTED_RESISTANCE_LEVEL_RANGE_UUID, SUPPORTED_RESISTANCE_LEVEL_RANGE_UUID16,
    WAHOO_TRAINER_CONTROL_UUID, ZWIFT_TRAINER_NOTIFY_UUID, ZWIFT_TRAINER_RESPONSE_UUID,
    ZWIFT_TRAINER_SERVICE_UUID, ZWIFT_TRAINER_SERVICE_UUID16, ZWIFT_TRAINER_WRITE_UUID, uuid16,
};
pub use wahoo::{
    WAHOO_OP_SET_ERG_POWER, WAHOO_OP_SET_LEVEL, WAHOO_OP_SET_SIM_GRADE,
    WAHOO_OP_SET_SIM_PARAMETERS, WAHOO_OP_SET_SIM_WIND_SPEED, WAHOO_OP_UNLOCK,
    WAHOO_RESULT_SUCCESS, WahooCommand, WahooResponse, WahooSimParameters,
};
pub use zwift::{
    ZWIFT_HANDSHAKE, ZWIFT_MSG_HUB_COMMAND, ZWIFT_MSG_RIDING_DATA, ZWIFT_VIRTUAL_GEAR_RATIOS,
    ZwiftHubCommand, ZwiftPhysical, ZwiftRidingData, ZwiftSimulation,
};

#[cfg(test)]
mod fuzz {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn arbitrary_bytes_do_not_panic(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
            let _ = IndoorBikeData::parse(&bytes);
            let _ = CyclingPowerMeasurement::parse(&bytes);
            let _ = HeartRateMeasurement::parse(&bytes);
            let _ = ControlPointResponse::parse(&bytes);
            let _ = FitnessMachineFeature::parse(&bytes);
            let _ = SupportedPowerRange::parse(&bytes);
            let _ = FecMessage::parse(&bytes);
            let _ = WahooResponse::parse(&bytes);
            let _ = ZwiftRidingData::parse(&bytes);
            let _ = CscMeasurement::parse(&bytes);
        }

        #[test]
        fn wahoo_grades_saturate_instead_of_wrapping(grade in -100_000.0f32..100_000.0f32) {
            let bytes = WahooCommand::SetSimGrade { grade_pct: grade }
                .encode()
                .unwrap();
            prop_assert_eq!(bytes.len(), 3);
            prop_assert_eq!(bytes[0], WAHOO_OP_SET_SIM_GRADE);
            let raw = u16::from_le_bytes([bytes[1], bytes[2]]);
            let clamped = grade.clamp(-100.0, 100.0);
            let scaled = (((clamped / 100.0) + 1.0) * 32_768.0)
                .round()
                .clamp(0.0, f32::from(u16::MAX));
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let expected = scaled as u16;
            prop_assert_eq!(raw, expected);
        }

        #[test]
        fn zwift_grades_saturate_into_sint32(grade in -1.0e20f32..1.0e20f32) {
            let bytes = ZwiftHubCommand::SetSimulation(ZwiftSimulation {
                wind_speed_mps: 0.0,
                grade_pct: grade,
                cw_kg_per_m: 0.0,
                crr: 0.0,
            })
            .encode()
            .unwrap();
            prop_assert_eq!(zwift_grade_x100(&bytes), expected_grade_x100(grade));
        }

        #[test]
        fn finite_grades_saturate_instead_of_wrapping(grade in -100_000.0f32..100_000.0f32) {
            let request = ControlPointRequest::SetIndoorBikeSimulation(IndoorBikeSimulation {
                wind_speed_mps: 0.0,
                grade_pct: grade,
                crr: 0.0,
                cw_kg_per_m: 0.0,
            });
            let bytes = request.encode().unwrap();
            prop_assert_eq!(bytes.len(), 7);
            let raw = i16::from_le_bytes([bytes[3], bytes[4]]);
            let scaled = (grade * 100.0).round();
            let expected = if scaled >= f32::from(i16::MAX) {
                i16::MAX
            } else if scaled <= f32::from(i16::MIN) {
                i16::MIN
            } else {
                #[allow(clippy::cast_possible_truncation)]
                {
                    scaled as i16
                }
            };
            prop_assert_eq!(raw, expected);
        }
    }

    fn expected_grade_x100(grade: f32) -> i32 {
        let scaled = (grade * 100.0).round();
        if scaled >= i32::MAX as f32 {
            i32::MAX
        } else if scaled <= i32::MIN as f32 {
            i32::MIN
        } else {
            #[allow(clippy::cast_possible_truncation)]
            {
                scaled as i32
            }
        }
    }

    fn zigzag_decode(encoded: u32) -> i32 {
        #[allow(clippy::cast_possible_wrap)]
        let magnitude = (encoded >> 1) as i32;
        magnitude ^ -i32::from((encoded & 1) == 1)
    }

    /// Grade field of a simulation command whose other fields are zero.
    fn zwift_grade_x100(bytes: &[u8]) -> i32 {
        assert_eq!(bytes[0], ZWIFT_MSG_HUB_COMMAND);
        assert_eq!(bytes[1], 0x22);
        let len = usize::from(bytes[2]);
        assert_eq!(bytes.len(), 3 + len);
        assert_eq!(&bytes[3..5], &[0x08, 0x00]);
        assert_eq!(bytes[5], 0x10);
        let mut pos = 6;
        let mut raw = 0u32;
        for shift in (0..32).step_by(7) {
            let byte = bytes[pos];
            pos += 1;
            raw |= u32::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                break;
            }
        }
        assert_eq!(&bytes[pos..], &[0x18, 0x00, 0x20, 0x00]);
        zigzag_decode(raw)
    }
}
