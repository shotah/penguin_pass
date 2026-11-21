//! Optional raw log of the trainer link.
//!
//! Off unless a directory was given. A disabled log does nothing on an event.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Column order of `ble.csv`. The first column joins this file to `ride.csv`.
pub(crate) const BLE_TRACE_HEADER: &str = "\
unix_ms,mono_ms,dir,channel,bytes_hex,fec_page,power_w,cadence_rpm,speed_kph,note";

/// One trainer-link event, already decided. Empty strings are empty cells.
pub(crate) struct TraceRow<'a> {
    pub mono_ms: u64,
    pub dir: &'a str,
    pub channel: &'a str,
    pub bytes_hex: &'a str,
    pub fec_page: Option<u8>,
    pub power_w: Option<i32>,
    pub cadence_rpm: Option<f32>,
    pub speed_kph: Option<f32>,
    pub note: &'a str,
}

/// Buffered `ble.csv`, or nothing.
pub(crate) struct BleTrace {
    writer: Option<BufWriter<File>>,
    last_flush: Instant,
}

impl BleTrace {
    /// No file, and [`Self::record`] returns immediately.
    pub(crate) fn off() -> Self {
        Self {
            writer: None,
            last_flush: Instant::now(),
        }
    }

    /// Directory from `PENGUIN_PASS_TRACE_DIR`.
    ///
    /// This is the only read of that variable. Unset or blank is [`Self::off`].
    pub(crate) fn from_env() -> Self {
        match std::env::var("PENGUIN_PASS_TRACE_DIR") {
            Ok(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    Self::off()
                } else {
                    Self::open(Path::new(trimmed))
                }
            }
            Err(_) => Self::off(),
        }
    }

    /// Create `dir/ble.csv` and write the header.
    ///
    /// A failure prints one warning and returns [`Self::off`].
    pub(crate) fn open(dir: &Path) -> Self {
        match open_file(dir) {
            Ok((writer, path)) => {
                eprintln!("pp-trainer: trace {}", path.display());
                Self {
                    writer: Some(writer),
                    last_flush: Instant::now(),
                }
            }
            Err(err) => {
                eprintln!("pp-trainer: trace not opened {}: {err}", dir.display());
                Self::off()
            }
        }
    }

    /// `true` when a file is open.
    pub(crate) fn is_recording(&self) -> bool {
        self.writer.is_some()
    }

    /// Append one row. Does nothing when recording is off.
    pub(crate) fn record(&mut self, row: TraceRow<'_>) {
        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        let line = format!(
            "{unix},{mono},{dir},{channel},{bytes},{page},{power},{cadence},{speed},{note}",
            unix = unix_ms(),
            mono = row.mono_ms,
            dir = row.dir,
            channel = row.channel,
            bytes = row.bytes_hex,
            page = row
                .fec_page
                .map(|page| page.to_string())
                .unwrap_or_default(),
            power = row
                .power_w
                .map(|watts| watts.to_string())
                .unwrap_or_default(),
            cadence = fmt_f32(row.cadence_rpm),
            speed = fmt_f32(row.speed_kph),
            note = csv_cell(row.note),
        );
        if writeln!(writer, "{line}").is_err() {
            return;
        }
        if self.last_flush.elapsed().as_secs() >= 1 {
            let _ = writer.flush();
            self.last_flush = Instant::now();
        }
    }

    /// Push buffered rows to disk.
    pub(crate) fn flush(&mut self) {
        if let Some(writer) = self.writer.as_mut() {
            let _ = writer.flush();
            self.last_flush = Instant::now();
        }
    }
}

impl Drop for BleTrace {
    fn drop(&mut self) {
        self.flush();
    }
}

fn open_file(dir: &Path) -> Result<(BufWriter<File>, PathBuf), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let path = dir.join("ble.csv");
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "{BLE_TRACE_HEADER}").map_err(|err| err.to_string())?;
    writer.flush().map_err(|err| err.to_string())?;
    let absolute = std::path::absolute(&path).unwrap_or(path);
    Ok((writer, absolute))
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn fmt_f32(value: Option<f32>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    if !value.is_finite() {
        return String::new();
    }
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
