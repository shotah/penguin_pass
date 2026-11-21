//! Segments, the ghost, fish, powerups, and the badge profile.
//!
//! `pp-segments` has no assisted flag on an effort. A segment or a first lap
//! ridden while a powerup effect was active is still timed for the HUD, and it
//! is not stored as a best or as the ghost.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pp_arcade::{
    AwardReason, Badge, FishField, FishRun, LocalDate, PhysicsModifiers, PowerupEvent, PowerupKind,
    PowerupSlot, Profile, RideOutcome,
};
use pp_core::Telemetry;
use pp_course::Course;
use pp_segments::{
    GhostRecorder, GhostTrace, Records, Segment, SegmentEvent, SegmentKind, SegmentTimer, detect,
};

use crate::course_sel::CourseChoice;
use crate::events::RideEvent;
use crate::stats::SegmentResult;

/// Fish drawn when they are at most this far ahead of the rider, in meters.
const FISH_AHEAD_M: f64 = 150.0;

/// How many lane slots the fish field uses.
const FISH_SLOTS: u8 = 5;

/// One lap of segments, for the banners.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentInfo {
    /// Stable id.
    pub id: String,
    /// Banner name.
    pub name: String,
    /// Climb, sprint, or the whole lap.
    pub kind: SegmentKind,
    /// Start line, in meters along the lap.
    pub start_m: f64,
    /// Finish line, in meters along the lap. A sprint may wrap past the start.
    pub end_m: f64,
    /// Stored best, in seconds. `None` when the rider has no best yet.
    pub best_s: Option<f64>,
}

/// The segment the rider is in.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveSegment {
    /// Stable id.
    pub id: String,
    /// Banner name.
    pub name: String,
    /// Seconds since the start line.
    pub elapsed_s: f64,
    /// `0.0` at the start line and `1.0` at the finish.
    pub fraction: f32,
    /// Seconds ahead of the stored best. Positive means this attempt is ahead.
    pub split_s: Option<f64>,
}

/// The stored ghost, on the same distance scale as the ride.
///
/// `gap_m` and `gap_s` are positive when the rider is ahead of the ghost.
#[derive(Debug, Clone, PartialEq)]
pub struct GhostView {
    /// Ghost distance from the start of the ride, in meters.
    pub total_distance_m: f64,
    /// Rider distance minus the ghost's distance, in meters.
    pub gap_m: f64,
    /// Seconds the rider is ahead of the ghost.
    pub gap_s: f64,
}

/// A fish still ahead of the rider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FishView {
    /// Index into this course's fish. The same fish has the same index every lap.
    pub index: u32,
    /// Distance from the start of the ride to the fish, in meters.
    pub total_distance_m: f64,
    /// Lane slot, `0..=4`.
    pub slot: u8,
    /// The fish is golden.
    pub golden: bool,
}

/// A powerup that is currently running.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActivePowerup {
    /// Which effect.
    pub kind: PowerupKind,
    /// Seconds left.
    pub remaining_s: f32,
    /// Seconds the effect runs in total.
    pub duration_s: f32,
}

/// What the game draws each frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayView {
    /// One lap's segments, for the banners.
    pub segments: Vec<SegmentInfo>,
    /// The open climb or sprint, or the lap when that is the only one open.
    pub active_segment: Option<ActiveSegment>,
    /// The stored ghost. `None` when ghost display is off or no ghost is stored.
    pub ghost: Option<GhostView>,
    /// Fish within about 150 m ahead.
    pub fish_ahead: Vec<FishView>,
    /// Fish collected on this ride.
    pub fish_count: u32,
    /// Golden fish collected on this ride.
    pub golden_count: u32,
    /// Consecutive collects since the last miss.
    pub combo: u32,
    /// The powerup waiting to be used.
    pub powerup_held: Option<PowerupKind>,
    /// The powerup that is running.
    pub powerup_active: Option<ActivePowerup>,
    /// Bots on the road. Empty when bots are off.
    pub bots: Vec<pp_bots::BotState>,
    /// Drag saved by drafting, from 0 to 40.
    pub draft_saving_pct: u8,
    /// The workout step on screen. `None` on a free ride.
    pub workout: Option<pp_workout::WorkoutView>,
    /// Bars for the workout graph. Empty on a free ride.
    pub workout_profile: Vec<WorkoutBar>,
    /// Last ERG target sent to the trainer, while ERG is holding one.
    pub workout_erg_w: Option<u16>,
}

/// One workout in the picker.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkoutSummary {
    /// Stable id.
    pub id: String,
    /// Name shown in the list.
    pub name: String,
    /// One or two sentences.
    pub description: String,
    /// Planned length, in seconds.
    pub duration_s: f32,
    /// The plan is the ramp test.
    pub ramp_test: bool,
    /// Flattened bars for the preview graph.
    pub profile: Vec<WorkoutBar>,
}

/// One bar of a workout graph.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkoutBar {
    /// Width of the bar, in seconds.
    pub duration_s: f32,
    /// Fraction of FTP at the left edge.
    pub start_ftp: f32,
    /// Fraction of FTP at the right edge.
    pub end_ftp: f32,
    /// Free ride or max effort. The trainer does not hold a target.
    pub open: bool,
}

/// One badge on the profile screen.
#[derive(Debug, Clone, PartialEq)]
pub struct BadgeView {
    /// Stable id.
    pub id: String,
    /// Short name.
    pub name: String,
    /// One line.
    pub description: String,
    /// The rider has earned it.
    pub earned: bool,
    /// Progress toward the badge, in the badge's own unit.
    pub current: f64,
    /// The value that earns the badge.
    pub target: f64,
    /// Kit unlocked when the badge is earned.
    pub kit: Option<String>,
}

/// Lifetime totals and badges.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileView {
    /// Finished rides of at least a minute.
    pub rides: u32,
    /// Total distance of counted rides, in meters.
    pub distance_m: f64,
    /// Total ascent of counted rides, in meters.
    pub ascent_m: f64,
    /// Total time of counted rides, in seconds.
    pub time_s: f64,
    /// Fish collected on counted rides.
    pub fish: u64,
    /// Consecutive local days with a ride of at least ten minutes.
    pub streak_days: u32,
    /// Every badge, earned or not.
    pub badges: Vec<BadgeView>,
    /// Kits unlocked by earned badges.
    pub unlocked_kits: Vec<String>,
}

struct CourseRun {
    key: String,
    badge_id: String,
    length_m: f64,
    penguin: bool,
    segments: Vec<Segment>,
    timer: SegmentTimer,
    ghost: GhostRecorder,
    stored: Option<GhostTrace>,
    fish: FishRun,
    power: PowerupSlot,
    prev_distance_m: f64,
    tainted: HashSet<String>,
    ghost_dirty: bool,
    results: Vec<SegmentResult>,
    record_fell: bool,
    opening: Vec<SegmentEvent>,
}

pub(crate) struct Play {
    records: Records,
    profile: Profile,
    dir: Option<PathBuf>,
    lane: f32,
    closed: bool,
    assisted: bool,
    run: Option<CourseRun>,
}

impl Play {
    pub(crate) fn new() -> Self {
        Self {
            records: Records::default(),
            profile: Profile::default(),
            dir: None,
            lane: 2.0,
            closed: false,
            assisted: false,
            run: None,
        }
    }

    pub(crate) fn has_dir(&self) -> bool {
        self.dir.is_some()
    }

    pub(crate) fn attach(
        &mut self,
        dir: PathBuf,
        course: &Course,
        choice: &CourseChoice,
        arcade: bool,
    ) {
        if self.dir.as_ref() == Some(&dir) {
            return;
        }
        self.records = Records::load(dir.join("records.toml"));
        self.profile = Profile::load(dir.join("arcade.toml"));
        self.dir = Some(dir);
        if self.run.is_some() {
            self.open(course, choice, arcade);
        }
    }

    pub(crate) fn open(&mut self, course: &Course, choice: &CourseChoice, arcade: bool) {
        self.closed = false;
        self.assisted = false;
        let length_m = course.length_m();
        let key = records_key(choice);
        let segments = detect(course);
        let efforts = self.records.efforts(&key, length_m);
        let stored = self.records.ghost(&key, length_m);
        let mut timer = SegmentTimer::new(segments.clone(), &efforts);
        let mut ghost = GhostRecorder::new(length_m, stored.clone());
        let opening = timer.tick(0.0, 0.0);
        let _ = ghost.tick(0.0, 0.0);
        let seed = layout_seed(choice);
        let slots = if arcade { FISH_SLOTS } else { 0 };
        let field = FishField::generate(
            length_m,
            |distance| course.grade_pct_at(distance),
            seed,
            slots,
        );
        self.run = Some(CourseRun {
            key,
            badge_id: badge_course(choice),
            length_m,
            penguin: matches!(choice, CourseChoice::PenguinPass),
            segments,
            timer,
            ghost,
            stored,
            fish: FishRun::new(&field),
            power: PowerupSlot::new(seed),
            prev_distance_m: 0.0,
            tainted: HashSet::new(),
            ghost_dirty: false,
            results: Vec::new(),
            record_fell: false,
            opening,
        });
    }

    pub(crate) fn set_lane(&mut self, slot: f32) {
        if slot.is_finite() {
            self.lane = slot.clamp(0.0, 4.0);
        }
    }

    pub(crate) fn lane(&self) -> f32 {
        self.lane
    }

    pub(crate) fn modifiers(&self) -> PhysicsModifiers {
        self.run
            .as_ref()
            .map(|run| run.power.modifiers())
            .unwrap_or(PhysicsModifiers::IDENTITY)
    }

    pub(crate) fn set_assisted(&mut self, assisted: bool) {
        self.assisted = assisted;
        if assisted && let Some(run) = &mut self.run {
            run.ghost_dirty = true;
        }
    }

    pub(crate) fn activate(&mut self) -> Option<PowerupKind> {
        let run = self.run.as_mut()?;
        match run.power.activate() {
            Some(PowerupEvent::Activated { kind }) => Some(kind),
            _ => None,
        }
    }

    pub(crate) fn after_step(
        &mut self,
        frame: &Telemetry,
        dt: f64,
        arcade: bool,
    ) -> Vec<RideEvent> {
        if self.closed || self.run.is_none() {
            return Vec::new();
        }
        let assisted = self.assisted;
        self.assisted = false;
        let mut events = Vec::new();
        self.expire_powerup(dt, arcade, &mut events);
        if arcade {
            self.tick_fish(frame, &mut events);
        }
        self.tick_segments(frame, assisted, arcade, &mut events);
        self.tick_ghost(frame, &mut events);
        if let Some(run) = &mut self.run {
            run.prev_distance_m = frame.distance_m;
        }
        events
    }

    pub(crate) fn finish(
        &mut self,
        frame: &Telemetry,
        ascent_m: f32,
        offset_s: i32,
    ) -> Vec<RideEvent> {
        if self.closed {
            return Vec::new();
        }
        self.closed = true;
        self.write_records();
        let outcome = self.outcome(frame, ascent_m);
        let earned = self.profile.apply_ride(&outcome, local_date(offset_s));
        self.write_profile();
        earned
            .into_iter()
            .filter_map(|id| {
                let badge = Badge::get(id.as_str())?;
                Some(RideEvent::BadgeEarned {
                    name: badge.name.to_owned(),
                    kit: badge.kit.map(str::to_owned),
                })
            })
            .collect()
    }

    pub(crate) fn view(&self, arcade: bool, ghost: bool, frame: &Telemetry) -> PlayView {
        let Some(run) = &self.run else {
            return empty_view();
        };
        let efforts = run.timer.efforts();
        let segments = run
            .segments
            .iter()
            .map(|segment| SegmentInfo {
                id: segment.id.clone(),
                name: segment.name.clone(),
                kind: segment.kind,
                start_m: segment.start_m,
                end_m: segment.end_m,
                best_s: efforts
                    .iter()
                    .find(|effort| effort.segment_id == segment.id)
                    .map(|effort| effort.time_s),
            })
            .collect();
        PlayView {
            segments,
            active_segment: active_segment(run),
            ghost: if ghost { ghost_view(run, frame) } else { None },
            fish_ahead: if arcade {
                fish_ahead(run, frame.distance_m)
            } else {
                Vec::new()
            },
            fish_count: if arcade { run.fish.count() } else { 0 },
            golden_count: if arcade { run.fish.golden_count() } else { 0 },
            combo: if arcade { run.fish.combo() } else { 0 },
            powerup_held: if arcade { run.power.held() } else { None },
            powerup_active: if arcade { active_powerup(run) } else { None },
            bots: Vec::new(),
            draft_saving_pct: 0,
            workout: None,
            workout_profile: Vec::new(),
            workout_erg_w: None,
        }
    }

    pub(crate) fn profile_view(&self) -> ProfileView {
        let badges = Badge::catalog()
            .iter()
            .map(|badge| {
                let (current, target) = self.profile.progress(badge);
                BadgeView {
                    id: badge.id.to_owned(),
                    name: badge.name.to_owned(),
                    description: badge.description.to_owned(),
                    earned: self.profile.has_badge(badge.id),
                    current,
                    target,
                    kit: badge.kit.map(str::to_owned),
                }
            })
            .collect();
        ProfileView {
            rides: self.profile.rides(),
            distance_m: self.profile.distance_m(),
            ascent_m: self.profile.ascent_m(),
            time_s: self.profile.time_s(),
            fish: self.profile.fish(),
            streak_days: self.profile.streak_days(),
            badges,
            unlocked_kits: self.profile.kits().into_iter().map(str::to_owned).collect(),
        }
    }

    pub(crate) fn summary_bits(&self) -> (u32, u32, Vec<SegmentResult>) {
        let Some(run) = &self.run else {
            return (0, 0, Vec::new());
        };
        (run.fish.count(), run.fish.best_combo(), run.results.clone())
    }

    fn expire_powerup(&mut self, dt: f64, arcade: bool, events: &mut Vec<RideEvent>) {
        if !arcade {
            return;
        }
        let Some(run) = &mut self.run else {
            return;
        };
        let dt_s = if dt.is_finite() && dt > 0.0 {
            dt as f32
        } else {
            0.0
        };
        if let Some(PowerupEvent::Expired { kind }) = run.power.tick(dt_s) {
            events.push(RideEvent::PowerupExpired { kind });
        }
    }

    fn tick_fish(&mut self, frame: &Telemetry, events: &mut Vec<RideEvent>) {
        let grade = frame.grade_pct;
        let Some(happened) = self.run.as_mut().map(|run| {
            run.fish
                .tick(run.prev_distance_m, frame.distance_m, self.lane)
        }) else {
            return;
        };
        for event in happened {
            match event {
                pp_arcade::FishEvent::Collected { golden, combo, .. } => {
                    events.push(RideEvent::FishCollected { golden, combo });
                    if golden {
                        self.award(AwardReason::GoldenFish, grade, events);
                    }
                    if combo > 0 && combo % 10 == 0 {
                        self.award(AwardReason::Combo, grade, events);
                    }
                }
                pp_arcade::FishEvent::Missed { .. } => events.push(RideEvent::FishMissed),
            }
        }
    }

    fn tick_segments(
        &mut self,
        frame: &Telemetry,
        assisted: bool,
        arcade: bool,
        events: &mut Vec<RideEvent>,
    ) {
        let Some(run) = &mut self.run else {
            return;
        };
        let mut raw = std::mem::take(&mut run.opening);
        raw.extend(run.timer.tick(frame.elapsed_s, frame.distance_m));
        if assisted {
            for event in &raw {
                if let SegmentEvent::Finished { segment_id, .. } = event {
                    run.tainted.insert(segment_id.clone());
                }
            }
            for progress in run.timer.in_progress() {
                run.tainted.insert(progress.segment_id.clone());
            }
        }
        let grade = frame.grade_pct;
        let mut awards = Vec::new();
        let mut save_now = false;
        for event in raw {
            match event {
                SegmentEvent::Entered { segment_id } => {
                    events.push(RideEvent::SegmentEntered {
                        name: segment_name(run, &segment_id),
                    });
                }
                SegmentEvent::Finished {
                    segment_id,
                    time_s,
                    previous_best_s,
                    record,
                } => {
                    let stored = record && !run.tainted.contains(&segment_id);
                    if stored {
                        run.record_fell = true;
                        save_now = true;
                    }
                    let name = segment_name(run, &segment_id);
                    run.results.push(SegmentResult {
                        name: name.clone(),
                        time_s,
                        record: stored,
                    });
                    events.push(RideEvent::SegmentFinished {
                        name,
                        time_s,
                        previous_best_s,
                        record: stored,
                    });
                    match segment_kind(run, &segment_id) {
                        Some(SegmentKind::Climb | SegmentKind::Sprint) => {
                            awards.push(AwardReason::Segment);
                        }
                        Some(SegmentKind::Lap) => awards.push(AwardReason::Lap),
                        None => {}
                    }
                }
            }
        }
        if arcade {
            for reason in awards {
                self.award(reason, grade, events);
            }
        }
        if save_now {
            self.write_records();
        }
    }

    fn tick_ghost(&mut self, frame: &Telemetry, events: &mut Vec<RideEvent>) {
        let save = {
            let Some(run) = &mut self.run else {
                return;
            };
            let Some(lap) = run.ghost.tick(frame.elapsed_s, frame.distance_m) else {
                return;
            };
            let save = lap.record && !run.ghost_dirty;
            if lap.record && lap.previous_s.is_some() && !run.ghost_dirty {
                events.push(RideEvent::GhostBeaten {
                    time_s: lap.time_s,
                    previous_s: lap.previous_s,
                });
            }
            save
        };
        if save {
            self.write_records();
        }
    }

    fn award(&mut self, reason: AwardReason, grade: f32, events: &mut Vec<RideEvent>) {
        let Some(run) = &mut self.run else {
            return;
        };
        if let Some(PowerupEvent::Awarded { kind, .. }) = run.power.award(reason, grade) {
            events.push(RideEvent::PowerupAwarded { kind });
        }
    }

    fn outcome(&self, frame: &Telemetry, ascent_m: f32) -> RideOutcome {
        let Some(run) = &self.run else {
            return RideOutcome {
                duration_s: frame.elapsed_s,
                distance_m: frame.distance_m,
                ascent_m: f64::from(ascent_m),
                fish: 0,
                golden_fish: 0,
                best_combo: 0,
                course_id: "penguin_pass".to_owned(),
                penguin_pass_laps: 0,
                segment_record: false,
            };
        };
        RideOutcome {
            duration_s: frame.elapsed_s,
            distance_m: frame.distance_m,
            ascent_m: f64::from(ascent_m),
            fish: u64::from(run.fish.count()),
            golden_fish: u64::from(run.fish.golden_count()),
            best_combo: run.fish.best_combo(),
            course_id: run.badge_id.clone(),
            penguin_pass_laps: if run.penguin {
                frame.lap.saturating_sub(1)
            } else {
                0
            },
            segment_record: run.record_fell,
        }
    }

    fn write_records(&mut self) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let packed = self.run.as_ref().map(|run| {
            let efforts = run
                .timer
                .efforts()
                .into_iter()
                .filter(|effort| !run.tainted.contains(&effort.segment_id))
                .collect::<Vec<_>>();
            let ghost = if run.ghost_dirty {
                None
            } else {
                run.ghost.trace().cloned()
            };
            (run.key.clone(), run.length_m, efforts, ghost)
        });
        if let Some((key, length_m, efforts, ghost)) = packed {
            for effort in efforts {
                let _ = self.records.set_effort(&key, length_m, effort);
            }
            if let Some(trace) = ghost {
                let _ = self.records.set_ghost(&key, length_m, trace);
            }
        }
        if let Err(err) = self.records.save(dir.join("records.toml")) {
            eprintln!("pp-session: records not saved: {err}");
        }
    }

    fn write_profile(&self) {
        let Some(dir) = &self.dir else {
            return;
        };
        if let Err(err) = self.profile.save(dir.join("arcade.toml")) {
            eprintln!("pp-session: profile not saved: {err}");
        }
    }
}

fn empty_view() -> PlayView {
    PlayView {
        segments: Vec::new(),
        active_segment: None,
        ghost: None,
        fish_ahead: Vec::new(),
        fish_count: 0,
        golden_count: 0,
        combo: 0,
        powerup_held: None,
        powerup_active: None,
        bots: Vec::new(),
        draft_saving_pct: 0,
        workout: None,
        workout_profile: Vec::new(),
        workout_erg_w: None,
    }
}

fn active_segment(run: &CourseRun) -> Option<ActiveSegment> {
    let progress = run.timer.in_progress();
    let chosen = progress
        .iter()
        .find(|item| segment_kind(run, &item.segment_id) != Some(SegmentKind::Lap))
        .or_else(|| progress.first())?;
    Some(ActiveSegment {
        id: chosen.segment_id.clone(),
        name: segment_name(run, &chosen.segment_id),
        elapsed_s: chosen.elapsed_s,
        fraction: chosen.fraction as f32,
        split_s: chosen.split_s,
    })
}

fn active_powerup(run: &CourseRun) -> Option<ActivePowerup> {
    let kind = run.power.active()?;
    Some(ActivePowerup {
        kind,
        remaining_s: run.power.remaining_s(),
        duration_s: kind.duration_s(),
    })
}

fn ghost_view(run: &CourseRun, frame: &Telemetry) -> Option<GhostView> {
    let trace = run.stored.as_ref()?;
    let gap = trace.gap(frame.elapsed_s, frame.distance_m)?;
    Some(GhostView {
        total_distance_m: trace.distance_at(frame.elapsed_s)?,
        gap_m: gap.meters,
        gap_s: gap.seconds,
    })
}

fn fish_ahead(run: &CourseRun, distance_m: f64) -> Vec<FishView> {
    run.fish
        .ahead(distance_m, FISH_AHEAD_M)
        .into_iter()
        .map(|fish| FishView {
            index: u32::try_from(fish.index).unwrap_or(u32::MAX),
            total_distance_m: distance_m + fish.ahead_m,
            slot: fish.slot,
            golden: fish.golden,
        })
        .collect()
}

fn segment_name(run: &CourseRun, id: &str) -> String {
    run.segments
        .iter()
        .find(|segment| segment.id == id)
        .map(|segment| segment.name.clone())
        .unwrap_or_else(|| id.to_owned())
}

fn segment_kind(run: &CourseRun, id: &str) -> Option<SegmentKind> {
    run.segments
        .iter()
        .find(|segment| segment.id == id)
        .map(|segment| segment.kind)
}

fn records_key(choice: &CourseChoice) -> String {
    match choice {
        CourseChoice::PenguinPass => "penguin_pass".to_owned(),
        CourseChoice::Generated {
            terrain,
            distance_km,
            seed,
        } => format!("{}-{distance_km}-{seed}", terrain.as_str()),
    }
}

fn badge_course(choice: &CourseChoice) -> String {
    match choice {
        CourseChoice::PenguinPass => "penguin_pass".to_owned(),
        CourseChoice::Generated { terrain, .. } => terrain.as_str().to_owned(),
    }
}

fn layout_seed(choice: &CourseChoice) -> u64 {
    match choice {
        CourseChoice::PenguinPass => 1,
        CourseChoice::Generated { seed, .. } => *seed,
    }
}

fn local_date(offset_s: i32) -> LocalDate {
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let local = i64::try_from(unix)
        .unwrap_or(i64::MAX)
        .saturating_add(i64::from(offset_s))
        .max(0);
    let (year, month, day) = civil_ymd(local / 86_400);
    LocalDate::new(year, month, day)
        .unwrap_or_else(|| LocalDate::new(1970, 1, 1).expect("1970-01-01 is a real day"))
}

/// Howard Hinnant's civil calendar, days after 1970-01-01.
fn civil_ymd(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year as i32, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use pp_routegen::TerrainPreset;

    use super::*;
    use crate::{CourseChoice, PowerupKind, RideEvent, Session, Settings};

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pp-play-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            fs::create_dir_all(&path).expect("temp");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn quiet() -> Settings {
        Settings {
            tv_dashboard_enabled: false,
            ..Settings::default()
        }
    }

    fn flat(seed: u64) -> CourseChoice {
        CourseChoice::Generated {
            terrain: TerrainPreset::Flat,
            distance_km: 2,
            seed,
        }
    }

    fn mountain(seed: u64) -> CourseChoice {
        CourseChoice::Generated {
            terrain: TerrainPreset::Mountain,
            distance_km: 5,
            seed,
        }
    }

    #[test]
    fn epoch_is_the_civil_origin() {
        assert_eq!(civil_ymd(0), (1970, 1, 1));
        let _ = local_date(0);
    }

    #[test]
    fn the_penguin_pass_climb_starts_near_273_m_and_a_faster_ride_keeps_the_record() {
        let dir = Temp::new();
        let mut slow = Session::new(quiet());
        slow.set_rides_dir(Some(dir.path().to_path_buf()));
        slow.set_sim_power(500);
        let entered = ride_for_event(
            &mut slow,
            200.0,
            |event| matches!(event, RideEvent::SegmentEntered { name } if name == "Penguin Pass climb"),
        );
        assert!(
            (273.0..320.0).contains(&entered),
            "climb entry at {entered}"
        );

        let slow_time = finish_named(&mut slow, "Penguin Pass climb", 900.0);
        slow.end_ride();

        let mut fast = Session::new(quiet());
        fast.set_rides_dir(Some(dir.path().to_path_buf()));
        fast.set_sim_power(1_200);
        let (fast_time, previous, record) = finish_record(&mut fast, "Penguin Pass climb", 900.0);
        assert!(record, "faster ride should set the record");
        let previous = previous.expect("the slow ride stored a best");
        assert!(
            (previous - slow_time).abs() < 0.05,
            "stored {previous}, slow {slow_time}"
        );
        assert!(fast_time < previous, "{fast_time} vs {previous}");
        fast.end_ride();

        let mut again = Session::new(quiet());
        again.set_rides_dir(Some(dir.path().to_path_buf()));
        let best = again
            .play()
            .segments
            .into_iter()
            .find(|segment| segment.name == "Penguin Pass climb")
            .and_then(|segment| segment.best_s);
        let best = best.expect("record persisted");
        assert!((best - fast_time).abs() < 0.05, "{best} vs {fast_time}");
    }

    #[test]
    fn the_second_ride_shows_a_ghost_ahead_when_the_rider_is_faster() {
        let dir = Temp::new();
        let mut first = Session::new(quiet());
        first.set_rides_dir(Some(dir.path().to_path_buf()));
        first.set_course(flat(4)).expect("flat");
        first.set_sim_power(180);
        complete_lap(&mut first, 0.25, 500.0);
        assert!(first.play().ghost.is_none(), "no ghost on the first ride");
        first.end_ride();

        let mut second = Session::new(quiet());
        second.set_rides_dir(Some(dir.path().to_path_buf()));
        second.set_course(flat(4)).expect("flat");
        second.set_sim_power(800);
        let mut saw = None;
        let mut elapsed = 0.0;
        while elapsed < 40.0 {
            second.tick(0.25);
            elapsed += 0.25;
            if let Some(ghost) = second.play().ghost {
                let ahead = ghost.gap_m > 1.0;
                saw = Some(ghost);
                if ahead {
                    break;
                }
            }
        }
        let ghost = saw.expect("ghost on the second ride");
        assert!(
            ghost.gap_m > 0.0 && ghost.gap_s > 0.0,
            "rider ahead should be positive: {ghost:?}"
        );
        assert!(ghost.total_distance_m > 0.0);
    }

    #[test]
    fn fish_collect_in_their_slot_and_return_next_lap() {
        let mut session = Session::new(quiet());
        session.set_course(flat(2)).expect("flat");
        session.set_sim_power(700);
        let length = session.course_facts().length_m;
        let fish = nearest_fish(&session).expect("a fish ahead");
        let wrong = if fish.slot == 0 { 4.0 } else { 0.0 };
        let events = cross_fish(&mut session, wrong, fish.total_distance_m);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RideEvent::FishMissed)),
            "wrong slot should miss: {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, RideEvent::FishCollected { .. })),
            "wrong slot collected: {events:?}"
        );

        let mut returned = None;
        let mut guard = 0.0;
        while guard < 600.0 {
            session.tick(0.2);
            guard += 0.2;
            if let Some(next) = session
                .play()
                .fish_ahead
                .into_iter()
                .find(|ahead| ahead.index == fish.index && ahead.total_distance_m > length)
            {
                returned = Some(next);
                break;
            }
        }
        let next = returned.expect("the fish comes back on the next lap");
        let count_before = session.play().fish_count;
        let events = cross_fish(&mut session, f32::from(next.slot), next.total_distance_m);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RideEvent::FishCollected { .. })),
            "matching slot should collect: {events:?}"
        );
        assert!(session.play().fish_count > count_before);
    }

    #[test]
    fn ten_fish_in_a_row_awards_a_powerup() {
        let mut session = Session::new(quiet());
        session.set_course(flat(6)).expect("flat");
        session.set_sim_power(650);
        let mut awarded_at = None;
        let mut elapsed = 0.0;
        while elapsed < 400.0 {
            if let Some(fish) = nearest_fish(&session) {
                let away = fish.total_distance_m - session.telemetry().distance_m;
                if away < 30.0 {
                    session.set_lane_position(f32::from(fish.slot));
                }
            }
            if session.play().powerup_held.is_some() && awarded_at.is_none() {
                session.activate_powerup();
            }
            session.tick(0.1);
            elapsed += 0.1;
            for event in session.drain_events() {
                if let RideEvent::FishCollected { combo, .. } = event
                    && combo > 0
                    && combo % 10 == 0
                {
                    awarded_at = Some(combo);
                }
                if matches!(event, RideEvent::PowerupAwarded { .. }) && awarded_at.is_some() {
                    return;
                }
            }
            if awarded_at.is_some() && session.play().powerup_held.is_some() {
                return;
            }
        }
        panic!("no powerup at a combo of ten, last combo {awarded_at:?}");
    }

    #[test]
    fn an_active_feather_is_faster_on_the_climb_and_identical_before_it() {
        let seed = (1..24)
            .find(|seed| holds_feather(*seed))
            .expect("a feather seed");
        let mut plain = open_mountain(seed);
        let mut feather = open_mountain(seed);
        assert!(ride_to_feather(&mut plain, 400.0));
        assert!(ride_to_feather(&mut feather, 400.0));
        let before_plain = plain.telemetry().distance_m;
        let before_feather = feather.telemetry().distance_m;
        assert!(
            (before_plain - before_feather).abs() < 1.0e-3,
            "{before_plain} vs {before_feather}"
        );
        assert_eq!(feather.play().powerup_held, Some(PowerupKind::Feather));
        feather.activate_powerup();
        assert_eq!(
            feather.play().powerup_active.map(|active| active.kind),
            Some(PowerupKind::Feather)
        );

        let mut climbed = false;
        for _ in 0..80 {
            plain.tick(0.25);
            feather.tick(0.25);
            if feather.telemetry().grade_pct >= 4.0 {
                climbed = true;
            }
        }
        assert!(climbed, "the comparison never reached the climb");
        assert!(
            feather.telemetry().distance_m > plain.telemetry().distance_m + 0.5,
            "feather {} plain {}",
            feather.telemetry().distance_m,
            plain.telemetry().distance_m
        );
        assert_eq!(feather.readout().power_w, plain.readout().power_w);
    }

    #[test]
    fn arcade_off_hides_fish_and_ghost_off_hides_the_ghost_but_still_records() {
        let mut settings = quiet();
        settings.arcade = false;
        let mut session = Session::new(settings);
        session.set_course(flat(3)).expect("flat");
        session.set_sim_power(600);
        let mut saw_segment = false;
        let mut elapsed = 0.0;
        while elapsed < 90.0 {
            session.tick(0.25);
            elapsed += 0.25;
            let view = session.play();
            assert!(view.fish_ahead.is_empty());
            assert_eq!(view.fish_count, 0);
            assert_eq!(view.combo, 0);
            assert!(view.powerup_held.is_none());
            assert!(view.powerup_active.is_none());
            for event in session.drain_events() {
                assert!(
                    !matches!(
                        event,
                        RideEvent::FishCollected { .. }
                            | RideEvent::FishMissed
                            | RideEvent::PowerupAwarded { .. }
                            | RideEvent::PowerupActivated { .. }
                            | RideEvent::PowerupExpired { .. }
                    ),
                    "{event:?}"
                );
                if matches!(event, RideEvent::SegmentEntered { .. }) {
                    saw_segment = true;
                }
            }
        }
        assert!(saw_segment, "segments still run with arcade off");

        let dir = Temp::new();
        let mut recorded = Session::new(quiet());
        recorded.set_rides_dir(Some(dir.path().to_path_buf()));
        recorded.set_course(flat(8)).expect("flat");
        recorded.set_sim_power(160);
        complete_lap(&mut recorded, 0.25, 500.0);
        recorded.end_ride();
        let before = fs::read_to_string(dir.path().join("records.toml")).expect("records");

        let mut hidden = quiet();
        hidden.ghost = false;
        let mut second = Session::new(hidden);
        second.set_rides_dir(Some(dir.path().to_path_buf()));
        second.set_course(flat(8)).expect("flat");
        second.set_sim_power(800);
        let mut elapsed = 0.0;
        while elapsed < 20.0 {
            second.tick(0.25);
            elapsed += 0.25;
            assert!(second.play().ghost.is_none());
        }
        complete_lap(&mut second, 0.25, 400.0);
        second.end_ride();
        let after = fs::read_to_string(dir.path().join("records.toml")).expect("records");
        assert_ne!(before, after, "a hidden ghost is still recorded");
    }

    #[test]
    fn a_badge_is_earned_once_and_nothing_is_written_without_a_path() {
        let dir = Temp::new();
        let mut session = Session::new(quiet());
        session.set_rides_dir(Some(dir.path().to_path_buf()));
        session.set_sim_power(200);
        for _ in 0..70 {
            session.tick(1.0);
        }
        session.end_ride();
        let earned: Vec<_> = session
            .drain_events()
            .into_iter()
            .filter_map(|event| match event {
                RideEvent::BadgeEarned { name, kit } => Some((name, kit)),
                _ => None,
            })
            .collect();
        assert!(
            earned.iter().any(|(name, _)| name == "First splash"),
            "{earned:?}"
        );
        assert!(dir.path().join("arcade.toml").is_file());
        assert_eq!(session.profile().rides, 1);

        let mut again = Session::new(quiet());
        again.set_rides_dir(Some(dir.path().to_path_buf()));
        again.set_sim_power(200);
        for _ in 0..70 {
            again.tick(1.0);
        }
        again.end_ride();
        let repeat = again.drain_events().into_iter().any(
            |event| matches!(event, RideEvent::BadgeEarned { name, .. } if name == "First splash"),
        );
        assert!(!repeat, "the same badge fired twice");
        assert_eq!(again.profile().rides, 2);

        let mut loose = Session::new(quiet());
        loose.set_sim_power(200);
        for _ in 0..5 {
            loose.tick(1.0);
        }
        loose.end_ride();
        assert!(!Path::new("records.toml").exists());
        assert!(!Path::new("arcade.toml").exists());
        assert!(!dir.path().join("not-used").exists());
    }

    #[test]
    fn corrupt_record_files_do_not_stop_the_ride() {
        let dir = Temp::new();
        fs::write(dir.path().join("records.toml"), b"records = [[[").expect("write");
        fs::write(dir.path().join("arcade.toml"), b"profile = [").expect("write");
        let mut session = Session::new(quiet());
        session.set_rides_dir(Some(dir.path().to_path_buf()));
        session.set_sim_power(250);
        let frame = session.tick(0.5);
        assert!(frame.elapsed_s > 0.0);
        assert!(!session.play().segments.is_empty());
        session.end_ride();
        assert!(session.ride_ended());
    }

    #[test]
    fn the_summary_lists_fish_combo_and_segment_results() {
        let mut session = Session::new(quiet());
        session.set_course(flat(9)).expect("flat");
        session.set_sim_power(700);
        let fish = nearest_fish(&session).expect("fish");
        let _ = cross_fish(&mut session, f32::from(fish.slot), fish.total_distance_m);
        let summary = session.summary();
        assert!(summary.fish >= 1, "fish {}", summary.fish);
        assert!(summary.best_combo >= 1);
    }

    fn holds_feather(seed: u64) -> bool {
        let mut session = open_mountain(seed);
        ride_to_feather(&mut session, 250.0)
    }

    fn open_mountain(seed: u64) -> Session {
        let mut session = Session::new(quiet());
        session.set_course(mountain(seed)).expect("mountain");
        session.set_sim_power(600);
        session
    }

    fn ride_to_feather(session: &mut Session, max_s: f64) -> bool {
        let mut elapsed = 0.0;
        while elapsed < max_s {
            steer_on_climb(session);
            session.tick(0.15);
            elapsed += 0.15;
            match session.play().powerup_held {
                Some(PowerupKind::Feather) => return true,
                Some(_) => return false,
                None => {}
            }
        }
        false
    }

    fn steer_on_climb(session: &mut Session) {
        let grade = session.telemetry().grade_pct;
        if grade >= 4.0
            && let Some(fish) = nearest_fish(session)
        {
            let away = fish.total_distance_m - session.telemetry().distance_m;
            if away < 25.0 {
                session.set_lane_position(f32::from(fish.slot));
                return;
            }
        }
        session.set_lane_position(0.0);
    }

    fn nearest_fish(session: &Session) -> Option<FishView> {
        session.play().fish_ahead.into_iter().next()
    }

    fn cross_fish(session: &mut Session, slot: f32, target_m: f64) -> Vec<RideEvent> {
        let mut events = Vec::new();
        let mut guard = 0.0;
        while session.telemetry().distance_m + 3.0 < target_m && guard < 300.0 {
            session.tick(0.15);
            guard += 0.15;
            events.extend(session.drain_events());
        }
        events.clear();
        session.set_lane_position(slot);
        while session.telemetry().distance_m < target_m + 2.0 && guard < 320.0 {
            session.set_lane_position(slot);
            session.tick(0.1);
            guard += 0.1;
            events.extend(session.drain_events());
        }
        events
    }

    fn ride_for_event(session: &mut Session, max_s: f64, pred: impl Fn(&RideEvent) -> bool) -> f64 {
        let mut elapsed = 0.0;
        while elapsed < max_s {
            session.tick(0.2);
            elapsed += 0.2;
            if session.drain_events().iter().any(&pred) {
                return session.telemetry().distance_m;
            }
        }
        panic!("event not seen by {:.0} m", session.telemetry().distance_m);
    }

    fn finish_named(session: &mut Session, name: &str, max_s: f64) -> f64 {
        let (time_s, _, _) = finish_record(session, name, max_s);
        time_s
    }

    fn finish_record(session: &mut Session, name: &str, max_s: f64) -> (f64, Option<f64>, bool) {
        let mut elapsed = 0.0;
        while elapsed < max_s {
            session.tick(0.5);
            elapsed += 0.5;
            for event in session.drain_events() {
                if let RideEvent::SegmentFinished {
                    name: finished,
                    time_s,
                    previous_best_s,
                    record,
                } = event
                    && finished == name
                {
                    return (time_s, previous_best_s, record);
                }
            }
        }
        panic!(
            "{name} not finished by {:.0} m",
            session.telemetry().distance_m
        );
    }

    fn complete_lap(session: &mut Session, dt: f64, max_s: f64) {
        let length = session.course_facts().length_m;
        let mut elapsed = 0.0;
        while session.telemetry().distance_m < length && elapsed < max_s {
            session.tick(dt);
            elapsed += dt;
            let _ = session.drain_events();
        }
        assert!(
            session.telemetry().distance_m >= length,
            "stopped at {}",
            session.telemetry().distance_m
        );
    }
}
