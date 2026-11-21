//! Bluetooth GATT codecs for a smart trainer.
//!
//! This crate turns bytes into values and values into bytes. It does not open
//! a socket, start a runtime, or talk to BlueZ. The `pp-trainer` crate does that.
//!
//! Field widths follow the Bluetooth GATT Specification Supplement, the
//! Fitness Machine Service 1.0 specification, and the ANT+ FE-C profile for
//! the Tacx BLE frames. See the module docs on each message for the places
//! those documents disagree.

#![forbid(unsafe_code)]

mod control;
mod cycling_power;
mod error;
mod feature;
mod fec;
mod heart_rate;
mod indoor_bike;
mod uuid;
mod wire;

pub use control::{
    ControlPointRequest, ControlPointResponse, ControlResult, DEFAULT_CRR, DEFAULT_CW_KG_PER_M,
    DEFAULT_WIND_SPEED_MPS, IndoorBikeSimulation, OPCODE_REQUEST_CONTROL, OPCODE_RESET,
    OPCODE_RESPONSE, OPCODE_SET_INDOOR_BIKE_SIMULATION, OPCODE_SET_TARGET_POWER,
    OPCODE_START_OR_RESUME, OPCODE_STOP_OR_PAUSE, StopOrPause,
};
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
    CYCLING_POWER_MEASUREMENT_UUID, CYCLING_POWER_MEASUREMENT_UUID16, CYCLING_POWER_SERVICE_UUID,
    CYCLING_POWER_SERVICE_UUID16, FEC_NOTIFY_UUID, FEC_SERVICE_UUID, FEC_WRITE_UUID,
    FITNESS_MACHINE_CONTROL_POINT_UUID, FITNESS_MACHINE_CONTROL_POINT_UUID16,
    FITNESS_MACHINE_FEATURE_UUID, FITNESS_MACHINE_FEATURE_UUID16, FITNESS_MACHINE_STATUS_UUID,
    FITNESS_MACHINE_STATUS_UUID16, FTMS_SERVICE_UUID, FTMS_SERVICE_UUID16,
    HEART_RATE_MEASUREMENT_UUID, HEART_RATE_MEASUREMENT_UUID16, HEART_RATE_SERVICE_UUID,
    HEART_RATE_SERVICE_UUID16, INDOOR_BIKE_DATA_UUID, INDOOR_BIKE_DATA_UUID16,
    SUPPORTED_POWER_RANGE_UUID, SUPPORTED_POWER_RANGE_UUID16,
    SUPPORTED_RESISTANCE_LEVEL_RANGE_UUID, SUPPORTED_RESISTANCE_LEVEL_RANGE_UUID16, uuid16,
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
}
