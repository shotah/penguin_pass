//! Tacx FE-C over Bluetooth.
//!
//! Tacx wraps an ANT+ FE-C data page in a 13-byte ANT serial frame and carries
//! it on a Nordic UART-style service. The layout of each page follows the
//! ANT+ Fitness Equipment profile.
//!
//! Two checksums show up in the wild. The ANT serial spec, GoldenCheetah, and
//! the worked example in Tacx's "FE-C over BLE" how-to XOR every byte,
//! including the sync byte. The how-to's prose and pycycling sum every byte
//! except the sync byte. Incoming frames are accepted when either matches.
//! [`FecMessage::checksum`] records which one did (XOR wins when a byte
//! matches both). Callers send the variant the trainer last used, and
//! [`FecChecksum::Xor`] before any frame has been seen.
//!
//! Service `6e40fec1-b5a3-f393-e0a9-e50e24dcca9e`.
//! Notify (trainer to app): `6e40fec2-…`. Write (app to trainer): `6e40fec3-…`.

use crate::error::FtmsError;

/// Sync byte of an ANT serial frame.
const ANT_SYNC: u8 = 0xA4;
/// Payload length of a broadcast or acknowledged data frame (channel + 8 page bytes).
const ANT_DATA_LENGTH: u8 = 0x09;
/// Broadcast data, trainer to app.
const ANT_BROADCAST: u8 = 0x4E;
/// Acknowledged data, app to trainer.
const ANT_ACKNOWLEDGED: u8 = 0x4F;
/// Channel Tacx uses for FE-C over BLE. The trainer ignores it on writes.
const TACX_CHANNEL: u8 = 0x05;

/// Which checksum closed a FE-C frame.
///
/// [`Self::Xor`] is the ANT serial checksum: the XOR of every byte, including
/// sync. [`Self::Sum`] is the Tacx how-to prose and pycycling: the low 8 bits
/// of the sum of every byte except sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FecChecksum {
    /// XOR of every byte, including the sync byte.
    Xor,
    /// Low 8 bits of the sum of every byte except the sync byte.
    Sum,
}

impl FecChecksum {
    /// `xor` or `sum`, for the ride log.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Xor => "xor",
            Self::Sum => "sum",
        }
    }

    fn apply(self, frame_without_checksum: &[u8]) -> u8 {
        match self {
            Self::Xor => checksum_xor(frame_without_checksum),
            Self::Sum => checksum_sum(frame_without_checksum),
        }
    }
}

const PAGE_GENERAL: u8 = 0x10;
const PAGE_TRAINER: u8 = 0x19;
const PAGE_TARGET_POWER: u8 = 0x31;
const PAGE_TRACK: u8 = 0x33;
const PAGE_USER: u8 = 0x37;
const PAGE_NEO_ROAD_FEEL: u8 = 0xFC;

/// Neo road-feel intensity for every surface except [`pp_core::RoadSurface::Smooth`].
///
/// pycycling notes 50% is already strong on a Neo and 100% is untested and may
/// damage the trainer, so this crate never sends 100 or the 255 “default” sentinel.
const NEO_ROAD_FEEL_INTENSITY_DEFAULT: u8 = 40;

const SPEED_INVALID: u16 = 0xFFFF;
const HEART_RATE_INVALID: u8 = 0xFF;
const CADENCE_INVALID: u8 = 0xFF;
const POWER_INVALID: u16 = 0x0FFF;
const USER_WEIGHT_INVALID: u16 = 0xFFFF;
const BIKE_WEIGHT_INVALID: u16 = 0x0FFF;
const WHEEL_OFFSET_DEFAULT: u8 = 0x0F;
const GRADE_MIN_PCT: f32 = -200.0;
const GRADE_MAX_PCT: f32 = 200.0;
/// Bicycle weight field tops out at 50 kg in the FE-C profile.
const BIKE_WEIGHT_MAX_KG: f32 = 50.0;
/// User weight field tops out one count below the invalid sentinel.
const USER_WEIGHT_MAX_RAW: u16 = 0xFFFE;

/// One FE-C page carried in an ANT frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FecData {
    /// Page 16, general fitness equipment data.
    General(GeneralFeData),
    /// Page 25, specific trainer data.
    Trainer(SpecificTrainerData),
    /// Page 51, track resistance. This is the grade command.
    TrackResistance(TrackResistance),
    /// Page 55, user configuration.
    UserConfiguration(UserConfiguration),
}

/// A parsed 13-byte FE-C frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FecMessage {
    /// ANT message id. `0x4E` is broadcast data, `0x4F` is acknowledged data.
    pub message_id: u8,
    /// ANT channel. Tacx sends `0x05`. The value is not used for control.
    pub channel: u8,
    /// The checksum variant that matched. XOR wins when a byte matches both.
    pub checksum: FecChecksum,
    /// The data page inside the frame.
    pub data: FecData,
}

impl FecMessage {
    /// Parse one trainer notification.
    ///
    /// A short buffer, a bad sync, a length other than 9, a checksum that
    /// matches neither the Tacx sum nor the ANT XOR, or a page this crate does
    /// not know, is an error. The function does not panic.
    pub fn parse(bytes: &[u8]) -> Result<Self, FtmsError> {
        if bytes.len() < 13 {
            return Err(FtmsError::Truncated {
                what: "fec frame",
                have: bytes.len(),
                need: 13,
            });
        }
        let frame = &bytes[..13];
        if frame[0] != ANT_SYNC {
            return Err(FtmsError::Malformed {
                what: "fec frame",
                detail: "bad sync",
            });
        }
        if frame[1] != ANT_DATA_LENGTH {
            return Err(FtmsError::Malformed {
                what: "fec frame",
                detail: "bad length",
            });
        }
        let message_id = frame[2];
        if message_id != ANT_BROADCAST && message_id != ANT_ACKNOWLEDGED {
            return Err(FtmsError::Malformed {
                what: "fec frame",
                detail: "bad message id",
            });
        }
        let Some(checksum) = classify_checksum(&frame[..12], frame[12]) else {
            return Err(FtmsError::Malformed {
                what: "fec frame",
                detail: "bad checksum",
            });
        };
        let channel = frame[3];
        let data = decode_page(&frame[4..12])?;
        Ok(Self {
            message_id,
            channel,
            checksum,
            data,
        })
    }

    /// Data page number. `16`, `25`, `51`, or `55`.
    #[must_use]
    pub const fn page_number(&self) -> u8 {
        match self.data {
            FecData::General(_) => PAGE_GENERAL,
            FecData::Trainer(_) => PAGE_TRAINER,
            FecData::TrackResistance(_) => PAGE_TRACK,
            FecData::UserConfiguration(_) => PAGE_USER,
        }
    }
}

/// Page 16 (0x10), general fitness equipment data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeneralFeData {
    /// Equipment type bit field. `25` is a trainer.
    pub equipment_type: u8,
    /// Elapsed time, in seconds. The wire unit is 0.25 s and the counter rolls over.
    pub elapsed_s: f32,
    /// Distance traveled, in meters. The counter rolls over at 256.
    pub distance_m: u8,
    /// Speed, in meters per second. `None` when the wire value is `0xFFFF`.
    pub speed_mps: Option<f32>,
    /// Heart rate, in beats per minute. `None` when the wire value is `0xFF`.
    pub heart_rate_bpm: Option<u8>,
    /// Capabilities nibble (byte 7, bits 0–3).
    pub capabilities: u8,
    /// FE state nibble (byte 7, bits 4–7), including the lap-toggle bit.
    pub fe_state: u8,
}

impl GeneralFeData {
    /// Speed in kilometers per hour, when the trainer reported one.
    #[must_use]
    pub fn speed_kph(&self) -> Option<f32> {
        self.speed_mps.map(|mps| mps * 3.6)
    }
}

/// Page 25 (0x19), specific trainer data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecificTrainerData {
    /// Update event counter. It rolls over at 256.
    pub event_count: u8,
    /// Cadence, in revolutions per minute. `None` when the wire value is `0xFF`.
    pub cadence_rpm: Option<u8>,
    /// Accumulated power, in watts. The counter rolls over at 65536.
    pub accumulated_power_w: u16,
    /// Instantaneous power, in watts. `None` when the 12-bit field is `0xFFF`.
    pub instantaneous_power_w: Option<u16>,
    /// Trainer status nibble (byte 6, bits 4–7).
    pub trainer_status: u8,
    /// Flags and FE state (byte 7). Bits 0–3 are flags, bits 4–7 are FE state.
    pub flags_and_state: u8,
}

/// Page 49 (0x31), target power. The ERG command.
///
/// The ANT+ FE-C profile puts the page number in byte 0, reserved `0xFF` in
/// bytes 1–5, and the target in bytes 6–7. The target is little-endian, in
/// units of 0.25 W, from 0 W to 4000 W. `0xFFFF` is the invalid value and is
/// never sent. This type only encodes. It is not part of [`FecData`], so a
/// frame the trainer sends back still has to be a page this crate already
/// parses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetPower {
    /// Target power, in watts.
    pub watts: f32,
}

/// Page 252 (0xFC), Tacx Neo road feel. Encode-only; not part of [`FecData`].
///
/// Layout follows pycycling `set_neo_modes` with isokinetic mode off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeoRoadFeel {
    /// Surface pattern, 0..=9.
    pub pattern: u8,
    /// Shake intensity, 0..=100.
    pub intensity: u8,
}

impl NeoRoadFeel {
    /// Map a game surface onto the Neo pattern and intensity.
    #[must_use]
    pub const fn from_surface(surface: pp_core::RoadSurface) -> Self {
        let pattern = match surface {
            pp_core::RoadSurface::Smooth => 0,
            pp_core::RoadSurface::ConcretePlates => 1,
            pp_core::RoadSurface::CattleGrid => 2,
            pp_core::RoadSurface::CobblestonesHard => 3,
            pp_core::RoadSurface::CobblestonesSoft => 4,
            pp_core::RoadSurface::BrickRoad => 5,
            pp_core::RoadSurface::OffRoad => 6,
            pp_core::RoadSurface::Gravel => 7,
            pp_core::RoadSurface::Ice => 8,
            pp_core::RoadSurface::WoodenBoards => 9,
        };
        let intensity = if matches!(surface, pp_core::RoadSurface::Smooth) {
            0
        } else {
            NEO_ROAD_FEEL_INTENSITY_DEFAULT
        };
        Self { pattern, intensity }
    }

    /// Clamp pattern to 0..=9 and intensity to 0..=100, then encode.
    #[must_use]
    pub fn encode(self, checksum: FecChecksum) -> Vec<u8> {
        let pattern = self.pattern.min(9);
        let intensity = self.intensity.min(100);
        command_frame(
            PAGE_NEO_ROAD_FEEL,
            [0x00, 0x00, 0x00, 0x00, pattern, intensity, 0x00],
            checksum,
        )
    }
}

impl TargetPower {
    /// Highest target the page can express, in watts.
    pub const MAX_W: f32 = 4000.0;

    /// Encode an acknowledged command on Tacx channel 5.
    ///
    /// Watts are rounded to the nearest 0.25 W and saturated into 0 W…4000 W.
    /// A non-finite target is [`FtmsError::NonFinite`]. `checksum` selects the
    /// trailer byte.
    pub fn encode(&self, checksum: FecChecksum) -> Result<Vec<u8>, FtmsError> {
        if !self.watts.is_finite() {
            return Err(FtmsError::NonFinite);
        }
        let raw = encode_target_power(self.watts);
        Ok(command_frame(
            PAGE_TARGET_POWER,
            [
                0xFF,
                0xFF,
                0xFF,
                0xFF,
                0xFF,
                raw.to_le_bytes()[0],
                raw.to_le_bytes()[1],
            ],
            checksum,
        ))
    }
}

/// Page 51 (0x33), track resistance. The grade command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackResistance {
    /// Grade, in percent. Positive is uphill. The wire range is −200% to +200%.
    pub grade_pct: f32,
    /// Coefficient of rolling resistance.
    ///
    /// `None` sends `0xFF`, which tells the trainer to use its own default.
    /// The wire unit is `5e-5`.
    pub rolling_resistance: Option<f32>,
}

impl TrackResistance {
    /// Encode an acknowledged command on Tacx channel 5.
    ///
    /// Grade is saturated into −200%…+200% so the reserved invalid value
    /// `0xFFFF` is never sent. A non-finite grade or rolling resistance is
    /// [`FtmsError::NonFinite`]. `checksum` selects the trailer byte.
    pub fn encode(&self, checksum: FecChecksum) -> Result<Vec<u8>, FtmsError> {
        if !self.grade_pct.is_finite() {
            return Err(FtmsError::NonFinite);
        }
        let grade = self.grade_pct.clamp(GRADE_MIN_PCT, GRADE_MAX_PCT);
        let scaled = ((grade - GRADE_MIN_PCT) * 100.0).round();
        let raw = saturate_u16_unit(scaled, 0xFFFE);
        let crr = encode_crr(self.rolling_resistance)?;
        Ok(command_frame(
            PAGE_TRACK,
            [
                0xFF,
                0xFF,
                0xFF,
                0xFF,
                raw.to_le_bytes()[0],
                raw.to_le_bytes()[1],
                crr,
            ],
            checksum,
        ))
    }
}

/// Page 55 (0x37), user configuration.
///
/// Absent fields are sent as the profile's invalid or default value: user
/// weight `0xFFFF`, bicycle weight `0xFFF`, wheel-diameter offset `0xF`,
/// wheel diameter `0`, gear ratio `0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UserConfiguration {
    /// Rider mass, in kilograms. `None` sends the invalid weight.
    pub user_weight_kg: Option<f32>,
    /// Bicycle mass, in kilograms, not including the rider. `None` sends invalid.
    pub bike_weight_kg: Option<f32>,
    /// Extra millimeters below the centimeter wheel diameter. `None` sends `0xF`.
    pub wheel_diameter_offset_mm: Option<u8>,
    /// Wheel diameter, in meters. `None` sends `0`.
    pub wheel_diameter_m: Option<f32>,
    /// Gear ratio. The wire unit is 0.03. `None` or `0` sends invalid (`0`).
    pub gear_ratio: Option<f32>,
}

impl UserConfiguration {
    /// Encode an acknowledged command on Tacx channel 5.
    ///
    /// A present but non-finite mass, diameter, or gear ratio is
    /// [`FtmsError::NonFinite`]. `checksum` selects the trailer byte.
    pub fn encode(&self, checksum: FecChecksum) -> Result<Vec<u8>, FtmsError> {
        let user = encode_user_weight(self.user_weight_kg)?;
        let bike = encode_bike_weight(self.bike_weight_kg)?;
        let offset = match self.wheel_diameter_offset_mm {
            Some(offset) => offset & 0x0F,
            None => WHEEL_OFFSET_DEFAULT,
        };
        let diameter = encode_wheel_diameter(self.wheel_diameter_m)?;
        let gear = encode_gear(self.gear_ratio)?;
        let weight_lsn = (bike & 0x0F) as u8;
        Ok(command_frame(
            PAGE_USER,
            [
                user.to_le_bytes()[0],
                user.to_le_bytes()[1],
                0xFF,
                offset | (weight_lsn << 4),
                (bike >> 4) as u8,
                diameter,
                gear,
            ],
            checksum,
        ))
    }
}

fn decode_page(page: &[u8]) -> Result<FecData, FtmsError> {
    match page[0] {
        PAGE_GENERAL => Ok(FecData::General(decode_general(page))),
        PAGE_TRAINER => Ok(FecData::Trainer(decode_trainer(page))),
        PAGE_TRACK => Ok(FecData::TrackResistance(decode_track(page)?)),
        PAGE_USER => Ok(FecData::UserConfiguration(decode_user(page))),
        _ => Err(FtmsError::Malformed {
            what: "fec page",
            detail: "unknown page",
        }),
    }
}

fn decode_general(page: &[u8]) -> GeneralFeData {
    let speed_raw = u16::from_le_bytes([page[4], page[5]]);
    GeneralFeData {
        equipment_type: page[1],
        elapsed_s: f32::from(page[2]) * 0.25,
        distance_m: page[3],
        speed_mps: (speed_raw != SPEED_INVALID).then_some(f32::from(speed_raw) * 0.001),
        heart_rate_bpm: (page[6] != HEART_RATE_INVALID).then_some(page[6]),
        capabilities: page[7] & 0x0F,
        fe_state: page[7] >> 4,
    }
}

fn decode_trainer(page: &[u8]) -> SpecificTrainerData {
    let power_raw = u16::from(page[5]) | (u16::from(page[6] & 0x0F) << 8);
    SpecificTrainerData {
        event_count: page[1],
        cadence_rpm: (page[2] != CADENCE_INVALID).then_some(page[2]),
        accumulated_power_w: u16::from_le_bytes([page[3], page[4]]),
        instantaneous_power_w: (power_raw != POWER_INVALID).then_some(power_raw),
        trainer_status: page[6] >> 4,
        flags_and_state: page[7],
    }
}

fn decode_track(page: &[u8]) -> Result<TrackResistance, FtmsError> {
    let raw = u16::from_le_bytes([page[5], page[6]]);
    if raw == SPEED_INVALID {
        return Err(FtmsError::Malformed {
            what: "fec grade",
            detail: "invalid grade",
        });
    }
    Ok(TrackResistance {
        grade_pct: f32::from(raw) * 0.01 + GRADE_MIN_PCT,
        rolling_resistance: (page[7] != 0xFF).then_some(f32::from(page[7]) * 5.0e-5),
    })
}

fn decode_user(page: &[u8]) -> UserConfiguration {
    let user_raw = u16::from_le_bytes([page[1], page[2]]);
    let bike_raw = (u16::from(page[5]) << 4) | u16::from(page[4] >> 4);
    let offset = page[4] & 0x0F;
    UserConfiguration {
        user_weight_kg: (user_raw != USER_WEIGHT_INVALID).then_some(f32::from(user_raw) * 0.01),
        bike_weight_kg: (bike_raw != BIKE_WEIGHT_INVALID).then_some(f32::from(bike_raw) * 0.05),
        wheel_diameter_offset_mm: (offset != WHEEL_OFFSET_DEFAULT).then_some(offset),
        wheel_diameter_m: (page[6] != 0).then_some(f32::from(page[6]) * 0.01),
        gear_ratio: (page[7] != 0).then_some(f32::from(page[7]) * 0.03),
    }
}

fn command_frame(page: u8, payload: [u8; 7], checksum: FecChecksum) -> Vec<u8> {
    let mut frame = Vec::with_capacity(13);
    frame.extend_from_slice(&[
        ANT_SYNC,
        ANT_DATA_LENGTH,
        ANT_ACKNOWLEDGED,
        TACX_CHANNEL,
        page,
    ]);
    frame.extend_from_slice(&payload);
    let trailer = checksum.apply(&frame);
    frame.push(trailer);
    frame
}

/// XOR wins a tie: the trailer byte is then identical either way, and XOR is
/// the checksum on the frame Tacx's how-to shows the trainer sending.
fn classify_checksum(frame_without_checksum: &[u8], trailer: u8) -> Option<FecChecksum> {
    if trailer == checksum_xor(frame_without_checksum) {
        Some(FecChecksum::Xor)
    } else if trailer == checksum_sum(frame_without_checksum) {
        Some(FecChecksum::Sum)
    } else {
        None
    }
}

/// Tacx FE-C over BLE: sum of every byte except sync, low 8 bits.
fn checksum_sum(frame_without_checksum: &[u8]) -> u8 {
    frame_without_checksum
        .iter()
        .skip(1)
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte))
}

/// ANT serial checksum: XOR of every byte, including sync.
fn checksum_xor(frame_without_checksum: &[u8]) -> u8 {
    frame_without_checksum
        .iter()
        .fold(0u8, |acc, byte| acc ^ *byte)
}

fn encode_crr(crr: Option<f32>) -> Result<u8, FtmsError> {
    let Some(crr) = crr else {
        return Ok(0xFF);
    };
    if !crr.is_finite() {
        return Err(FtmsError::NonFinite);
    }
    if crr <= 0.0 {
        return Ok(0);
    }
    let scaled = (crr / 5.0e-5).round();
    if scaled >= 254.0 {
        Ok(254)
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok(scaled as u8)
    }
}

fn encode_user_weight(kg: Option<f32>) -> Result<u16, FtmsError> {
    let Some(kg) = kg else {
        return Ok(USER_WEIGHT_INVALID);
    };
    if !kg.is_finite() {
        return Err(FtmsError::NonFinite);
    }
    Ok(saturate_u16_unit((kg * 100.0).round(), USER_WEIGHT_MAX_RAW))
}

fn encode_bike_weight(kg: Option<f32>) -> Result<u16, FtmsError> {
    let Some(kg) = kg else {
        return Ok(BIKE_WEIGHT_INVALID);
    };
    if !kg.is_finite() {
        return Err(FtmsError::NonFinite);
    }
    let capped = kg.clamp(0.0, BIKE_WEIGHT_MAX_KG);
    Ok(saturate_u16_unit((capped / 0.05).round(), 1000))
}

fn encode_wheel_diameter(meters: Option<f32>) -> Result<u8, FtmsError> {
    let Some(meters) = meters else {
        return Ok(0);
    };
    if !meters.is_finite() {
        return Err(FtmsError::NonFinite);
    }
    if meters <= 0.0 {
        return Ok(0);
    }
    let scaled = (meters * 100.0).round();
    if scaled >= 254.0 {
        Ok(254)
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok(scaled as u8)
    }
}

fn encode_gear(ratio: Option<f32>) -> Result<u8, FtmsError> {
    let Some(ratio) = ratio else {
        return Ok(0);
    };
    if !ratio.is_finite() {
        return Err(FtmsError::NonFinite);
    }
    if ratio <= 0.0 {
        return Ok(0);
    }
    let scaled = (ratio / 0.03).round();
    if scaled >= 255.0 {
        Ok(255)
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok(scaled as u8)
    }
}

fn encode_target_power(watts: f32) -> u16 {
    let capped = watts.clamp(0.0, TargetPower::MAX_W);
    saturate_u16_unit((capped * 4.0).round(), 16_000)
}

fn saturate_u16_unit(scaled: f32, max: u16) -> u16 {
    if scaled <= 0.0 {
        0
    } else if scaled >= f32::from(max) {
        max
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            scaled as u16
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_grade_with_default_rolling_resistance_matches_the_hand_vector() {
        // Grade 0% → raw (0 + 200) × 100 = 20000 = 0x4E20.
        // Rolling resistance omitted → 0xFF (trainer default).
        // Checksum is the Tacx sum of every byte after 0xA4, computed by hand:
        // 0x09+0x4F+0x05+0x33+4×0xFF+0x20+0x4E+0xFF = 0x5F9, low byte 0xF9.
        // Page layout: ANT+ FE-C page 51. Checksum rule: Tacx "FE-C over BLE"
        // (2015-02-17) and pycycling `tacx_trainer_control.py` `_send_fec_cmd`.
        let bytes = TrackResistance {
            grade_pct: 0.0,
            rolling_resistance: None,
        }
        .encode(FecChecksum::Sum)
        .unwrap();
        assert_eq!(
            bytes,
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0x20, 0x4E, 0xFF, 0xF9,
            ]
        );
    }

    #[test]
    fn grade_and_user_configuration_round_trip() {
        let grades = [
            TrackResistance {
                grade_pct: 0.0,
                rolling_resistance: Some(0.004),
            },
            TrackResistance {
                grade_pct: 1.0,
                rolling_resistance: Some(0.004),
            },
            TrackResistance {
                grade_pct: -10.0,
                rolling_resistance: Some(0.004),
            },
            TrackResistance {
                grade_pct: 200.0,
                rolling_resistance: Some(0.0),
            },
        ];
        for page in grades {
            let bytes = page.encode(FecChecksum::Sum).unwrap();
            let parsed = FecMessage::parse(&bytes).unwrap();
            let FecData::TrackResistance(decoded) = parsed.data else {
                panic!("page 51 decoded as {:?}", parsed.data);
            };
            assert!((decoded.grade_pct - page.grade_pct).abs() < 1.0e-3);
            match (page.rolling_resistance, decoded.rolling_resistance) {
                (Some(left), Some(right)) => assert!((left - right).abs() < 1.0e-6),
                (None, None) => {}
                other => panic!("rolling resistance {other:?}"),
            }
            assert_eq!(parsed.message_id, ANT_ACKNOWLEDGED);
            assert_eq!(parsed.channel, TACX_CHANNEL);
        }

        let user = UserConfiguration {
            user_weight_kg: Some(75.0),
            bike_weight_kg: Some(9.0),
            wheel_diameter_offset_mm: None,
            wheel_diameter_m: Some(0.70),
            gear_ratio: None,
        };
        let bytes = user.encode(FecChecksum::Sum).unwrap();
        let parsed = FecMessage::parse(&bytes).unwrap();
        let FecData::UserConfiguration(decoded) = parsed.data else {
            panic!("page 55 decoded as {:?}", parsed.data);
        };
        assert!((decoded.user_weight_kg.unwrap() - 75.0).abs() < 1.0e-3);
        assert!((decoded.bike_weight_kg.unwrap() - 9.0).abs() < 1.0e-3);
        assert!(decoded.wheel_diameter_offset_mm.is_none());
        assert!((decoded.wheel_diameter_m.unwrap() - 0.70).abs() < 1.0e-3);
        assert!(decoded.gear_ratio.is_none());
    }

    #[test]
    fn user_configuration_without_mass_uses_the_invalid_sentinels() {
        // No rider mass and no bike mass. User weight 0xFFFF, bike weight
        // 0xFFF packed with the default wheel offset 0xF, so bytes 4 and 5
        // are both 0xFF. Wheel diameter and gear ratio stay 0 (invalid).
        let bytes = UserConfiguration {
            user_weight_kg: None,
            bike_weight_kg: None,
            wheel_diameter_offset_mm: None,
            wheel_diameter_m: None,
            gear_ratio: None,
        }
        .encode(FecChecksum::Sum)
        .unwrap();
        assert_eq!(
            bytes,
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0x37, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x8F,
            ]
        );
    }

    #[test]
    fn seventy_five_kilogram_rider_matches_the_hand_vector() {
        // 75.00 kg → 7500 = 0x1D4C. 9.00 kg bike → 180 = 0x00B4 in 0.05 kg units.
        // Byte 4 low nibble is the default offset 0xF, high nibble is 0x4.
        // Byte 5 is 0x0B. Checksum 0x56 is the Tacx sum, computed by hand.
        let bytes = UserConfiguration {
            user_weight_kg: Some(75.0),
            bike_weight_kg: Some(9.0),
            wheel_diameter_offset_mm: None,
            wheel_diameter_m: None,
            gear_ratio: None,
        }
        .encode(FecChecksum::Sum)
        .unwrap();
        assert_eq!(
            bytes,
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0x37, 0x4C, 0x1D, 0xFF, 0x4F, 0x0B, 0x00, 0x00, 0x56,
            ]
        );
    }

    #[test]
    fn tacx_howto_general_page_uses_the_ant_xor_checksum() {
        // Tacx "FE-C over BLE" shows a page 16 from the trainer. The table's
        // last data byte is 0x24 and the checksum is 0x30. That is the XOR of
        // every preceding byte, including sync (GoldenCheetah `ANTMessage`
        // does the same). The how-to's prose says "sum excluding sync", which
        // would be 0xAC for these bytes, so the worked example wins.
        // Equipment type 0x19 is a trainer, elapsed 0x04 is 1.0 s, speed 0,
        // heart rate 0xFF is invalid, capabilities nibble 0x4, FE state 0x2.
        let bytes = [
            0xA4, 0x09, 0x4E, 0x05, 0x10, 0x19, 0x04, 0x00, 0x00, 0x00, 0xFF, 0x24, 0x30,
        ];
        let parsed = FecMessage::parse(&bytes).unwrap();
        let FecData::General(data) = parsed.data else {
            panic!("page 16 decoded as {:?}", parsed.data);
        };
        assert_eq!(parsed.message_id, ANT_BROADCAST);
        assert_eq!(parsed.checksum, FecChecksum::Xor);
        assert_eq!(parsed.page_number(), 16);
        assert_eq!(data.equipment_type, 25);
        assert!((data.elapsed_s - 1.0).abs() < 1.0e-6);
        assert_eq!(data.distance_m, 0);
        assert_eq!(data.speed_mps, Some(0.0));
        assert_eq!(data.speed_kph(), Some(0.0));
        assert_eq!(data.heart_rate_bpm, None);
        assert_eq!(data.capabilities, 0x4);
        assert_eq!(data.fe_state, 0x2);
    }

    #[test]
    fn specific_trainer_page_decodes_power_and_cadence() {
        // Event 7, cadence 90, accumulated 1000 W, instantaneous 250 W.
        // Checksum 0xEB is the Tacx sum of the bytes after sync.
        let bytes = [
            0xA4, 0x09, 0x4E, 0x05, 0x19, 0x07, 0x5A, 0xE8, 0x03, 0xFA, 0x00, 0x30, 0xEB,
        ];
        let parsed = FecMessage::parse(&bytes).unwrap();
        let FecData::Trainer(data) = parsed.data else {
            panic!("page 25 decoded as {:?}", parsed.data);
        };
        assert_eq!(data.event_count, 7);
        assert_eq!(data.cadence_rpm, Some(90));
        assert_eq!(data.accumulated_power_w, 1000);
        assert_eq!(parsed.checksum, FecChecksum::Sum);
        assert_eq!(parsed.page_number(), 25);
        assert_eq!(data.instantaneous_power_w, Some(250));
        assert_eq!(data.trainer_status, 0);
        assert_eq!(data.flags_and_state, 0x30);
    }

    #[test]
    fn invalid_trainer_fields_are_absent() {
        // Cadence 0xFF, power 0xFFF (byte 5 = 0xFF, low nibble of byte 6 = 0xF).
        let bytes = [
            0xA4, 0x09, 0x4E, 0x05, 0x19, 0x02, 0xFF, 0x10, 0x00, 0xFF, 0x0F, 0x40, 0xD4,
        ];
        let parsed = FecMessage::parse(&bytes).unwrap();
        let FecData::Trainer(data) = parsed.data else {
            panic!("page 25 decoded as {:?}", parsed.data);
        };
        assert_eq!(data.cadence_rpm, None);
        assert_eq!(data.instantaneous_power_w, None);
        assert_eq!(data.accumulated_power_w, 0x0010);
    }

    #[test]
    fn the_same_page_is_accepted_with_either_checksum() {
        let mut xor = [
            0xA4, 0x09, 0x4E, 0x05, 0x19, 0x07, 0x5A, 0xE8, 0x03, 0xFA, 0x00, 0x30, 0x00,
        ];
        xor[12] = checksum_xor(&xor[..12]);
        let parsed = FecMessage::parse(&xor).unwrap();
        let FecData::Trainer(data) = parsed.data else {
            panic!("xor frame decoded as {:?}", parsed.data);
        };
        assert_eq!(parsed.checksum, FecChecksum::Xor);
        assert_eq!(data.instantaneous_power_w, Some(250));

        let summed = TrackResistance {
            grade_pct: 0.0,
            rolling_resistance: None,
        }
        .encode(FecChecksum::Sum)
        .unwrap();
        assert_eq!(
            FecMessage::parse(&summed).unwrap().checksum,
            FecChecksum::Sum
        );
    }

    #[test]
    fn both_checksums_are_byte_exact() {
        // Grade 0%, rolling resistance omitted. The body is the same; only
        // the trailer changes. Sum 0xF9 is the Tacx/pycycling rule. XOR 0x45
        // is the ANT serial rule used by the Tacx how-to's trainer frame.
        let page = TrackResistance {
            grade_pct: 0.0,
            rolling_resistance: None,
        };
        assert_eq!(
            page.encode(FecChecksum::Xor).unwrap(),
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0x20, 0x4E, 0xFF, 0x45,
            ]
        );
        assert_eq!(
            page.encode(FecChecksum::Sum).unwrap(),
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0x20, 0x4E, 0xFF, 0xF9,
            ]
        );
        // Tacx "FE-C over BLE" trainer frame. Checksum 0x30 is the XOR of
        // every preceding byte, including sync. The sum of the bytes after
        // sync is 0xAC, so this frame is XOR and not the sum.
        let tacx = [
            0xA4, 0x09, 0x4E, 0x05, 0x10, 0x19, 0x04, 0x00, 0x00, 0x00, 0xFF, 0x24, 0x30,
        ];
        assert_eq!(checksum_xor(&tacx[..12]), 0x30);
        assert_eq!(FecMessage::parse(&tacx).unwrap().checksum, FecChecksum::Xor);
    }

    #[test]
    fn grade_saturates_instead_of_sending_the_invalid_sentinel() {
        let high = TrackResistance {
            grade_pct: 10_000.0,
            rolling_resistance: Some(1.0),
        }
        .encode(FecChecksum::Sum)
        .unwrap();
        assert_eq!(high[9], 0x40);
        assert_eq!(high[10], 0x9C);
        assert_eq!(high[11], 254);
        let low = TrackResistance {
            grade_pct: -10_000.0,
            rolling_resistance: Some(-1.0),
        }
        .encode(FecChecksum::Sum)
        .unwrap();
        assert_eq!(low[9], 0x00);
        assert_eq!(low[10], 0x00);
        assert_eq!(low[11], 0);
    }

    #[test]
    fn page_49_target_power_matches_the_hand_vector() {
        // 200 W → 800 counts of 0.25 W = 0x0320, little-endian in bytes 6–7.
        // Reserved bytes 1–5 are 0xFF. Page layout: ANT+ FE-C page 49.
        // Tacx sum of the bytes after sync, by hand:
        // 0x09+0x4F+0x05+0x31+5×0xFF+0x20+0x03 = 0x5AC, low byte 0xAC.
        // XOR of every byte including sync is 0x0A.
        let page = TargetPower { watts: 200.0 };
        assert_eq!(
            page.encode(FecChecksum::Sum).unwrap(),
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0x31, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x20, 0x03, 0xAC,
            ]
        );
        assert_eq!(
            page.encode(FecChecksum::Xor).unwrap(),
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0x31, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x20, 0x03, 0x0A,
            ]
        );
    }

    #[test]
    fn page_49_rounds_to_a_quarter_watt_and_clamps() {
        // 200.25 W is 801 counts (0x0321). 200.125 W is 800.5 counts, which
        // rounds to 801.
        let quarter = TargetPower { watts: 200.25 }
            .encode(FecChecksum::Sum)
            .unwrap();
        assert_eq!(quarter[10], 0x21);
        assert_eq!(quarter[11], 0x03);
        let half_up = TargetPower { watts: 200.125 }
            .encode(FecChecksum::Sum)
            .unwrap();
        assert_eq!(half_up[10], 0x21);
        assert_eq!(half_up[11], 0x03);
        let high = TargetPower { watts: 10_000.0 }
            .encode(FecChecksum::Sum)
            .unwrap();
        // 4000 W → 16000 = 0x3E80.
        assert_eq!(high[10], 0x80);
        assert_eq!(high[11], 0x3E);
        let low = TargetPower { watts: -50.0 }
            .encode(FecChecksum::Sum)
            .unwrap();
        assert_eq!(low[10], 0x00);
        assert_eq!(low[11], 0x00);
        assert!(
            TargetPower { watts: f32::NAN }
                .encode(FecChecksum::Sum)
                .is_err()
        );
        assert!(
            TargetPower {
                watts: f32::INFINITY
            }
            .encode(FecChecksum::Xor)
            .is_err()
        );
    }

    #[test]
    fn page_fc_gravel_matches_pycycling_vector() {
        let page = NeoRoadFeel::from_surface(pp_core::RoadSurface::Gravel);
        assert_eq!(
            page.encode(FecChecksum::Xor),
            vec![
                0xA4, 0x09, 0x4F, 0x05, 0xFC, 0x00, 0x00, 0x00, 0x00, 0x07, 0x28, 0x00, 0x34,
            ]
        );
        let mut sum_page = page.encode(FecChecksum::Sum);
        assert_eq!(sum_page[12], 0x88);
        sum_page[12] = 0x34;
        assert_eq!(sum_page, page.encode(FecChecksum::Xor));
    }

    #[test]
    fn page_fc_from_surface_sets_pattern_and_intensity() {
        let smooth = NeoRoadFeel::from_surface(pp_core::RoadSurface::Smooth);
        assert_eq!(smooth.pattern, 0);
        assert_eq!(smooth.intensity, 0);
        let boards = NeoRoadFeel::from_surface(pp_core::RoadSurface::WoodenBoards);
        assert_eq!(boards.pattern, 9);
        assert_eq!(boards.intensity, 40);
    }

    #[test]
    fn page_fc_clamps_pattern_and_intensity() {
        let page = NeoRoadFeel {
            pattern: 99,
            intensity: 255,
        };
        let encoded = page.encode(FecChecksum::Xor);
        assert_eq!(encoded[9], 9);
        assert_eq!(encoded[10], 100);
        assert_ne!(encoded[10], 255);
    }

    #[test]
    fn page_fc_maps_every_road_surface_pattern() {
        let surfaces = [
            (pp_core::RoadSurface::Smooth, 0),
            (pp_core::RoadSurface::ConcretePlates, 1),
            (pp_core::RoadSurface::CattleGrid, 2),
            (pp_core::RoadSurface::CobblestonesHard, 3),
            (pp_core::RoadSurface::CobblestonesSoft, 4),
            (pp_core::RoadSurface::BrickRoad, 5),
            (pp_core::RoadSurface::OffRoad, 6),
            (pp_core::RoadSurface::Gravel, 7),
            (pp_core::RoadSurface::Ice, 8),
            (pp_core::RoadSurface::WoodenBoards, 9),
        ];
        for (surface, pattern) in surfaces {
            assert_eq!(NeoRoadFeel::from_surface(surface).pattern, pattern);
        }
    }

    #[test]
    fn a_bad_frame_is_rejected() {
        let ok = TrackResistance {
            grade_pct: 0.0,
            rolling_resistance: None,
        }
        .encode(FecChecksum::Sum)
        .unwrap();
        let mut bad_sync = ok.clone();
        bad_sync[0] = 0x00;
        assert!(FecMessage::parse(&bad_sync).is_err());
        let mut bad_length = ok.clone();
        bad_length[1] = 0x08;
        assert!(FecMessage::parse(&bad_length).is_err());
        let mut bad_sum = ok.clone();
        bad_sum[12] ^= 0xFF;
        assert!(FecMessage::parse(&bad_sum).is_err());
        let mut unknown = ok.clone();
        unknown[4] = 0x01;
        unknown[12] = checksum_sum(&unknown[..12]);
        assert!(FecMessage::parse(&unknown).is_err());
        assert!(FecMessage::parse(&ok[..12]).is_err());
        assert!(
            TrackResistance {
                grade_pct: f32::NAN,
                rolling_resistance: None,
            }
            .encode(FecChecksum::Sum)
            .is_err()
        );
    }
}
