//! Bots, the workout runner, and the Strava link.
//!
//! Strava is opened only when a settings directory is known. The default
//! endpoints are the real site, and nothing is requested until the rider
//! connects or a ride is uploaded.

use std::path::Path;

use pp_bots::{BotConfig, Peloton, PlayerInput};
use pp_course::Course;
use pp_strava::{Endpoints, StravaEvent, StravaLink, StravaState, UploadMeta};
use pp_workout::{
    ErgGuide, Library, Workout, WorkoutEvent, WorkoutResult, WorkoutRun, WorkoutView,
};

use crate::course_sel::CourseChoice;
use crate::events::RideEvent;
use crate::play::{WorkoutBar, WorkoutSummary};

/// What the trainer should do with ERG this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErgSend {
    /// Keep the previous mode.
    Keep,
    /// Send this target and do not send grade.
    Hold(u16),
    /// Stop ERG and send grade again.
    Release,
}

pub(crate) struct Group {
    peloton: Option<Peloton>,
    peloton_count: u8,
    peloton_seed: u64,
    peloton_len: f64,
    workout_id: Option<String>,
    workout_name: Option<String>,
    profile: Vec<WorkoutBar>,
    run: Option<WorkoutRun>,
    erg: ErgGuide,
    erg_hold: bool,
    erg_w: Option<u16>,
    result: Option<WorkoutResult>,
    strava: Option<StravaLink>,
    endpoints: Endpoints,
    uploaded: bool,
}

impl Group {
    pub(crate) fn new() -> Self {
        Self {
            peloton: None,
            peloton_count: u8::MAX,
            peloton_seed: 0,
            peloton_len: -1.0,
            workout_id: None,
            workout_name: None,
            profile: Vec::new(),
            run: None,
            erg: ErgGuide::default(),
            erg_hold: false,
            erg_w: None,
            result: None,
            strava: None,
            endpoints: Endpoints::strava(),
            uploaded: false,
        }
    }

    pub(crate) fn on_ride_start(
        &mut self,
        course: &Course,
        choice: &CourseChoice,
        count: u8,
        ftp_w: u16,
        dir: Option<&Path>,
    ) {
        self.peloton_count = u8::MAX;
        self.sync_count(course, choice, count);
        self.restart_run(ftp_w, &load_workouts(dir));
    }

    pub(crate) fn sync_count(&mut self, course: &Course, choice: &CourseChoice, count: u8) {
        let count = if !course.bots() { 0 } else { count.min(24) };
        let seed = seed_of(choice);
        let len = course.length_m();
        if self.peloton_count == count
            && self.peloton_seed == seed
            && (self.peloton_len - len).abs() < 1.0e-3
        {
            return;
        }
        self.rebuild_peloton(course, seed, count);
    }

    pub(crate) fn draft_factor(&self, player: &PlayerInput) -> f32 {
        self.peloton
            .as_ref()
            .map(|peloton| peloton.draft_factor(player))
            .unwrap_or(1.0)
    }

    pub(crate) fn tick_bots(&mut self, dt_s: f32, player: &PlayerInput) {
        if let Some(peloton) = &mut self.peloton {
            let _ = peloton.tick(dt_s, player);
        }
    }

    pub(crate) fn bots(&self) -> Vec<pp_bots::BotState> {
        self.peloton
            .as_ref()
            .map(|peloton| peloton.riders().to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn draft_saving_pct(&self, player: &PlayerInput) -> u8 {
        if self.peloton.is_none() {
            return 0;
        }
        saving_pct(self.draft_factor(player))
    }

    pub(crate) fn workouts(dir: Option<&Path>) -> Vec<WorkoutSummary> {
        load_workouts(dir)
            .iter()
            .map(|workout| WorkoutSummary {
                id: workout.id.clone(),
                name: workout.name.clone(),
                description: workout.description.clone(),
                duration_s: workout.duration_s(),
                ramp_test: workout.ramp_test,
                profile: bars(workout),
            })
            .collect()
    }

    pub(crate) fn selected_workout_id(&self) -> Option<&str> {
        self.workout_id.as_deref()
    }

    pub(crate) fn select(&mut self, id: Option<&str>, dir: Option<&Path>, ftp_w: u16) {
        let Some(id) = id else {
            self.clear_workout();
            return;
        };
        let workouts = load_workouts(dir);
        let Some(workout) = workouts.into_iter().find(|workout| workout.id == id) else {
            return;
        };
        self.workout_id = Some(workout.id.clone());
        self.profile = bars(&workout);
        self.start_run(&workout, ftp_w);
    }

    pub(crate) fn set_bias(&mut self, percent: i8) {
        if let Some(run) = &mut self.run {
            run.set_bias(percent);
        }
    }

    pub(crate) fn skip(&mut self) -> (Vec<RideEvent>, ErgSend) {
        if let Some(run) = &mut self.run {
            run.skip_step();
        }
        self.tick_workout(0.0, 0.0, None)
    }

    pub(crate) fn tick_workout(
        &mut self,
        dt_s: f32,
        power_w: f32,
        cadence_rpm: Option<f32>,
    ) -> (Vec<RideEvent>, ErgSend) {
        let packed = {
            let Some(run) = &mut self.run else {
                return (Vec::new(), ErgSend::Keep);
            };
            let raw = run.tick(dt_s, power_w, cadence_rpm);
            let mut events = Vec::new();
            let mut finished = None;
            for event in raw {
                match event {
                    WorkoutEvent::StepStarted {
                        label, target_w, ..
                    } => {
                        events.push(RideEvent::WorkoutStepStarted { label, target_w });
                    }
                    WorkoutEvent::StepEndingSoon { seconds } => {
                        events.push(RideEvent::WorkoutStepEndingSoon { seconds });
                    }
                    WorkoutEvent::Cue { text } => events.push(RideEvent::WorkoutCue { text }),
                    WorkoutEvent::Finished => {
                        let result = run.result();
                        let ramp_test_ftp_w = rounded_ftp(result.ramp_test_ftp_w);
                        finished = Some(result);
                        events.push(RideEvent::WorkoutFinished { ramp_test_ftp_w });
                    }
                }
            }
            (events, run.target_w(), finished)
        };
        let (events, target, finished) = packed;
        if let Some(result) = finished {
            self.result = Some(result);
        }
        let pedaling = match cadence_rpm {
            Some(rpm) => rpm >= 5.0,
            None => power_w >= 15.0,
        };
        let command = self.erg.update(dt_s, target, pedaling);
        let send = if command.send {
            match command.watts {
                Some(watts) => ErgSend::Hold(watts),
                None => ErgSend::Release,
            }
        } else {
            ErgSend::Keep
        };
        (events, send)
    }

    pub(crate) fn note_erg(&mut self, sent: Option<u16>) {
        match sent {
            Some(watts) => {
                self.erg_hold = true;
                self.erg_w = Some(watts);
            }
            None => {
                self.erg_hold = false;
                self.erg_w = None;
            }
        }
    }

    pub(crate) fn holding_erg(&self) -> bool {
        self.erg_hold
    }

    pub(crate) fn workout_view(&self) -> Option<WorkoutView> {
        self.run.as_ref().map(WorkoutRun::view)
    }

    pub(crate) fn workout_name(&self) -> Option<&str> {
        self.workout_name.as_deref()
    }

    pub(crate) fn workout_profile(&self) -> Vec<WorkoutBar> {
        self.profile.clone()
    }

    pub(crate) fn erg_w(&self) -> Option<u16> {
        self.erg_w
    }

    pub(crate) fn workout_result(&self) -> Option<WorkoutResult> {
        self.result.clone()
    }

    pub(crate) fn set_endpoints(&mut self, endpoints: Endpoints) {
        self.endpoints = endpoints;
        self.strava = None;
    }

    pub(crate) fn ensure_strava(&mut self, dir: &Path) {
        if self.strava.is_none() {
            self.open_strava(dir);
        }
    }

    pub(crate) fn strava_state(&self) -> StravaState {
        self.strava
            .as_ref()
            .map(StravaLink::state)
            .unwrap_or(StravaState::NoKeys)
    }

    pub(crate) fn begin_strava_connect(&mut self) -> Option<String> {
        self.strava.as_ref().and_then(StravaLink::begin_connect)
    }

    pub(crate) fn cancel_strava_connect(&mut self) {
        if let Some(link) = &self.strava {
            link.cancel_connect();
        }
    }

    pub(crate) fn disconnect_strava(&mut self) {
        if let Some(link) = &self.strava {
            link.disconnect();
        }
    }

    pub(crate) fn drain_strava(&mut self) -> Vec<RideEvent> {
        let Some(link) = &self.strava else {
            return Vec::new();
        };
        let mut events = Vec::new();
        for event in link.drain_events() {
            match event {
                StravaEvent::Connected { athlete } => {
                    events.push(RideEvent::StravaConnected { athlete });
                }
                StravaEvent::ConnectFailed { reason } => {
                    events.push(RideEvent::StravaConnectFailed { reason });
                }
                StravaEvent::UploadStarted { .. } => events.push(RideEvent::StravaUploadStarted),
                StravaEvent::Uploaded {
                    activity_id, url, ..
                } => {
                    self.uploaded = true;
                    events.push(RideEvent::StravaUploaded { activity_id, url });
                }
                StravaEvent::UploadFailed {
                    reason, duplicate, ..
                } => {
                    events.push(RideEvent::StravaUploadFailed { reason, duplicate });
                }
                StravaEvent::KeysSaved | StravaEvent::Disconnected => {}
            }
        }
        events
    }

    pub(crate) fn maybe_auto_upload(&self, path: &Path, enabled: bool, course_name: &str) {
        if !enabled {
            return;
        }
        self.upload_file(path, course_name, true);
    }

    pub(crate) fn upload_file(&self, path: &Path, course_name: &str, require_connected: bool) {
        let Some(link) = &self.strava else {
            return;
        };
        if require_connected && !matches!(link.state(), StravaState::Connected { .. }) {
            return;
        }
        if link.was_uploaded(path) {
            return;
        }
        link.upload(
            path.to_path_buf(),
            UploadMeta {
                name: format!("{course_name} — Penguin Pass"),
                description: format!("Ridden in Penguin Pass on {course_name}."),
            },
        );
    }

    pub(crate) fn strava_uploaded(&self) -> bool {
        self.uploaded
    }

    fn rebuild_peloton(&mut self, course: &Course, seed: u64, count: u8) {
        let count = count.min(24);
        self.peloton_count = count;
        self.peloton_seed = seed;
        self.peloton_len = course.length_m();
        self.peloton = if count == 0 {
            None
        } else {
            Some(Peloton::new(
                course,
                seed,
                BotConfig {
                    count,
                    ..BotConfig::default()
                },
            ))
        };
    }

    fn restart_run(&mut self, ftp_w: u16, workouts: &[Workout]) {
        let Some(id) = self.workout_id.clone() else {
            self.run = None;
            self.workout_name = None;
            self.erg = ErgGuide::default();
            self.erg_hold = false;
            self.erg_w = None;
            self.result = None;
            return;
        };
        let Some(workout) = workouts.iter().find(|workout| workout.id == id) else {
            self.clear_workout();
            return;
        };
        self.profile = bars(workout);
        self.start_run(workout, ftp_w);
    }

    fn clear_workout(&mut self) {
        self.workout_id = None;
        self.workout_name = None;
        self.profile.clear();
        self.run = None;
        self.erg = ErgGuide::default();
        self.erg_hold = false;
        self.erg_w = None;
        self.result = None;
    }

    fn start_run(&mut self, workout: &Workout, ftp_w: u16) {
        self.workout_name = Some(workout.name.clone());
        self.run = Some(WorkoutRun::new(workout, f32::from(ftp_w)));
        self.erg = ErgGuide::default();
        self.erg_hold = false;
        self.erg_w = None;
        self.result = None;
    }

    fn open_strava(&mut self, dir: &Path) {
        self.strava = Some(StravaLink::open(
            dir.join("strava.toml"),
            self.endpoints.clone(),
        ));
    }
}

fn load_workouts(dir: Option<&Path>) -> Vec<Workout> {
    let mut workouts = Library::builtin().workouts;
    if let Some(dir) = dir
        && let Ok(extra) = Library::load_dir(dir.join("workouts"))
    {
        workouts.extend(extra.workouts);
    }
    workouts
}

fn bars(workout: &Workout) -> Vec<WorkoutBar> {
    workout
        .profile()
        .into_iter()
        .map(|bar| WorkoutBar {
            duration_s: bar.duration_s,
            start_ftp: bar.start_power.unwrap_or(0.0),
            end_ftp: bar.end_power.unwrap_or(0.0),
            open: bar.open,
        })
        .collect()
}

fn saving_pct(factor: f32) -> u8 {
    if !factor.is_finite() {
        return 0;
    }
    let pct = ((1.0 - factor) * 100.0).round();
    if pct <= 0.0 {
        0
    } else if pct >= 40.0 {
        40
    } else {
        pct as u8
    }
}

fn rounded_ftp(watts: Option<f32>) -> Option<u16> {
    let watts = watts.filter(|watts| watts.is_finite() && *watts >= 0.0)?;
    let rounded = watts.round();
    if rounded >= f32::from(u16::MAX) {
        Some(u16::MAX)
    } else {
        Some(rounded as u16)
    }
}

fn seed_of(choice: &CourseChoice) -> u64 {
    match choice {
        CourseChoice::PenguinPass => 1,
        CourseChoice::LonelyRoad => 2,
        CourseChoice::Custom { id } => fnv1a(id),
        CourseChoice::Generated { seed, .. } => *seed,
    }
}

fn fnv1a(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use pp_routegen::TerrainPreset;

    use crate::{CourseChoice, RideEvent, Session, Settings};

    fn quiet() -> Settings {
        Settings {
            dashboard_enabled: false,
            ..Settings::default()
        }
    }

    fn flat() -> CourseChoice {
        CourseChoice::Generated {
            terrain: TerrainPreset::Flat,
            distance_km: 5,
            seed: 1,
        }
    }

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("pp-group-{}-{nanos}-{n}", std::process::id()));
            fs::create_dir_all(&path).expect("temp");
            Self(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn drafting_is_faster_than_an_empty_road_and_zero_bots_stay_empty() {
        let mut with_bots = Session::new(quiet());
        with_bots.set_course(flat()).expect("flat");
        with_bots.set_sim_power(220);
        let mut alone = quiet();
        alone.bots_count = 0;
        let mut alone = Session::new(alone);
        alone.set_course(flat()).expect("flat");
        alone.set_sim_power(220);

        let mut drafted = false;
        let mut faster = false;
        let mut closest = f64::MAX;
        for _ in 0..6_000 {
            let me = with_bots.telemetry().distance_m;
            if let Some(bot) = with_bots
                .play()
                .bots
                .iter()
                .filter(|bot| bot.total_distance_m > me)
                .min_by(|left, right| {
                    left.total_distance_m
                        .partial_cmp(&right.total_distance_m)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            {
                closest = closest.min(bot.total_distance_m - me);
                with_bots.set_lane_position(bot.slot);
            }
            with_bots.tick(0.05);
            alone.tick(0.05);
            assert!(alone.play().bots.is_empty());
            assert_eq!(alone.play().draft_saving_pct, 0);
            if with_bots.play().draft_saving_pct > 0 {
                drafted = true;
                if with_bots.telemetry().speed_mps > alone.telemetry().speed_mps + 0.05 {
                    faster = true;
                    break;
                }
            }
        }
        assert!(drafted, "never sat in a draft, closest gap {closest:.1} m");
        assert!(faster, "draft did not raise speed");
        assert!(!with_bots.play().bots.is_empty());
    }

    #[test]
    fn a_workout_sends_target_power_then_releases_to_grade() {
        let mut session = Session::new(quiet());
        session.select_workout(Some("endurance-20"));
        session.set_sim_power(160);
        session.tick(1.0);
        let (grade, target) = session.sim_grade_and_target();
        assert!(grade.is_none(), "grade {grade:?} while ERG holds");
        assert!(target.is_some(), "expected a target power");
        assert!(
            session
                .drain_events()
                .iter()
                .any(|event| { matches!(event, RideEvent::WorkoutStepStarted { .. }) })
        );
        let before = session
            .play()
            .workout
            .as_ref()
            .and_then(|view| view.target_w)
            .expect("target");
        session.set_workout_bias(25);
        let after = session
            .play()
            .workout
            .as_ref()
            .and_then(|view| view.target_w)
            .expect("biased target");
        assert!(after > before, "{after} vs {before}");

        session.set_sim_power(0);
        session.tick(1.2);
        let (grade, _) = session.sim_grade_and_target();
        assert!(grade.is_some(), "grade should resume after the rider stops");

        let dir = Temp::new();
        fs::create_dir_all(dir.0.join("workouts")).expect("workouts");
        fs::write(
            dir.0.join("workouts").join("open.zwo"),
            r#"<workout_file><name>Open</name><workout><FreeRide Duration="30"/></workout></workout_file>"#,
        )
        .expect("zwo");
        let mut open = Session::new(quiet());
        open.set_rides_dir(Some(dir.0.clone()));
        assert!(open.workouts().iter().any(|workout| workout.id == "open"));
        open.select_workout(Some("open"));
        open.set_sim_power(140);
        open.tick(1.0);
        let (grade, target) = open.sim_grade_and_target();
        assert!(target.is_none(), "free ride holds no target");
        assert!(grade.is_some(), "free ride sends grade");

        open.skip_workout_step();
        assert!(
            open.drain_events()
                .iter()
                .any(|event| { matches!(event, RideEvent::WorkoutFinished { .. }) })
        );
    }

    #[test]
    fn a_ramp_test_offers_an_ftp_and_accepting_it_saves_settings() {
        let dir = Temp::new();
        let mut session = Session::new(quiet());
        session.set_settings_path(dir.0.join("settings.toml"));
        session.select_workout(Some("ramp-test"));
        session.set_sim_power(200);
        let mut offered = None;
        for _ in 0..2_000 {
            session.tick(1.0);
            for event in session.drain_events() {
                if let RideEvent::WorkoutFinished { ramp_test_ftp_w } = event {
                    offered = ramp_test_ftp_w;
                }
            }
            if offered.is_some() {
                break;
            }
        }
        let ftp = offered.expect("ramp test ftp");
        assert_ne!(ftp, session.settings().ftp_w);
        session.accept_ramp_ftp(ftp);
        assert_eq!(session.settings().ftp_w, ftp);
        let text = fs::read_to_string(dir.0.join("settings.toml")).expect("saved settings");
        assert!(text.contains(&format!("ftp_w = {ftp}")), "{text}");
        assert!(session.summary().workout_result.is_some());
    }

    #[test]
    fn strava_uploads_a_saved_ride_and_stays_quiet_without_a_path() {
        let hits = Arc::new(AtomicU64::new(0));
        let seen = Arc::clone(&hits);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_worker = Arc::clone(&stop);
        let server = tiny_http::Server::http("127.0.0.1:0").expect("mock");
        let port = server.server_addr().to_ip().expect("mock ip").port();
        let base = format!("http://127.0.0.1:{port}");
        let worker = std::thread::spawn(move || {
            while !stop_worker.load(Ordering::SeqCst) {
                let request = match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(request)) => request,
                    Ok(None) => continue,
                    Err(_) => break,
                };
                seen.fetch_add(1, Ordering::SeqCst);
                let mut request = request;
                let mut body = Vec::new();
                let _ = std::io::Read::read_to_end(&mut request.as_reader(), &mut body);
                let response = tiny_http::Response::from_string(
                    r#"{"id":77,"activity_id":42,"status":"ready","error":null}"#,
                )
                .with_status_code(tiny_http::StatusCode(201));
                let _ = request.respond(response);
            }
        });

        let dir = Temp::new();
        fs::write(
            dir.0.join("strava.toml"),
            "\
client_id = \"pp-test\"
client_secret = \"pp-secret\"
access_token = \"pp-access\"
refresh_token = \"pp-refresh\"
expires_at = 4000000000
athlete = \"Ada Lovelace\"
",
        )
        .expect("store");
        let mut session = Session::new(quiet());
        session.set_strava_endpoints(pp_strava::Endpoints {
            oauth_base: base.clone(),
            api_base: base.clone(),
        });
        session.set_rides_dir(Some(dir.0.clone()));
        assert!(matches!(
            session.strava_state(),
            pp_strava::StravaState::Connected { .. }
        ));
        let url = session.begin_strava_connect().expect("authorize url");
        assert!(url.contains("127.0.0.1"), "{url}");
        session.cancel_strava_connect();

        session.set_sim_power(180);
        for _ in 0..70 {
            session.tick(1.0);
        }
        session.end_ride();
        let mut uploaded = false;
        for _ in 0..100 {
            session.tick(0.2);
            uploaded |= session.drain_events().iter().any(|event| {
                matches!(
                    event,
                    RideEvent::StravaUploaded {
                        activity_id: 42,
                        ..
                    }
                )
            });
            if uploaded {
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(
            uploaded,
            "upload events missing, hits {}",
            hits.load(Ordering::SeqCst)
        );
        assert!(session.summary().strava_uploaded);

        let quiet_dir = Temp::new();
        fs::write(
            quiet_dir.0.join("strava.toml"),
            "\
client_id = \"pp-test\"
client_secret = \"pp-secret\"
access_token = \"pp-access\"
refresh_token = \"pp-refresh\"
expires_at = 4000000000
athlete = \"Ada Lovelace\"
",
        )
        .expect("store");
        let mut off = quiet();
        off.strava_auto_upload = false;
        let mut off = Session::new(off);
        off.set_strava_endpoints(pp_strava::Endpoints {
            oauth_base: base.clone(),
            api_base: base,
        });
        off.set_rides_dir(Some(quiet_dir.0.clone()));
        let before = hits.load(Ordering::SeqCst);
        off.set_sim_power(180);
        for _ in 0..70 {
            off.tick(1.0);
        }
        off.end_ride();
        for _ in 0..10 {
            off.tick(0.2);
        }
        assert!(off.drain_events().iter().all(|event| {
            !matches!(
                event,
                RideEvent::StravaUploadStarted | RideEvent::StravaUploaded { .. }
            )
        }));
        assert_eq!(hits.load(Ordering::SeqCst), before);

        let mut loose = Session::new(quiet());
        loose.set_sim_power(180);
        for _ in 0..5 {
            loose.tick(1.0);
        }
        loose.end_ride();
        assert!(!std::path::Path::new("strava.toml").exists());
        drop(session);
        drop(off);
        stop.store(true, Ordering::SeqCst);
        let _ = worker.join();
    }

    #[test]
    fn lonely_road_spawns_no_bots_penguin_pass_keeps_bots() {
        let mut lonely = Session::new(Settings {
            bots_count: 8,
            dashboard_enabled: false,
            ..Settings::default()
        });
        lonely
            .set_course(CourseChoice::LonelyRoad)
            .expect("lonely road");
        lonely.set_sim_power(200);
        lonely.tick(1.0);
        assert!(lonely.play().bots.is_empty());

        let mut pass = Session::new(Settings {
            bots_count: 8,
            dashboard_enabled: false,
            ..Settings::default()
        });
        pass.set_course(CourseChoice::PenguinPass)
            .expect("penguin pass");
        pass.set_sim_power(200);
        pass.tick(1.0);
        assert!(!pass.play().bots.is_empty());
    }
}
