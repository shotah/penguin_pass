//! 16-bit Bluetooth SIG identifiers and their 128-bit forms.
//!
//! The 128-bit form is the Bluetooth base UUID with the 16-bit value in the
//! first group: `0000XXXX-0000-1000-8000-00805f9b34fb`.

use uuid::Uuid;

/// Build a Bluetooth SIG UUID from a 16-bit assigned number.
#[must_use]
pub const fn uuid16(short: u16) -> Uuid {
    Uuid::from_u128((short as u128) << 96 | 0x0000_0000_0000_1000_8000_0080_5f9b_34fb)
}

/// Fitness Machine Service, 16-bit form.
pub const FTMS_SERVICE_UUID16: u16 = 0x1826;
/// Fitness Machine Service, 128-bit form.
pub const FTMS_SERVICE_UUID: Uuid = uuid16(FTMS_SERVICE_UUID16);

/// Indoor Bike Data, 16-bit form.
pub const INDOOR_BIKE_DATA_UUID16: u16 = 0x2AD2;
/// Indoor Bike Data, 128-bit form.
pub const INDOOR_BIKE_DATA_UUID: Uuid = uuid16(INDOOR_BIKE_DATA_UUID16);

/// Fitness Machine Control Point, 16-bit form.
pub const FITNESS_MACHINE_CONTROL_POINT_UUID16: u16 = 0x2AD9;
/// Fitness Machine Control Point, 128-bit form.
pub const FITNESS_MACHINE_CONTROL_POINT_UUID: Uuid = uuid16(FITNESS_MACHINE_CONTROL_POINT_UUID16);

/// Fitness Machine Status, 16-bit form.
pub const FITNESS_MACHINE_STATUS_UUID16: u16 = 0x2ADA;
/// Fitness Machine Status, 128-bit form.
pub const FITNESS_MACHINE_STATUS_UUID: Uuid = uuid16(FITNESS_MACHINE_STATUS_UUID16);

/// Fitness Machine Feature, 16-bit form.
pub const FITNESS_MACHINE_FEATURE_UUID16: u16 = 0x2ACC;
/// Fitness Machine Feature, 128-bit form.
pub const FITNESS_MACHINE_FEATURE_UUID: Uuid = uuid16(FITNESS_MACHINE_FEATURE_UUID16);

/// Supported Resistance Level Range, 16-bit form.
///
/// The GATT supplement describes this value as three `uint8`s. FTMS clients
/// that talk to real bikes, including GoldenCheetah, read it as `sint16`
/// minimum, `sint16` maximum, and `uint16` increment (6 octets, 0.1 unit).
/// This crate only publishes the identifier.
pub const SUPPORTED_RESISTANCE_LEVEL_RANGE_UUID16: u16 = 0x2AD6;
/// Supported Resistance Level Range, 128-bit form.
pub const SUPPORTED_RESISTANCE_LEVEL_RANGE_UUID: Uuid =
    uuid16(SUPPORTED_RESISTANCE_LEVEL_RANGE_UUID16);

/// Supported Power Range, 16-bit form.
pub const SUPPORTED_POWER_RANGE_UUID16: u16 = 0x2AD8;
/// Supported Power Range, 128-bit form.
pub const SUPPORTED_POWER_RANGE_UUID: Uuid = uuid16(SUPPORTED_POWER_RANGE_UUID16);

/// Cycling Power Service, 16-bit form.
pub const CYCLING_POWER_SERVICE_UUID16: u16 = 0x1818;
/// Cycling Power Service, 128-bit form.
pub const CYCLING_POWER_SERVICE_UUID: Uuid = uuid16(CYCLING_POWER_SERVICE_UUID16);

/// Cycling Power Measurement, 16-bit form.
pub const CYCLING_POWER_MEASUREMENT_UUID16: u16 = 0x2A63;
/// Cycling Power Measurement, 128-bit form.
pub const CYCLING_POWER_MEASUREMENT_UUID: Uuid = uuid16(CYCLING_POWER_MEASUREMENT_UUID16);

/// Heart Rate Service, 16-bit form.
pub const HEART_RATE_SERVICE_UUID16: u16 = 0x180D;
/// Heart Rate Service, 128-bit form.
pub const HEART_RATE_SERVICE_UUID: Uuid = uuid16(HEART_RATE_SERVICE_UUID16);

/// Heart Rate Measurement, 16-bit form.
pub const HEART_RATE_MEASUREMENT_UUID16: u16 = 0x2A37;
/// Heart Rate Measurement, 128-bit form.
pub const HEART_RATE_MEASUREMENT_UUID: Uuid = uuid16(HEART_RATE_MEASUREMENT_UUID16);

/// Tacx FE-C over BLE service.
///
/// Not a Bluetooth SIG uuid. Tacx documents it in "FE-C over BLE" (2015).
pub const FEC_SERVICE_UUID: Uuid = Uuid::from_u128(0x6e40fec1_b5a3_f393_e0a9_e50e24dcca9e);
/// Trainer-to-app notifications on the Tacx FE-C service.
pub const FEC_NOTIFY_UUID: Uuid = Uuid::from_u128(0x6e40fec2_b5a3_f393_e0a9_e50e24dcca9e);
/// App-to-trainer writes on the Tacx FE-C service.
pub const FEC_WRITE_UUID: Uuid = Uuid::from_u128(0x6e40fec3_b5a3_f393_e0a9_e50e24dcca9e);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixteen_bit_uuids_use_the_bluetooth_base() {
        assert_eq!(
            FTMS_SERVICE_UUID.to_string(),
            "00001826-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            INDOOR_BIKE_DATA_UUID.to_string(),
            "00002ad2-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            FITNESS_MACHINE_CONTROL_POINT_UUID.to_string(),
            "00002ad9-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            FITNESS_MACHINE_STATUS_UUID.to_string(),
            "00002ada-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            FITNESS_MACHINE_FEATURE_UUID.to_string(),
            "00002acc-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            SUPPORTED_RESISTANCE_LEVEL_RANGE_UUID.to_string(),
            "00002ad6-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            SUPPORTED_POWER_RANGE_UUID.to_string(),
            "00002ad8-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            CYCLING_POWER_SERVICE_UUID.to_string(),
            "00001818-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            CYCLING_POWER_MEASUREMENT_UUID.to_string(),
            "00002a63-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            HEART_RATE_SERVICE_UUID.to_string(),
            "0000180d-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            HEART_RATE_MEASUREMENT_UUID.to_string(),
            "00002a37-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            FEC_SERVICE_UUID.to_string(),
            "6e40fec1-b5a3-f393-e0a9-e50e24dcca9e"
        );
        assert_eq!(
            FEC_NOTIFY_UUID.to_string(),
            "6e40fec2-b5a3-f393-e0a9-e50e24dcca9e"
        );
        assert_eq!(
            FEC_WRITE_UUID.to_string(),
            "6e40fec3-b5a3-f393-e0a9-e50e24dcca9e"
        );
    }
}
