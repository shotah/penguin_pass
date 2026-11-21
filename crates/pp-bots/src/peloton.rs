//! The field of riders, ticked with the player.

use pp_course::{Course, MAX_SPEED_MPS, acceleration_mps2};

use crate::config::{BotConfig, Limits};
use crate::draft::draft;
use crate::lane::{advance_lanes, separate};
use crate::rider::{
    Rider, Roster, cadence_rpm, company_offset, make_company, make_pace, pedal_power,
    sanitize_mass, smooth_power, split_counts, standing, surge_of,
};
use crate::rng::Rng;
use crate::types::{BotEvent, BotState, PlayerInput};

/// Longest tick that is integrated, in seconds. A longer hitch is clamped.
const MAX_DT_S: f32 = 60.0;

/// Internal step, in seconds. Ticks are split so 30 Hz and 120 Hz agree.
const STEP_S: f32 = 1.0 / 120.0;

/// A pass is only announced when the two were this close, in metres, on
/// both sides of the crossing. A rider introduced far up the road does not
/// count as a pass.
const PASS_WINDOW_M: f64 = 60.0;

/// Seconds before the same pair can raise another pass event.
const PASS_COOLDOWN_S: f64 = 4.0;

/// The riders on the road for one ride.
///
/// Build one when the course loads. Call [`Self::tick`] once per frame with
/// the player's new odometer sample. [`Self::draft_factor`] is the number to
/// multiply the player's drag area by before the bike step. The same seed,
/// course, config, and player samples always produce the same riders.
#[derive(Debug)]
pub struct Peloton {
    course: Course,
    limits: Limits,
    rng: Rng,
    roster: Roster,
    elapsed_s: f64,
    ema_w: f32,
    ema_ready: bool,
    company_index: usize,
    riders: Vec<Rider>,
    published: Vec<BotState>,
    prev_player_m: Option<f64>,
}

impl Peloton {
    /// Riders spread along `course`. `seed` fixes their names, looks, and
    /// the small differences between them.
    ///
    /// The player is assumed to be on the start line. Company riders are
    /// placed a few tens of metres ahead and behind, already rolling. Pace
    /// riders are spread around the lap.
    #[must_use]
    pub fn new(course: &Course, seed: u64, config: BotConfig) -> Self {
        let limits = Limits::from_config(config);
        let mut rng = Rng::new(seed);
        let mut roster = Roster::new(&mut rng);
        let mut riders = Vec::new();
        let (pace_n, company_n) = split_counts(&limits);
        let length = course.length_m();
        for index in 0..pace_n {
            let taken = names_of(&riders);
            riders.push(make_pace(
                &mut roster,
                &mut rng,
                index,
                pace_n,
                length,
                &taken,
            ));
        }
        for index in 0..company_n {
            let taken = names_of(&riders);
            let distance = company_offset(index) + rng.range(-4.0, 4.0);
            riders.push(make_company(
                &mut roster,
                &mut rng,
                index,
                75.0,
                distance,
                None,
                &taken,
                None,
            ));
        }
        let mut peloton = Self {
            course: course.clone(),
            limits,
            rng,
            roster,
            elapsed_s: 0.0,
            ema_w: 0.0,
            ema_ready: false,
            company_index: company_n,
            riders,
            published: Vec::new(),
            prev_player_m: None,
        };
        for index in 0..peloton.riders.len() {
            let grade = grade_at(&peloton.course, peloton.riders[index].distance_m);
            let rider = &mut peloton.riders[index];
            if rider.pace_wkg.is_some() {
                rider.power_w = pedal_power(rider, 0.0, 0.0, grade, 0.0);
            } else {
                rider.power_w = (140.0 * rider.factor).clamp(80.0, 220.0);
            }
            rider.cadence_rpm =
                cadence_rpm(rider.power_w, rider.mass_kg, grade, rider.cadence_offset);
        }
        peloton.publish();
        peloton
    }

    /// Advance every rider by `dt_s` seconds.
    ///
    /// Zero, negative, and non-finite steps do nothing. A step longer than
    /// 60 s is clamped. The player's sample is where they are at the end of
    /// the step; inside the step their distance is walked back by
    /// `speed_mps * dt` so a long frame does not teleport them.
    ///
    /// The first call seeds the smoothed power with that sample, so the
    /// company is already near the player's opening effort. Later calls
    /// follow it with the configured time constant.
    pub fn tick(&mut self, dt_s: f32, player: &PlayerInput) -> Vec<BotEvent> {
        let dt = sanitize_dt(dt_s);
        if dt == 0.0 || self.riders.is_empty() {
            return Vec::new();
        }
        let player = sanitize_player(player);
        if !self.ema_ready {
            self.ema_w = player.power_w;
            self.ema_ready = true;
        }
        let n = substep_count(dt);
        let h = dt / n as f32;
        let speed = f64::from(player.speed_mps);
        let end = player.total_distance_m;
        let start = end - speed * f64::from(dt);
        for step in 0..n {
            let distance = start + speed * f64::from((step + 1) as f32 * h);
            let sample = PlayerInput {
                total_distance_m: distance,
                ..player
            };
            self.substep(h, &sample);
        }
        let mut events = self.pass_events(player.total_distance_m);
        events.extend(self.retire_and_spawn(&player));
        self.publish();
        events
    }

    /// Riders in id order.
    #[must_use]
    pub fn riders(&self) -> &[BotState] {
        &self.published
    }

    /// Multiply the player's drag area by this.
    ///
    /// 1 means nobody ahead is close enough, or in a close enough slot, to
    /// shelter the player. The same rule the bots use on each other.
    #[must_use]
    pub fn draft_factor(&self, player: &PlayerInput) -> f32 {
        let player = sanitize_player(player);
        let mut best = 1.0_f32;
        for rider in &self.riders {
            let gap = rider.distance_m - player.total_distance_m;
            let offset = (rider.slot - player.slot).abs();
            best = best.min(draft(gap, offset));
        }
        best
    }

    /// The bot a short way up the road, and the gap in metres.
    #[must_use]
    pub fn nearest_ahead(&self, player: &PlayerInput) -> Option<(&BotState, f64)> {
        nearest(self.riders(), player, true)
    }

    /// The bot a short way down the road, and the gap in metres (positive).
    #[must_use]
    pub fn nearest_behind(&self, player: &PlayerInput) -> Option<(&BotState, f64)> {
        nearest(self.riders(), player, false)
    }

    fn substep(&mut self, dt: f32, player: &PlayerInput) {
        self.elapsed_s += f64::from(dt);
        self.ema_w = smooth_power(self.ema_w, player.power_w, dt, self.limits.smooth_s);
        advance_lanes(&mut self.riders, player, dt);
        let mut powers = Vec::with_capacity(self.riders.len());
        let mut drafts = Vec::with_capacity(self.riders.len());
        for index in 0..self.riders.len() {
            let grade = grade_at(&self.course, self.riders[index].distance_m);
            powers.push(pedal_power(
                &self.riders[index],
                self.ema_w,
                self.elapsed_s,
                grade,
                player.total_distance_m,
            ));
            drafts.push(draft_for(index, &self.riders, player));
        }
        for index in 0..self.riders.len() {
            let grade = grade_at(&self.course, self.riders[index].distance_m);
            step_bike(
                &mut self.riders[index],
                dt,
                powers[index],
                drafts[index],
                grade,
            );
        }
        separate(&mut self.riders, player);
        for index in 0..self.riders.len() {
            let grade = grade_at(&self.course, self.riders[index].distance_m);
            let surge = surge_of(&self.riders[index], self.elapsed_s);
            let power = self.riders[index].power_w;
            let mass = self.riders[index].mass_kg;
            let offset = self.riders[index].cadence_offset;
            let phase = self.riders[index].stand_phase;
            self.riders[index].cadence_rpm = cadence_rpm(power, mass, grade, offset);
            self.riders[index].standing = standing(grade, surge, self.elapsed_s, phase);
        }
    }

    fn pass_events(&mut self, player_dist: f64) -> Vec<BotEvent> {
        let mut events = Vec::new();
        if let Some(prev_player) = self.prev_player_m {
            for rider in &mut self.riders {
                let before = prev_player - rider.prev_distance_m;
                let after = player_dist - rider.distance_m;
                let crossed = before * after < 0.0;
                let close = before.abs() < PASS_WINDOW_M && after.abs() < PASS_WINDOW_M;
                if crossed && close && self.elapsed_s >= rider.pass_ready_s {
                    if before < 0.0 {
                        events.push(BotEvent::Passed {
                            id: rider.id,
                            name: rider.name.clone(),
                        });
                    } else {
                        events.push(BotEvent::PassedBy {
                            id: rider.id,
                            name: rider.name.clone(),
                        });
                    }
                    rider.pass_ready_s = self.elapsed_s + PASS_COOLDOWN_S;
                }
            }
        }
        for rider in &mut self.riders {
            rider.prev_distance_m = rider.distance_m;
        }
        self.prev_player_m = Some(player_dist);
        events
    }

    fn retire_and_spawn(&mut self, player: &PlayerInput) -> Vec<BotEvent> {
        let mut events = Vec::new();
        let mut index = 0;
        while index < self.riders.len() {
            let company = self.riders[index].pace_wkg.is_none();
            let gap = (self.riders[index].distance_m - player.total_distance_m).abs();
            if company && gap > self.limits.retire_m {
                let was_ahead = self.riders[index].distance_m > player.total_distance_m;
                let id = self.riders[index].id;
                self.riders.remove(index);
                events.push(BotEvent::Left { id });
                if let Some(rider) = self.spawn_company(player, !was_ahead) {
                    events.push(BotEvent::Joined { id: rider.id });
                    self.riders.push(rider);
                }
            } else {
                index += 1;
            }
        }
        events
    }

    fn spawn_company(&mut self, player: &PlayerInput, ahead: bool) -> Option<Rider> {
        let mass = sanitize_mass(player.mass_kg);
        let factor = if ahead {
            self.rng.range(0.90, 0.98) as f32
        } else {
            self.rng.range(1.02, 1.12) as f32
        };
        let mut distance = player.total_distance_m;
        let mut slot = 1.0;
        let mut placed = false;
        for attempt in 0..6 {
            let extra = self.limits.spawn_m + self.rng.range(0.0, 50.0) + f64::from(attempt) * 25.0;
            distance = if ahead {
                player.total_distance_m + extra
            } else {
                player.total_distance_m - extra
            };
            slot = self.free_slot(distance);
            let blocked = self.riders.iter().any(|rider| {
                (rider.distance_m - distance).abs() < 10.0 && (rider.slot - slot).abs() < 0.6
            });
            if !blocked {
                placed = true;
                break;
            }
        }
        if !placed {
            distance = if ahead {
                player.total_distance_m + self.limits.retire_m - 20.0
            } else {
                player.total_distance_m - (self.limits.retire_m - 20.0)
            };
        }
        let taken = names_of(&self.riders);
        let index = self.company_index;
        self.company_index = self.company_index.saturating_add(1);
        let speed = if player.speed_mps > 1.0 {
            player.speed_mps
        } else {
            3.0
        };
        let mut rider = make_company(
            &mut self.roster,
            &mut self.rng,
            index,
            mass,
            distance,
            Some(speed),
            &taken,
            Some(factor),
        );
        rider.slot = slot;
        rider.target_slot = slot;
        rider.home_slot = slot;
        rider.prev_distance_m = distance;
        Some(rider)
    }

    fn free_slot(&self, distance_m: f64) -> f32 {
        for slot in [1.0_f32, 3.0, 4.0, 0.0, 2.0] {
            let blocked = self.riders.iter().any(|rider| {
                (rider.distance_m - distance_m).abs() < 10.0 && (rider.slot - slot).abs() < 0.6
            });
            if !blocked {
                return slot;
            }
        }
        4.0
    }

    fn publish(&mut self) {
        self.riders.sort_by_key(|rider| rider.id);
        self.published = self.riders.iter().map(snapshot).collect();
    }
}

fn snapshot(rider: &Rider) -> BotState {
    BotState {
        id: rider.id,
        name: rider.name.clone(),
        preset: rider.preset.clone(),
        kit: rider.kit.clone(),
        total_distance_m: rider.distance_m,
        speed_mps: rider.speed_mps,
        power_w: rider.power_w,
        cadence_rpm: rider.cadence_rpm,
        slot: rider.slot,
        standing: rider.standing,
        pace_wkg: rider.pace_wkg,
    }
}

fn names_of(riders: &[Rider]) -> Vec<String> {
    riders.iter().map(|rider| rider.name.clone()).collect()
}

fn nearest<'a>(
    riders: &'a [BotState],
    player: &PlayerInput,
    ahead: bool,
) -> Option<(&'a BotState, f64)> {
    let origin = if player.total_distance_m.is_finite() {
        player.total_distance_m
    } else {
        0.0
    };
    riders
        .iter()
        .filter_map(|bot| {
            let gap = bot.total_distance_m - origin;
            let keep = if ahead { gap > 0.0 } else { gap < 0.0 };
            keep.then_some((bot, gap.abs()))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
}

fn draft_for(index: usize, riders: &[Rider], player: &PlayerInput) -> f32 {
    let me = &riders[index];
    let mut best = 1.0_f32;
    for other in riders {
        if other.id == me.id {
            continue;
        }
        let gap = other.distance_m - me.distance_m;
        best = best.min(draft(gap, (other.slot - me.slot).abs()));
    }
    let gap = player.total_distance_m - me.distance_m;
    best.min(draft(gap, (player.slot - me.slot).abs()))
}

fn step_bike(rider: &mut Rider, dt: f32, power: f32, draft_cda: f32, grade: f32) {
    let mut profile = rider.profile;
    profile.cda_m2 *= draft_cda.max(0.0);
    let accel = acceleration_mps2(rider.speed_mps, power, grade, &profile);
    let mut speed = rider.speed_mps + accel * dt;
    if !speed.is_finite() {
        speed = rider.speed_mps.max(0.0);
    }
    speed = speed.clamp(0.0, MAX_SPEED_MPS);
    let distance = f64::from(speed) * f64::from(dt);
    rider.speed_mps = speed;
    if distance.is_finite() && distance > 0.0 {
        rider.distance_m += distance;
    }
    rider.power_w = power;
}

fn grade_at(course: &Course, distance_m: f64) -> f32 {
    if !distance_m.is_finite() || course.length_m() <= 0.0 {
        0.0
    } else {
        course.grade_pct_at(distance_m)
    }
}

fn sanitize_dt(dt_s: f32) -> f32 {
    if dt_s.is_finite() && dt_s > 0.0 {
        dt_s.min(MAX_DT_S)
    } else {
        0.0
    }
}

fn substep_count(dt: f32) -> u32 {
    let n = (f64::from(dt) / f64::from(STEP_S)).ceil();
    if n.is_finite() && n > 1.0 && n < 20_000.0 {
        n as u32
    } else {
        1
    }
}

fn sanitize_player(player: &PlayerInput) -> PlayerInput {
    PlayerInput {
        total_distance_m: if player.total_distance_m.is_finite() {
            player.total_distance_m
        } else {
            0.0
        },
        speed_mps: if player.speed_mps.is_finite() {
            player.speed_mps.max(0.0)
        } else {
            0.0
        },
        power_w: if player.power_w.is_finite() {
            player.power_w.max(0.0)
        } else {
            0.0
        },
        slot: if player.slot.is_finite() {
            player.slot.clamp(0.0, 4.0)
        } else {
            2.0
        },
        mass_kg: sanitize_mass(player.mass_kg),
    }
}
