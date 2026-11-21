//! FIT activity encoder and the atomic file write.
//!
//! Encoding goes through `rustyfit`. Invalid FIT sentinels are left in place
//! for sensors that were never reported, and `rustyfit` omits those fields.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use embedded_io_adapters::std::FromStd;
use rustyfit::Encoder;
use rustyfit::profile::{mesgdef, typedef};
use rustyfit::proto::{FIT, Message};

use crate::{
    Activity, ActivitySummary, LapSummary, MIN_WRITTEN_DISTANCE_M, MIN_WRITTEN_ELAPSED_S,
    RecordedSecond, RideMeta, SkipReason, WriteOutcome,
};

const PRODUCT_PENGUIN_PASS: u16 = 1;
const PRODUCT_NAME: &str = "Penguin Pass";

pub(crate) fn write_activity(activity: &Activity, dir: &Path) -> io::Result<WriteOutcome> {
    if let Some(reason) = skip_reason(activity) {
        return Ok(WriteOutcome::Skipped(reason));
    }
    let stem = file_stem(&activity.meta)?;
    fs::create_dir_all(dir)?;
    let (tmp, mut file) = create_temp(dir, &stem)?;
    let _remove_tmp = RemoveOnDrop(tmp.clone());
    let mut fit = encode(activity)?;
    {
        let mut writer = FromStd::new(&mut file);
        Encoder::new()
            .encode(&mut writer, &mut fit)
            .map_err(|err| io::Error::new(ErrorKind::InvalidData, err.to_string()))?;
    }
    file.sync_all()?;
    drop(file);
    let path = commit(&tmp, dir, &stem)?;
    Ok(WriteOutcome::Written(path))
}

fn skip_reason(activity: &Activity) -> Option<SkipReason> {
    if activity.summary.elapsed_s < MIN_WRITTEN_ELAPSED_S {
        return Some(SkipReason::TooShort {
            elapsed_s: activity.summary.elapsed_s,
        });
    }
    if activity.summary.distance_m < MIN_WRITTEN_DISTANCE_M {
        return Some(SkipReason::TooShortDistance {
            distance_m: activity.summary.distance_m,
        });
    }
    None
}

fn encode(activity: &Activity) -> io::Result<FIT> {
    let start_unix = unix_of(activity.meta.started_at)?;
    let end_extra = end_offset(activity);
    let start = fit_time(start_unix)?;
    let end = unix_plus(start_unix, end_extra)?;
    let mut messages = Vec::with_capacity(activity.records.len() + activity.laps.len() + 8);
    messages.push(Message::from(file_id(start)));
    messages.extend(device_messages(&activity.meta, start));
    messages.push(Message::from(timer_event(start, typedef::EventType::START)));
    for record in &activity.records {
        messages.push(Message::from(record_message(
            unix_plus(start_unix, record.elapsed_s)?,
            record,
        )));
    }
    messages.push(Message::from(timer_event(
        end,
        typedef::EventType::STOP_ALL,
    )));
    for (index, lap) in activity.laps.iter().enumerate() {
        messages.push(Message::from(lap_message(
            unix_plus(start_unix, whole_seconds(lap.start_s))?,
            unix_plus(start_unix, whole_seconds(lap.end_s))?,
            index,
            lap,
        )));
    }
    messages.push(Message::from(session_message(
        start,
        end,
        &activity.meta,
        &activity.summary,
        activity.laps.len(),
    )));
    messages.push(Message::from(activity_message(end, &activity.summary)));
    Ok(FIT {
        messages,
        ..FIT::default()
    })
}

fn file_id(start: typedef::DateTime) -> mesgdef::FileId {
    let mut file_id = mesgdef::FileId::new();
    file_id.r#type = typedef::File::ACTIVITY;
    file_id.manufacturer = typedef::Manufacturer::DEVELOPMENT;
    file_id.product = PRODUCT_PENGUIN_PASS;
    file_id.time_created = start;
    file_id.product_name = PRODUCT_NAME.into();
    file_id
}

fn device_messages(meta: &RideMeta, start: typedef::DateTime) -> Vec<Message> {
    let mut messages = Vec::with_capacity(2);
    let mut creator = mesgdef::DeviceInfo::new();
    creator.timestamp = start;
    creator.device_index = typedef::DeviceIndex::CREATOR;
    creator.manufacturer = typedef::Manufacturer::DEVELOPMENT;
    creator.product = PRODUCT_PENGUIN_PASS;
    creator.product_name = PRODUCT_NAME.into();
    let version = meta.game_version.trim();
    if !version.is_empty() {
        creator.descriptor = clip(version, 64).into();
    }
    messages.push(Message::from(creator));
    if let Some(name) = meta
        .trainer_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let mut trainer = mesgdef::DeviceInfo::new();
        trainer.timestamp = start;
        trainer.device_index = typedef::DeviceIndex(1);
        trainer.product_name = clip(name, 64).into();
        messages.push(Message::from(trainer));
    }
    messages
}

fn timer_event(when: typedef::DateTime, event_type: typedef::EventType) -> mesgdef::Event {
    let mut event = mesgdef::Event::new();
    event.timestamp = when;
    event.event = typedef::Event::TIMER;
    event.event_type = event_type;
    event
}

fn record_message(when: typedef::DateTime, record: &RecordedSecond) -> mesgdef::Record {
    let mut message = mesgdef::Record::new();
    message.timestamp = when;
    if let Some(distance) = record.distance_m {
        assign_u32(&mut message.distance, scale_u32(distance, 100.0));
    }
    if let Some(speed) = record.speed_mps {
        let raw = scale_u32(f64::from(speed), 1000.0);
        if let Some(raw) = raw {
            assign_u32(&mut message.enhanced_speed, Some(raw));
            if let Ok(narrow) = u16::try_from(raw)
                && narrow != u16::MAX
            {
                message.speed = narrow;
            }
        }
    }
    if let Some(altitude) = record.altitude_m {
        let scaled = altitude_raw(altitude);
        if let Some(raw) = scaled {
            assign_u32(&mut message.enhanced_altitude, Some(raw));
            if let Ok(narrow) = u16::try_from(raw)
                && narrow != u16::MAX
            {
                message.altitude = narrow;
            }
        }
    }
    if let Some(grade) = record.grade_pct.and_then(grade_raw) {
        message.grade = grade;
    }
    if let Some(power) = record.power_w.and_then(round_u16) {
        message.power = power;
    }
    if let Some(cadence) = record.cadence_rpm.and_then(|value| round_u8(value, 0)) {
        message.cadence = cadence;
    }
    if let Some(heart) = record.heart_rate_bpm.and_then(|value| round_u8(value, 1)) {
        message.heart_rate = heart;
    }
    message
}

fn lap_message(
    start: typedef::DateTime,
    end: typedef::DateTime,
    index: usize,
    lap: &LapSummary,
) -> mesgdef::Lap {
    let mut message = mesgdef::Lap::new();
    if let Ok(index) = u16::try_from(index)
        && index != u16::MAX
    {
        message.message_index = typedef::MessageIndex(index);
    }
    message.timestamp = end;
    message.start_time = start;
    message.event = typedef::Event::LAP;
    message.event_type = typedef::EventType::STOP;
    message.sport = typedef::Sport::CYCLING;
    message.sub_sport = typedef::SubSport::VIRTUAL_ACTIVITY;
    let elapsed = (lap.end_s - lap.start_s).max(0.0);
    assign_u32(&mut message.total_elapsed_time, scale_u32(elapsed, 1000.0));
    assign_u32(
        &mut message.total_timer_time,
        scale_u32(lap.moving_s, 1000.0),
    );
    assign_u32(
        &mut message.total_distance,
        scale_u32(lap.distance_m, 100.0),
    );
    if let Some(ascent) = round_u16(lap.ascent_m) {
        message.total_ascent = ascent;
    }
    assign_speed(&mut message.avg_speed, lap.average_speed_mps);
    assign_speed(&mut message.max_speed, lap.max_speed_mps);
    assign_u16(&mut message.avg_power, lap.average_power_w);
    assign_u16(&mut message.max_power, lap.max_power_w);
    assign_u8(&mut message.avg_cadence, lap.average_cadence_rpm, 0);
    assign_u8(&mut message.max_cadence, lap.max_cadence_rpm, 0);
    assign_u8(&mut message.avg_heart_rate, lap.average_heart_rate_bpm, 1);
    assign_u8(&mut message.max_heart_rate, lap.max_heart_rate_bpm, 1);
    message
}

fn session_message(
    start: typedef::DateTime,
    end: typedef::DateTime,
    meta: &RideMeta,
    summary: &ActivitySummary,
    lap_count: usize,
) -> mesgdef::Session {
    let mut message = mesgdef::Session::new();
    message.timestamp = end;
    message.start_time = start;
    message.event = typedef::Event::SESSION;
    message.event_type = typedef::EventType::STOP;
    message.sport = typedef::Sport::CYCLING;
    message.sub_sport = typedef::SubSport::VIRTUAL_ACTIVITY;
    assign_u32(
        &mut message.total_elapsed_time,
        scale_u32(summary.elapsed_s, 1000.0),
    );
    assign_u32(
        &mut message.total_timer_time,
        scale_u32(summary.moving_s, 1000.0),
    );
    assign_u32(
        &mut message.total_distance,
        scale_u32(summary.distance_m, 100.0),
    );
    if let Some(ascent) = round_u16(summary.ascent_m) {
        message.total_ascent = ascent;
    }
    assign_speed(&mut message.avg_speed, summary.average_speed_mps);
    assign_speed(&mut message.max_speed, summary.max_speed_mps);
    assign_u16(&mut message.avg_power, summary.average_power_w);
    assign_u16(&mut message.max_power, summary.max_power_w);
    assign_u16(&mut message.normalized_power, summary.normalized_power_w);
    assign_u8(&mut message.avg_cadence, summary.average_cadence_rpm, 0);
    assign_u8(&mut message.max_cadence, summary.max_cadence_rpm, 0);
    assign_u8(
        &mut message.avg_heart_rate,
        summary.average_heart_rate_bpm,
        1,
    );
    assign_u8(&mut message.max_heart_rate, summary.max_heart_rate_bpm, 1);
    if let Ok(laps) = u16::try_from(lap_count)
        && laps != u16::MAX
    {
        message.num_laps = laps;
    }
    if meta.ftp_w > 0 && meta.ftp_w != u16::MAX {
        message.threshold_power = meta.ftp_w;
    }
    message
}

fn activity_message(end: typedef::DateTime, summary: &ActivitySummary) -> mesgdef::Activity {
    let mut message = mesgdef::Activity::new();
    message.timestamp = end;
    message.num_sessions = 1;
    message.r#type = typedef::Activity::MANUAL;
    message.event = typedef::Event::ACTIVITY;
    message.event_type = typedef::EventType::STOP;
    assign_u32(
        &mut message.total_timer_time,
        scale_u32(summary.moving_s, 1000.0),
    );
    message
}

fn assign_u32(slot: &mut u32, value: Option<u32>) {
    if let Some(value) = value {
        *slot = value;
    }
}

fn assign_u16(slot: &mut u16, value: Option<f32>) {
    if let Some(value) = value.and_then(round_u16) {
        *slot = value;
    }
}

fn assign_u8(slot: &mut u8, value: Option<f32>, min: u8) {
    if let Some(value) = value.and_then(|value| round_u8(value, min)) {
        *slot = value;
    }
}

fn assign_speed(slot: &mut u16, meters_per_second: Option<f32>) {
    let Some(speed) = meters_per_second else {
        return;
    };
    let Some(raw) = scale_u32(f64::from(speed), 1000.0) else {
        return;
    };
    if let Ok(narrow) = u16::try_from(raw)
        && narrow != u16::MAX
    {
        *slot = narrow;
    }
}

fn scale_u32(value: f64, scale: f64) -> Option<u32> {
    if !value.is_finite() || !scale.is_finite() || scale <= 0.0 || value < 0.0 {
        return None;
    }
    let scaled = (value * scale).round();
    if !scaled.is_finite() || scaled < 0.0 || scaled >= f64::from(u32::MAX) {
        return None;
    }
    Some(scaled as u32)
}

fn altitude_raw(meters: f32) -> Option<u32> {
    if !meters.is_finite() {
        return None;
    }
    let scaled = ((f64::from(meters) + 500.0) * 5.0).round();
    if !scaled.is_finite() || scaled < 0.0 || scaled >= f64::from(u32::MAX) {
        return None;
    }
    Some(scaled as u32)
}

fn grade_raw(percent: f32) -> Option<i16> {
    if !percent.is_finite() {
        return None;
    }
    let scaled = (f64::from(percent) * 100.0).round();
    if !scaled.is_finite() || scaled < f64::from(i16::MIN) || scaled >= f64::from(i16::MAX) {
        return None;
    }
    Some(scaled as i16)
}

fn round_u16(value: f32) -> Option<u16> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let rounded = value.round();
    if rounded >= f32::from(u16::MAX) {
        return None;
    }
    Some(rounded as u16)
}

fn round_u8(value: f32, min: u8) -> Option<u8> {
    if !value.is_finite() || value < f32::from(min) {
        return None;
    }
    let rounded = value.round();
    if rounded >= f32::from(u8::MAX) {
        return None;
    }
    Some(rounded as u8)
}

fn whole_seconds(elapsed_s: f64) -> u64 {
    if !elapsed_s.is_finite() || elapsed_s <= 0.0 {
        return 0;
    }
    let floored = elapsed_s.floor();
    if floored > 1.0e12 { 0 } else { floored as u64 }
}

fn end_offset(activity: &Activity) -> u64 {
    let last_record = activity
        .records
        .last()
        .map(|record| record.elapsed_s)
        .unwrap_or(0);
    let elapsed = activity.summary.elapsed_s;
    if !elapsed.is_finite() || elapsed <= 0.0 {
        return last_record;
    }
    let rounded = elapsed.round();
    if !rounded.is_finite() || rounded <= 0.0 || rounded > 1.0e12 {
        return last_record;
    }
    (rounded as u64).max(last_record)
}

fn unix_of(time: SystemTime) -> io::Result<u64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "ride start is before 1970-01-01"))
}

fn fit_time(unix: u64) -> io::Result<typedef::DateTime> {
    let secs = i64::try_from(unix).map_err(|_| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "timestamp does not fit in the FIT clock",
        )
    })?;
    let stamp = typedef::DateTime::from_unix_timestamp(secs);
    if stamp.unix_timestamp().is_none() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "timestamp is outside the FIT range",
        ));
    }
    Ok(stamp)
}

fn unix_plus(start: u64, extra: u64) -> io::Result<typedef::DateTime> {
    let sum = start
        .checked_add(extra)
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "ride timestamp overflowed"))?;
    fit_time(sum)
}

fn file_stem(meta: &RideMeta) -> io::Result<String> {
    let local = local_unix(unix_of(meta.started_at)?, meta.utc_offset_s)?;
    let stamp = format_stamp(local);
    Ok(format!("{stamp}-{}", course_slug(&meta.course_name)))
}

/// Shift a UTC unix time by a fixed offset for the file name only.
fn local_unix(unix: u64, offset_s: i32) -> io::Result<u64> {
    let shifted = i64::try_from(unix)
        .unwrap_or(i64::MAX)
        .saturating_add(i64::from(offset_s));
    if shifted < 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "local file name is before 1970-01-01",
        ));
    }
    u64::try_from(shifted).map_err(|_| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "local file name does not fit in the clock",
        )
    })
}

fn format_stamp(unix: u64) -> String {
    let days = unix / 86_400;
    let time_of_day = unix % 86_400;
    let hour = time_of_day / 3_600;
    let minute = (time_of_day % 3_600) / 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}-{hour:02}{minute:02}")
}

/// Howard Hinnant's `civil_from_days`. `days` is the count since 1970-01-01.
fn civil_from_days(days: u64) -> (i32, u32, u32) {
    let z = i64::try_from(days).unwrap_or(i64::MAX) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = u64::try_from(z - era * 146_097).unwrap_or(0);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = i64::try_from(yoe).unwrap_or(0) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (
        i32::try_from(year).unwrap_or(i32::MAX),
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

fn course_slug(name: &str) -> String {
    let mut slug = String::new();
    let mut hyphen = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            hyphen = false;
        } else if !slug.is_empty() && !hyphen {
            slug.push('-');
            hyphen = true;
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    if slug.len() > 80 {
        slug.truncate(80);
        if slug.ends_with('-') {
            slug.pop();
        }
    }
    if slug.is_empty() {
        "ride".to_owned()
    } else {
        slug
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn create_temp(dir: &Path, stem: &str) -> io::Result<(PathBuf, File)> {
    for n in 0..100 {
        let name = if n == 0 {
            format!(".{stem}.{}.tmp", std::process::id())
        } else {
            format!(".{stem}.{}.{n}.tmp", std::process::id())
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "could not create a temporary activity file",
    ))
}

fn commit(tmp: &Path, dir: &Path, stem: &str) -> io::Result<PathBuf> {
    for n in 1..10_000 {
        let name = if n == 1 {
            format!("{stem}.fit")
        } else {
            format!("{stem}-{n}.fit")
        };
        let dest = dir.join(name);
        match fs::hard_link(tmp, &dest) {
            Ok(()) => return Ok(dest),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) if dest.exists() => continue,
            Err(_) => {
                fs::rename(tmp, &dest)?;
                return Ok(dest);
            }
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "no free activity file name",
    ))
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Recorder, RideMeta, RideSample};
    use fitparser::Value;
    use fitparser::de::from_reader_with_options;
    use fitparser::profile::MesgNum;
    use std::collections::HashSet;
    use std::time::Duration;

    fn meta(course: &str) -> RideMeta {
        RideMeta {
            started_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            course_name: course.to_owned(),
            rider_mass_kg: 75.0,
            bike_mass_kg: 9.0,
            ftp_w: 250,
            trainer_name: Some("Tacx Neo".to_owned()),
            game_version: "0.1.0".to_owned(),
            utc_offset_s: 0,
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("pp-record-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn long_enough(course: &str, seconds: u32, distance_per_second: f64) -> Activity {
        let mut recorder = Recorder::new(meta(course));
        for second in 0..=seconds {
            let elapsed = f64::from(second);
            recorder.push(RideSample {
                elapsed_s: elapsed,
                power_w: Some(180.0),
                cadence_rpm: Some(90.0),
                heart_rate_bpm: Some(140),
                speed_mps: 5.0,
                distance_m: distance_per_second * elapsed,
                altitude_m: 1.0,
                grade_pct: 0.0,
                lap: 1,
            });
        }
        recorder.finish()
    }

    #[test]
    fn short_rides_are_not_written() {
        let dir = temp_dir("skip");
        let empty = Recorder::new(meta("Penguin Pass")).finish();
        match empty.write(&dir).unwrap() {
            WriteOutcome::Skipped(SkipReason::TooShort { elapsed_s }) => {
                assert_eq!(elapsed_s, 0.0);
            }
            other => panic!("expected a short-ride skip, got {other:?}"),
        }

        let mut brief = Recorder::new(meta("Penguin Pass"));
        brief.push(RideSample {
            elapsed_s: 0.0,
            distance_m: 0.0,
            power_w: Some(200.0),
            cadence_rpm: None,
            heart_rate_bpm: None,
            speed_mps: 8.0,
            altitude_m: 0.0,
            grade_pct: 0.0,
            lap: 1,
        });
        brief.push(RideSample {
            elapsed_s: 30.0,
            distance_m: 500.0,
            power_w: Some(200.0),
            cadence_rpm: None,
            heart_rate_bpm: None,
            speed_mps: 8.0,
            altitude_m: 0.0,
            grade_pct: 0.0,
            lap: 1,
        });
        match brief.finish().write(&dir).unwrap() {
            WriteOutcome::Skipped(SkipReason::TooShort { .. }) => {}
            other => panic!("duration is checked first, got {other:?}"),
        }

        let mut far = Recorder::new(meta("Penguin Pass"));
        far.push(RideSample {
            elapsed_s: 0.0,
            distance_m: 0.0,
            power_w: Some(100.0),
            cadence_rpm: None,
            heart_rate_bpm: None,
            speed_mps: 1.0,
            altitude_m: 0.0,
            grade_pct: 0.0,
            lap: 1,
        });
        far.push(RideSample {
            elapsed_s: 90.0,
            distance_m: 50.0,
            power_w: Some(100.0),
            cadence_rpm: None,
            heart_rate_bpm: None,
            speed_mps: 1.0,
            altitude_m: 0.0,
            grade_pct: 0.0,
            lap: 1,
        });
        match far.finish().write(&dir).unwrap() {
            WriteOutcome::Skipped(SkipReason::TooShortDistance { distance_m }) => {
                assert_eq!(distance_m, 50.0);
            }
            other => panic!("expected a short-distance skip, got {other:?}"),
        }
        assert!(fs::read_dir(&dir).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_names_do_not_overwrite() {
        let dir = temp_dir("name");
        let activity = long_enough("Penguin Pass!", 60, 2.0);
        let WriteOutcome::Written(first) = activity.write(&dir).unwrap() else {
            panic!("a 60 s, 120 m ride is written");
        };
        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some("2023-11-14-2213-penguin-pass.fit")
        );
        let bytes = fs::read(&first).unwrap();
        let WriteOutcome::Written(second) = activity.write(&dir).unwrap() else {
            panic!("the second ride still writes");
        };
        assert_eq!(
            second.file_name().and_then(|name| name.to_str()),
            Some("2023-11-14-2213-penguin-pass-2.fit")
        );
        assert_eq!(fs::read(&first).unwrap(), bytes);

        let blank = long_enough("   ", 60, 2.0);
        let WriteOutcome::Written(third) = blank.write(&dir).unwrap() else {
            panic!("an empty course name still writes");
        };
        assert_eq!(
            third.file_name().and_then(|name| name.to_str()),
            Some("2023-11-14-2213-ride.fit")
        );
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp") || name.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_name_uses_the_local_offset_and_fit_timestamps_stay_utc() {
        // 2023-11-15 06:26:00 UTC.
        let started = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_029_560);
        let dir = temp_dir("offset");
        let utc = write_offset(&dir, started, 0);
        let west = write_offset(&dir, started, -480 * 60);
        let odd = write_offset(&dir, started, 345 * 60);
        assert_eq!(fit_name(&utc), "2023-11-15-0626-offset.fit");
        assert_eq!(fit_name(&west), "2023-11-14-2226-offset.fit");
        assert_eq!(fit_name(&odd), "2023-11-15-1211-offset.fit");
        assert_eq!(record_times(&utc), record_times(&west));
        assert_eq!(record_times(&utc), record_times(&odd));
        let _ = fs::remove_dir_all(&dir);
    }

    fn write_offset(dir: &Path, started: SystemTime, offset_s: i32) -> PathBuf {
        let mut header = meta("Offset");
        header.started_at = started;
        header.utc_offset_s = offset_s;
        let mut recorder = Recorder::new(header);
        for second in 0..=60 {
            let elapsed = f64::from(second);
            recorder.push(RideSample {
                elapsed_s: elapsed,
                power_w: Some(180.0),
                cadence_rpm: Some(90.0),
                heart_rate_bpm: Some(140),
                speed_mps: 5.0,
                distance_m: 2.0 * elapsed,
                altitude_m: 1.0,
                grade_pct: 0.0,
                lap: 1,
            });
        }
        let WriteOutcome::Written(path) = recorder.finish().write(dir).unwrap() else {
            panic!("offset ride should be written");
        };
        path
    }

    fn fit_name(path: &Path) -> String {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap()
            .to_owned()
    }

    fn record_times(path: &Path) -> Vec<i64> {
        let bytes = fs::read(path).unwrap();
        parse(&bytes)
            .iter()
            .filter(|message| message.kind() == MesgNum::Record)
            .map(timestamp)
            .collect()
    }

    #[test]
    fn ten_minute_ride_round_trips_through_an_independent_reader() {
        let dir = temp_dir("fit");
        let mut recorder = Recorder::new(meta("Penguin Pass"));
        for second in 0u32..600 {
            let elapsed = f64::from(second);
            recorder.push(RideSample {
                elapsed_s: elapsed,
                power_w: Some(180.0 + (second % 7) as f32),
                cadence_rpm: Some(80.0 + (second % 3) as f32),
                heart_rate_bpm: Some(120 + u16::try_from(second % 5).unwrap()),
                speed_mps: 8.0,
                distance_m: 8.0 * elapsed,
                altitude_m: elapsed as f32,
                grade_pct: 1.5,
                lap: 1 + second / 200,
            });
        }
        let activity = recorder.finish();
        assert_eq!(activity.records.len(), 600);
        assert!(activity.summary.normalized_power_w.is_some());
        let WriteOutcome::Written(path) = activity.write(&dir).unwrap() else {
            panic!("the ten minute ride is written");
        };
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("2023-11-14-2213-penguin-pass.fit")
        );

        let bytes = fs::read(&path).unwrap();
        assert!(bytes.windows(4).any(|window| window == b".FIT"));
        let parsed = parse(&bytes);
        let records: Vec<_> = parsed
            .iter()
            .filter(|message| message.kind() == MesgNum::Record)
            .collect();
        assert_eq!(records.len(), 600);
        let mut times: Vec<i64> = records.iter().map(|record| timestamp(record)).collect();
        times.sort_unstable();
        assert_eq!(times.first().copied(), Some(1_700_000_000));
        assert_eq!(times.last().copied(), Some(1_700_000_599));

        let at = |second: i64| {
            records
                .iter()
                .find(|record| timestamp(record) == 1_700_000_000 + second)
                .unwrap_or_else(|| panic!("missing record at {second}"))
        };
        assert_close(number(field(at(0), "power")), 180.0);
        assert_close(number(field(at(0), "cadence")), 80.0);
        assert_close(number(field(at(0), "heart_rate")), 120.0);
        assert_close(number(field(at(1), "power")), 181.0);
        assert_close(number(field(at(1), "cadence")), 81.0);
        assert_close(number(field(at(1), "heart_rate")), 121.0);
        assert_close(number(field(at(599), "power")), 184.0);
        assert_close(number(field(at(599), "cadence")), 82.0);
        assert_close(number(field(at(599), "heart_rate")), 124.0);
        assert_close(number(field(at(599), "distance")), 8.0 * 599.0);
        assert_close(speed_mps(at(0)), 8.0);
        assert_close(speed_mps(at(599)), 8.0);

        let session = parsed
            .iter()
            .find(|message| message.kind() == MesgNum::Session)
            .expect("session message");
        assert!(
            is_named(field(session, "sport"), "cycling", 2),
            "{}",
            describe(session)
        );
        assert!(
            is_named(field(session, "sub_sport"), "virtual_activity", 58),
            "{}",
            describe(session)
        );
        assert_close(number(field(session, "total_distance")), 8.0 * 599.0);
        let normalized = activity.summary.normalized_power_w.unwrap().round();
        assert_close(
            number(field(session, "normalized_power")),
            f64::from(normalized),
        );
        assert_eq!(
            parsed
                .iter()
                .filter(|message| message.kind() == MesgNum::Lap)
                .count(),
            3
        );
        for message in &parsed {
            for field in message.fields() {
                assert_ne!(field.name(), "position_lat", "{}", describe(message));
                assert_ne!(field.name(), "position_long");
                assert_ne!(field.name(), "start_position_lat");
                assert_ne!(field.name(), "start_position_long");
                assert_ne!(field.name(), "end_position_lat");
                assert_ne!(field.name(), "end_position_long");
            }
        }
        let names: Vec<String> = parsed
            .iter()
            .filter_map(|message| field(message, "product_name"))
            .filter_map(text)
            .map(str::to_owned)
            .collect();
        assert!(names.iter().any(|name| name == "Penguin Pass"), "{names:?}");
        assert!(names.iter().any(|name| name == "Tacx Neo"), "{names:?}");

        let mut corrupt = bytes.clone();
        corrupt[20] ^= 0xff;
        let mut cursor = std::io::Cursor::new(corrupt);
        let options = HashSet::new();
        assert!(
            from_reader_with_options(&mut cursor, &options).is_err(),
            "a damaged file must fail the FIT CRC check"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_sensors_are_omitted_from_the_file() {
        let dir = temp_dir("absent");
        let mut recorder = Recorder::new(meta("Penguin Pass"));
        for second in 0u32..=70 {
            let elapsed = f64::from(second);
            let present = second != 10;
            recorder.push(RideSample {
                elapsed_s: elapsed,
                power_w: present.then_some(200.0),
                cadence_rpm: present.then_some(85.0),
                heart_rate_bpm: present.then_some(130),
                speed_mps: 4.0,
                distance_m: 4.0 * elapsed,
                altitude_m: 2.0,
                grade_pct: 0.0,
                lap: 1,
            });
        }
        let WriteOutcome::Written(path) = recorder.finish().write(&dir).unwrap() else {
            panic!("70 s and 280 m is written");
        };
        let parsed = parse(&fs::read(&path).unwrap());
        let quiet = parsed
            .iter()
            .filter(|message| message.kind() == MesgNum::Record)
            .find(|record| timestamp(record) == 1_700_000_010)
            .expect("second 10");
        assert!(field(quiet, "power").is_none(), "{}", describe(quiet));
        assert!(field(quiet, "cadence").is_none(), "{}", describe(quiet));
        assert!(field(quiet, "heart_rate").is_none(), "{}", describe(quiet));
        assert_close(speed_mps(quiet), 4.0);
        let neighbor = parsed
            .iter()
            .filter(|message| message.kind() == MesgNum::Record)
            .find(|record| timestamp(record) == 1_700_000_011)
            .expect("second 11");
        assert_close(number(field(neighbor, "power")), 200.0);
        assert_close(number(field(neighbor, "cadence")), 85.0);
        assert_close(number(field(neighbor, "heart_rate")), 130.0);
        let _ = fs::remove_dir_all(&dir);
    }

    fn parse(bytes: &[u8]) -> Vec<fitparser::FitDataRecord> {
        let mut cursor = std::io::Cursor::new(bytes);
        let options = HashSet::new();
        from_reader_with_options(&mut cursor, &options).expect("FIT CRC and structure")
    }

    fn field<'a>(message: &'a fitparser::FitDataRecord, name: &str) -> Option<&'a Value> {
        message
            .fields()
            .iter()
            .find(|field| field.name() == name)
            .map(|field| field.value())
    }

    fn timestamp(message: &fitparser::FitDataRecord) -> i64 {
        match field(message, "timestamp") {
            Some(Value::Timestamp(time)) => time.timestamp(),
            Some(value) => number(Some(value)).unwrap_or(0.0) as i64,
            None => 0,
        }
    }

    fn speed_mps(message: &fitparser::FitDataRecord) -> Option<f64> {
        number(field(message, "enhanced_speed")).or_else(|| number(field(message, "speed")))
    }

    fn number(value: Option<&Value>) -> Option<f64> {
        match value? {
            Value::Byte(v) | Value::UInt8(v) | Value::UInt8z(v) | Value::Enum(v) => {
                Some(f64::from(*v))
            }
            Value::SInt8(v) => Some(f64::from(*v)),
            Value::UInt16(v) | Value::UInt16z(v) => Some(f64::from(*v)),
            Value::SInt16(v) => Some(f64::from(*v)),
            Value::UInt32(v) | Value::UInt32z(v) => Some(f64::from(*v)),
            Value::SInt32(v) => Some(f64::from(*v)),
            Value::Float32(v) => Some(f64::from(*v)),
            Value::Float64(v) => Some(*v),
            Value::SInt64(v) => Some(*v as f64),
            Value::UInt64(v) | Value::UInt64z(v) => Some(*v as f64),
            Value::String(text) => text.trim_end_matches('\0').parse().ok(),
            Value::Timestamp(time) => Some(time.timestamp() as f64),
            Value::Array(_) | Value::Invalid => None,
        }
    }

    fn text(value: &Value) -> Option<&str> {
        match value {
            Value::String(text) => Some(text.trim_end_matches('\0')),
            _ => None,
        }
    }

    fn is_named(value: Option<&Value>, name: &str, code: u8) -> bool {
        match value {
            Some(Value::String(text)) => text.trim_end_matches('\0').eq_ignore_ascii_case(name),
            Some(Value::Enum(v) | Value::UInt8(v) | Value::Byte(v)) => *v == code,
            Some(_) => number(value) == Some(f64::from(code)),
            None => false,
        }
    }

    fn assert_close(actual: Option<f64>, expected: f64) {
        let Some(actual) = actual else {
            panic!("missing value, expected {expected}");
        };
        assert!((actual - expected).abs() < 0.02, "{actual} vs {expected}");
    }

    fn describe(message: &fitparser::FitDataRecord) -> String {
        message
            .fields()
            .iter()
            .map(|field| format!("{}={:?}", field.name(), field.value()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
