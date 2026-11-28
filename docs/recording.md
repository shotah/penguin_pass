# Ride recording

`pp-record` turns a ride into one FIT activity file. It does not link Godot, Bluetooth, or the network. `pp-session` already records from `Session` in `crates/pp-session/src/session.rs`. The format below is the contract. The plan at the bottom is already applied.

## What is recorded

One sample arrives every frame. The file keeps one record per second:

| Field | What is stored |
| --- | --- |
| Power, cadence, heart rate | Mean of the samples in that second. A sensor that did not report is left out. Zero watts and zero cadence count. A heart rate of 0 does not. |
| Speed, distance, altitude, grade | The last finite sample in that second. Negative speed and distance are ignored. Altitude and grade may be negative. |
| Time | The ride clock, not the wall clock of each frame. |

A gap of minutes stays a gap. The missing seconds are not filled in. A sample with a non-finite or negative clock, or a clock that runs backwards, is dropped. An empty ride finishes without panicking.

The session summary has elapsed time, moving time, distance, ascent, average and maximum power, cadence, heart rate, and speed, and normalised power. Moving time is time above 0.5 m/s. A gap longer than 5 seconds is not moving time. Ascent is the sum of the rises between successive one-second altitudes. Average speed is distance divided by moving time. Normalised power is the fourth-power mean of the 30-second rolling averages of the one-second power values. It is omitted when the ride is shorter than 30 seconds, or when power never covers 30 consecutive recorded seconds. Missing power is not treated as 0 watts.

There is one lap per course lap. Functional threshold power is the session threshold. The trainer name, when there is one, is a device info. The game version is the creator's descriptor.

Rider mass and bike mass are on `RideMeta` for the caller. They are not FIT fields. The course title is the file name, not a message inside the file.

## Format

The file is FIT, written with `rustyfit`. FIT can mark the ride as cycling with sub-sport virtual activity, which is how an indoor trainer ride is labelled, and the training sites import it.

There is no latitude or longitude. The courses have no real coordinates yet, so a site shows the ride without a map.

The manufacturer id is the FIT development id (255). Penguin Pass does not have an ANT+ manufacturer id. A site may show a generic device name beside "Penguin Pass".

A ride shorter than 60 seconds, or shorter than 100 meters, is not written. `Activity::write` returns `Skipped` for that. It is not an I/O error. Duration is checked first.

The file name is `YYYY-MM-DD-HHMM-<course>.fit`, in local time (`RideMeta::utc_offset_s`, default 0 so the name stays UTC). Timestamps inside the file stay UTC. The course part is lower case, with a hyphen where the title is not a letter or a digit. An empty title becomes `ride`. The write uses a temporary name in the same directory, flushes, then links it into place. An existing file is not replaced. The next one is `-2`, then `-3`.

## Where the files go

By default, a `rides` folder next to the settings file. `rides_folder` in that file overrides it. `set_rides_dir` overrides both. With none of those set, the session writes nothing.

On this machine, a normal `./x play` loads `user://settings.toml`, which is:

`/home/deck/.local/share/godot/app_userdata/Penguin Pass/settings.toml`

Once the game passes that path to `set_settings_path`, the files land in:

`/home/deck/.local/share/godot/app_userdata/Penguin Pass/rides`

`Activity::write` creates the directory if it is missing. The session does not know `user://`.

## Upload one by hand

Log in on the Strava website. Open <https://www.strava.com/upload/select>, choose the `.fit` file, and save the activity. Strava's help page is [How do I get my activities to Strava?](https://support.strava.com/en-us/articles/15402066-how-do-i-get-my-activities-to-strava). The site accepts FIT, TCX, and GPX, up to 25 MB.

The ride has no map. The upload reference says a file with no position is marked stationary unless the caller sets otherwise, and `sport_type=VirtualRide` on that call overrides the type in the file. Whether the website's own uploader shows this file as a virtual ride is not confirmed here.

## Later, the Strava API

Read on 20 November 2026. No account, and no API call.

`POST https://www.strava.com/api/v3/uploads` takes `data_type` of `fit`, `fit.gz`, `tcx`, `tcx.gz`, `gpx`, `gpx.gz`, or `json`. JSON is only for strength training. The call needs the `activity:write` scope. The reference is [Uploading to Strava](https://developers.strava.com/docs/uploads/).

The same page says the activity type is taken from `session.sport` and `session.sub_sport`. This file sets cycling and virtual activity. `sport_type` on the upload, including `VirtualRide`, overrides that. `trainer` set to 1 forces a stationary activity. A file with no latitude or longitude is marked stationary on its own. This file has no position.

The token exchange is documented on [Authentication](https://developers.strava.com/docs/authentication/). The parameters are `client_id`, `client_secret`, `code`, and `grant_type=authorization_code`. The client secret is required. The page does not mention PKCE or a code verifier. The prose says `POST https://www.strava.com/oauth/token`. The curl example on that page says `POST https://www.strava.com/api/v3/oauth/token`. `localhost` and `127.0.0.1` are allowed redirect hosts.

A new application can authenticate one athlete. Upgrading it in the dashboard raises that to 10. Past 10 needs a review. That is [Getting Started](https://developers.strava.com/docs/getting-started/) and [Rate Limits](https://developers.strava.com/docs/rate-limits/).

The default overall limit is 200 requests per 15 minutes and 2,000 per day. The default limit on everything except uploads is 100 per 15 minutes and 1,000 per day. After the upgrade, the read limit is 200 per 15 minutes and 2,000 per day, and the overall limit is 400 per 15 minutes and 4,000 per day.

A community thread, not the API reference, says a virtual upload should use sub-sport virtual activity and should not set `trainer`, and that a map needs GPS in the file: [FIT upload, virtual row](https://communityhub.strava.com/developers-api-7/fit-upload-virtualrow-with-map-timestamp-handling-11447).

## What is not here

No map coordinates. Masses are not FIT fields. The file writer does not open Bluetooth or the network. Upload is `pp-strava`, described in `docs/strava.md`.

## Lines for `pp-session`

Already applied in `crates/pp-session/src/session.rs`. The block below is the original plan.

In `Cargo.toml`, under `[dependencies]`:

```toml
pp-record = { path = "../pp-record" }
```

On `Session`, next to the trace directory:

```rust
recorder: Option<pp_record::Recorder>,
rides_dir: Option<std::path::PathBuf>,
```

`rides_dir` starts as `None`. Add `set_rides_dir`, the same shape as `set_trace_dir`. Godot passes the absolute path of `user://rides`.

```rust
fn start_recording(&mut self) {
    let rider = self.rider();
    self.recorder = Some(pp_record::Recorder::new(pp_record::RideMeta {
        started_at: std::time::SystemTime::now(),
        course_name: self.course().name().to_owned(),
        rider_mass_kg: rider.rider_mass_kg,
        bike_mass_kg: rider.bike_mass_kg,
        ftp_w: rider.ftp_w,
        trainer_name: self.connection().trainer_name,
        game_version: env!("CARGO_PKG_VERSION").to_owned(),
    }));
}

fn record_sample(&mut self, frame: &pp_core::Telemetry) {
    let readout = self.readout();
    let Some(recorder) = self.recorder.as_mut() else {
        return;
    };
    recorder.push(pp_record::RideSample {
        elapsed_s: frame.elapsed_s,
        // readout is None when the link is down. telemetry would say 0 W.
        power_w: readout.power_w,
        cadence_rpm: readout.cadence_rpm,
        heart_rate_bpm: readout.heart_rate_bpm.map(u16::from),
        speed_mps: frame.speed_mps,
        distance_m: frame.distance_m,
        altitude_m: frame.elevation_m,
        grade_pct: frame.grade_pct,
        lap: frame.lap,
    });
}

fn save_ride(&mut self) {
    let Some(recorder) = self.recorder.take() else {
        return;
    };
    let Some(dir) = self.rides_dir.clone() else {
        return;
    };
    match recorder.finish().write(&dir) {
        Ok(pp_record::WriteOutcome::Written(path)) => {
            eprintln!("pp-session: ride {}", path.display());
        }
        Ok(pp_record::WriteOutcome::Skipped(reason)) => {
            eprintln!("pp-session: ride not saved ({reason:?})");
        }
        Err(err) => eprintln!("pp-session: ride not saved: {err}"),
    }
}
```

Where to call them:

- `Session::new`, after the ride exists. A new session is already `Riding`.
- `begin_ride`, when the phase leaves `Ready`.
- `restart`, instead of keeping the old recorder. Do not write that abandoned ride.
- `hold_at_start` sets `self.recorder = None`. The title screen is not a ride.
- The `Riding` arm of `tick`, after `stats.record`, calls `self.record_sample(&telemetry)`. Do not call it from `tick_ready`, and do not call it after `end_ride`.
- `end_ride` calls `self.save_ride()` after `stats.finish()`.
- `Drop`, if a recorder is still there, calls `save_ride` so closing the window still writes. A short ride comes back as `Skipped` and the drop continues.
