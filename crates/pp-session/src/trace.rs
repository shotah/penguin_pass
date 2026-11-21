//! Optional per-tick ride log.
//!
//! Off unless a directory was given. A disabled log does nothing on a tick.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Column order of `ride.csv`. The first column joins this file to `ble.csv`.
pub(crate) const RIDE_TRACE_HEADER: &str = "\
unix_ms,clock_s,dt_s,phase,source,link,power_w,cadence_rpm,trainer_speed_kph,\
heart_rate_bpm,sample_changed,sample_age_ms,game_speed_kph,game_accel_mps2,\
grade_pct,grade_sent_pct,distance_m,power_3s_w";

/// Fields of one trainer or sim sample, for the changed-notification flag.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct SampleStamp {
    /// Watts the tick fed onward. `None` when the source did not report power.
    pub power_w: Option<f32>,
    /// Cadence, in revolutions per minute.
    pub cadence_rpm: Option<f32>,
    /// Trainer wheel speed, in kilometers per hour.
    pub trainer_speed_kph: Option<f32>,
    /// Heart rate, in beats per minute.
    pub heart_rate_bpm: Option<u8>,
}

/// One tick, already measured. The log formats it only when recording.
pub(crate) struct TraceTick<'a> {
    /// Session clock after this tick, in seconds.
    pub clock_s: f64,
    /// Accepted frame duration, in seconds.
    pub dt_s: f64,
    /// `ready` or `riding`.
    pub phase: &'a str,
    /// `sim`, `trainer`, or `none`.
    pub source: &'a str,
    /// Connection state as a short word.
    pub link: &'a str,
    /// Sample fields compared with the previous tick.
    pub sample: SampleStamp,
    /// Game speed before this tick, in meters per second.
    pub previous_speed_mps: f32,
    /// Game speed after this tick, in meters per second.
    pub game_speed_mps: f32,
    /// Course grade under the rider, in percent.
    pub grade_pct: f32,
    /// Grade last commanded to the trainer, in percent.
    pub grade_sent_pct: Option<f32>,
    /// Distance ridden, in meters.
    pub distance_m: f64,
    /// Three-second power, in watts.
    pub power_3s_w: Option<f32>,
}

/// Buffered `ride.csv`, or nothing.
pub(crate) struct RideTrace {
    writer: Option<BufWriter<File>>,
    last_flush: Instant,
    previous: Option<SampleStamp>,
}

impl RideTrace {
    /// No file.
    pub(crate) fn off() -> Self {
        Self {
            writer: None,
            last_flush: Instant::now(),
            previous: None,
        }
    }

    /// Directory from `PENGUIN_PASS_TRACE_DIR`, read here and nowhere else.
    ///
    /// Unset or blank is [`Self::off`].
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

    /// Create `dir/ride.csv` and write the header.
    ///
    /// A failure prints one warning and returns [`Self::off`].
    pub(crate) fn open(dir: &Path) -> Self {
        match open_file(dir) {
            Ok((writer, path)) => {
                eprintln!("pp-session: trace {}", path.display());
                Self {
                    writer: Some(writer),
                    last_flush: Instant::now(),
                    previous: None,
                }
            }
            Err(err) => {
                eprintln!("pp-session: trace not opened {}: {err}", dir.display());
                Self::off()
            }
        }
    }

    /// `true` when a file is open.
    pub(crate) fn is_recording(&self) -> bool {
        self.writer.is_some()
    }

    /// Append one row. Does nothing when recording is off.
    pub(crate) fn record(&mut self, tick: &TraceTick<'_>) {
        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        let changed = self.previous.as_ref() != Some(&tick.sample);
        self.previous = Some(tick.sample);
        let accel = (tick.dt_s > 0.0 && tick.dt_s.is_finite()).then(|| {
            (f64::from(tick.game_speed_mps) - f64::from(tick.previous_speed_mps)) / tick.dt_s
        });
        let line = format!(
            "{unix},{clock},{dt},{phase},{source},{link},{power},{cadence},{trainer_speed},{heart},{changed},,{game_speed},{accel},{grade},{sent},{distance},{power_3s}",
            unix = unix_ms(),
            clock = fmt3(tick.clock_s),
            dt = fmt3(tick.dt_s),
            phase = tick.phase,
            source = tick.source,
            link = tick.link,
            power = fmt1(tick.sample.power_w),
            cadence = fmt1(tick.sample.cadence_rpm),
            trainer_speed = fmt_kph(tick.sample.trainer_speed_kph),
            heart = tick
                .sample
                .heart_rate_bpm
                .map(|bpm| bpm.to_string())
                .unwrap_or_default(),
            changed = u8::from(changed),
            game_speed = fmt3(f64::from(tick.game_speed_mps) * 3.6),
            accel = accel.map(fmt3).unwrap_or_default(),
            grade = fmt3(f64::from(tick.grade_pct)),
            sent = tick
                .grade_sent_pct
                .map(|grade| fmt3(f64::from(grade)))
                .unwrap_or_default(),
            distance = fmt3(tick.distance_m),
            power_3s = fmt1(tick.power_3s_w),
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

impl Drop for RideTrace {
    fn drop(&mut self) {
        self.flush();
    }
}

fn open_file(dir: &Path) -> Result<(BufWriter<File>, PathBuf), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let path = dir.join("ride.csv");
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "{RIDE_TRACE_HEADER}").map_err(|err| err.to_string())?;
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

fn fmt3(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.3}")
    } else {
        String::new()
    }
}

fn fmt1(value: Option<f32>) -> String {
    match value {
        Some(value) if value.is_finite() => format!("{value:.1}"),
        _ => String::new(),
    }
}

fn fmt_kph(value: Option<f32>) -> String {
    match value {
        Some(value) if value.is_finite() => format!("{value:.3}"),
        _ => String::new(),
    }
}
