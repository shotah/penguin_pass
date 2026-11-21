//! Heart-rate strap link, separate from the trainer.

use pp_trainer::{ConnectionState, DiscoveredSensor, HeartRateLink};

/// One row of the heart-rate strap list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensorEntry {
    /// Bluetooth address or the id the strap reported.
    pub id: String,
    /// Advertised name.
    pub name: String,
    /// Received signal strength, in dBm, when the adapter reported it.
    pub rssi_dbm: Option<i16>,
    /// Signal bars, from 0 to 4. See [`crate::signal_bars`].
    pub bars: u8,
    /// `true` when this row matches the remembered strap.
    pub remembered: bool,
}

/// Builds the strap link a [`crate::Session`] keeps beside the trainer.
///
/// The session spawns at most one link. With no factory, nothing is spawned.
pub trait HeartRateFactory: Send {
    /// Start scanning for heart-rate straps.
    ///
    /// # Errors
    ///
    /// The string is a short reason the link could not be started. The ride
    /// continues with no strap.
    fn spawn(&mut self) -> Result<Box<dyn HeartRateLink>, String>;
}

/// Opens a real Bluetooth heart-rate strap.
///
/// The trainer link is left alone. [`pp_trainer::spawn_ble_heart_rate`] runs
/// on its own background thread.
#[derive(Debug, Default, Clone, Copy)]
pub struct BleHeartRateFactory;

impl HeartRateFactory for BleHeartRateFactory {
    fn spawn(&mut self) -> Result<Box<dyn HeartRateLink>, String> {
        pp_trainer::spawn_ble_heart_rate()
    }
}

pub(crate) fn remembered(
    id: &str,
    name: &str,
    saved_id: Option<&str>,
    saved_name: Option<&str>,
) -> bool {
    if let Some(saved) = saved_id.map(str::trim).filter(|saved| !saved.is_empty())
        && (id.eq_ignore_ascii_case(saved)
            || name
                .to_ascii_lowercase()
                .contains(&saved.to_ascii_lowercase()))
    {
        return true;
    }
    if let Some(saved) = saved_name.map(str::trim).filter(|saved| !saved.is_empty()) {
        return name.eq_ignore_ascii_case(saved) || id.eq_ignore_ascii_case(saved);
    }
    false
}

pub(crate) fn entry(
    sensor: &DiscoveredSensor,
    saved_id: Option<&str>,
    saved_name: Option<&str>,
) -> SensorEntry {
    SensorEntry {
        id: sensor.id.clone(),
        name: sensor.name.clone(),
        rssi_dbm: sensor.rssi_dbm,
        bars: crate::signal::signal_bars(sensor.rssi_dbm),
        remembered: remembered(&sensor.id, &sensor.name, saved_id, saved_name),
    }
}

pub(crate) fn strap_bpm_u8(link: &dyn HeartRateLink) -> Option<u8> {
    let bpm = link.bpm()?;
    Some(u8::try_from(bpm).unwrap_or(u8::MAX))
}

pub(crate) fn is_connected(state: &ConnectionState) -> bool {
    matches!(state, ConnectionState::Connected)
}
