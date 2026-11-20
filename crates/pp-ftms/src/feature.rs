//! Fitness Machine Feature (0x2ACC) and Supported Power Range (0x2AD8).
//!
//! Feature is two little-endian `uint32` fields: machine features, then target
//! setting features (FTMS Tables 4.3 and 4.4). Supported Power Range is
//! minimum `sint16` watts, maximum `sint16` watts, increment `uint16` watts.

use crate::error::FtmsError;

/// Target Setting Features bit 3: Power Target Setting Supported.
pub const TARGET_FEATURE_POWER: u32 = 1 << 3;
/// Target Setting Features bit 13: Indoor Bike Simulation Parameters Supported.
pub const TARGET_FEATURE_INDOOR_BIKE_SIMULATION: u32 = 1 << 13;

/// The two feature bitfields from Fitness Machine Feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FitnessMachineFeature {
    /// Fitness Machine Features bitfield.
    pub machine_features: u32,
    /// Target Setting Features bitfield.
    pub target_features: u32,
}

impl FitnessMachineFeature {
    /// Parse the 8-octet characteristic. Extra trailing bytes are ignored.
    pub fn parse(bytes: &[u8]) -> Result<Self, FtmsError> {
        if bytes.len() < 8 {
            return Err(FtmsError::Truncated {
                what: "fitness machine feature",
                have: bytes.len(),
                need: 8,
            });
        }
        Ok(Self {
            machine_features: u32_le(&bytes[0..4]),
            target_features: u32_le(&bytes[4..8]),
        })
    }

    /// Target Setting Features bit 13.
    #[must_use]
    pub fn supports_indoor_bike_simulation(self) -> bool {
        self.target_features & TARGET_FEATURE_INDOOR_BIKE_SIMULATION != 0
    }

    /// Target Setting Features bit 3.
    #[must_use]
    pub fn supports_power_target(self) -> bool {
        self.target_features & TARGET_FEATURE_POWER != 0
    }
}

/// Supported Power Range, in watts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedPowerRange {
    /// Lowest target the server accepts, in watts.
    pub minimum_w: i16,
    /// Highest target the server accepts, in watts.
    pub maximum_w: i16,
    /// Smallest step between targets, in watts.
    pub increment_w: u16,
}

impl SupportedPowerRange {
    /// Parse the 6-octet characteristic. Extra trailing bytes are ignored.
    pub fn parse(bytes: &[u8]) -> Result<Self, FtmsError> {
        if bytes.len() < 6 {
            return Err(FtmsError::Truncated {
                what: "supported power range",
                have: bytes.len(),
                need: 6,
            });
        }
        Ok(Self {
            minimum_w: i16::from_le_bytes([bytes[0], bytes[1]]),
            maximum_w: i16::from_le_bytes([bytes[2], bytes[3]]),
            increment_w: u16::from_le_bytes([bytes[4], bytes[5]]),
        })
    }
}

fn u32_le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_bits_for_simulation_and_power() {
        // Machine bit 14 (power measurement) = 0x00004000.
        // Target bit 3 and bit 13 = 0x00002008.
        let bytes = [0x00, 0x40, 0x00, 0x00, 0x08, 0x20, 0x00, 0x00];
        let feature = FitnessMachineFeature::parse(&bytes).unwrap();
        assert!(feature.supports_power_target());
        assert!(feature.supports_indoor_bike_simulation());
        assert_eq!(feature.machine_features, 1 << 14);

        let neither = FitnessMachineFeature::parse(&[0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(!neither.supports_power_target());
        assert!(!neither.supports_indoor_bike_simulation());
        assert!(FitnessMachineFeature::parse(&[0, 0, 0, 0, 0, 0, 0]).is_err());
    }

    #[test]
    fn power_range_is_signed_min_and_max() {
        // Min -10, max 800, increment 1.
        let bytes = [0xF6, 0xFF, 0x20, 0x03, 0x01, 0x00];
        let range = SupportedPowerRange::parse(&bytes).unwrap();
        assert_eq!(range.minimum_w, -10);
        assert_eq!(range.maximum_w, 800);
        assert_eq!(range.increment_w, 1);
        assert!(SupportedPowerRange::parse(&[0xF6, 0xFF, 0x20, 0x03, 0x01]).is_err());
    }
}
