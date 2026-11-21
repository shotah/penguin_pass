//! One rider's identity and the watts they choose to hold.

use pp_core::RiderProfile;

use crate::config::Limits;
use crate::rng::Rng;

/// Watts per kilogram of body mass for the four pace riders, slowest first.
pub(crate) const PACE_WKG: [f32; 4] = [1.5, 2.2, 3.0, 3.8];

/// Personal scale on the player's smoothed watts. The default four company
/// riders use the first entries, which sit close to the player. Later
/// entries are for a larger field.
const COMPANY_FACTOR: [f32; 8] = [0.985, 0.995, 1.012, 1.03, 0.96, 1.06, 0.94, 1.08];

/// Body-mass offset from the reference rider, in kilograms.
const MASS_OFFSET_KG: [f32; 8] = [-1.0, 0.5, -0.5, 1.5, -3.0, 3.0, 0.5, -2.5];

/// Extra power on a climb, as a fraction at an 8% grade. Negative fades.
const CLIMB_BIAS: [f32; 8] = [0.03, -0.02, 0.04, -0.015, 0.04, -0.05, 0.03, -0.04];

const PRESETS: [&str; 2] = ["nils", "sigrid"];
const KITS: [&str; 4] = ["default", "kit-sunrise", "kit-glacier", "kit-kom-dots"];

const NAMES: [&str; 24] = [
    "Marta", "Jonas", "Hanna", "Leif", "Signe", "Otto", "Freya", "Astrid", "Edvin", "Ingrid",
    "Soren", "Linnea", "Kasper", "Thea", "Malin", "Viktor", "Elin", "Hugo", "Nora", "Lars", "Ida",
    "Emil", "Saga", "Peder",
];

/// Company power stays inside this multiple of the smoothed player watts,
/// before the climb adjustment.
pub(crate) const FACTOR_MIN: f32 = 0.85;
pub(crate) const FACTOR_MAX: f32 = 1.15;

/// Below this smoothed player power, company riders soft-pedal instead of
/// stopping.
const SOFT_EMA_W: f32 = 40.0;

/// Soft-pedal effort, in watts per kilogram of body mass.
const SOFT_WPKG: f32 = 1.2;

/// Gap, in metres, inside which company riders do not ease or push to close.
const NUDGE_DEAD_M: f64 = 100.0;

/// Gap, in metres, at which the ease or push is fully on.
const NUDGE_FULL_M: f64 = 240.0;

/// Reference body mass when the player sample is missing, in kilograms.
const REFERENCE_MASS_KG: f32 = 75.0;

/// A rider the peloton integrates. Not part of the public snapshot.
#[derive(Debug, Clone)]
pub(crate) struct Rider {
    pub id: u32,
    pub name: String,
    pub preset: String,
    pub kit: String,
    pub mass_kg: f32,
    pub profile: RiderProfile,
    pub distance_m: f64,
    pub speed_mps: f32,
    pub power_w: f32,
    pub cadence_rpm: f32,
    pub slot: f32,
    pub target_slot: f32,
    pub home_slot: f32,
    pub standing: bool,
    pub pace_wkg: Option<f32>,
    pub factor: f32,
    pub amp: f32,
    pub period_s: f64,
    pub phase: f64,
    pub climb_bias: f32,
    pub cadence_offset: f32,
    pub stand_phase: f64,
    pub prev_distance_m: f64,
    pub pass_ready_s: f64,
}

#[derive(Debug)]
pub(crate) struct Roster {
    pub next_id: u32,
    pub name_cursor: usize,
}

impl Roster {
    pub fn new(rng: &mut Rng) -> Self {
        Self {
            next_id: 1,
            name_cursor: usize::try_from(rng.index(u32::try_from(NAMES.len()).unwrap_or(1)))
                .unwrap_or(0),
        }
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    fn alloc_name(&mut self, taken: &[String]) -> String {
        for _ in 0..NAMES.len() {
            let name = NAMES[self.name_cursor % NAMES.len()];
            self.name_cursor = self.name_cursor.wrapping_add(1);
            if !taken.iter().any(|have| have == name) {
                return name.to_string();
            }
        }
        format!("Rider {}", self.next_id)
    }
}

pub(crate) fn make_pace(
    roster: &mut Roster,
    rng: &mut Rng,
    index: usize,
    pace_n: usize,
    length_m: f64,
    taken: &[String],
) -> Rider {
    let wkg = PACE_WKG[index % PACE_WKG.len()];
    let masses = [66.0, 72.0, 78.0, 70.0];
    let mass = masses[index % masses.len()];
    let n = pace_n.max(1);
    let frac = (index as f64 + 0.5) / n as f64;
    let mut distance = frac * length_m.max(1.0);
    if distance > length_m * 0.5 {
        distance -= length_m;
    }
    distance += rng.range(-12.0, 12.0);
    // Keep the opening group free of a pace rider sitting on the line.
    if distance.abs() < 250.0 && length_m > 800.0 {
        distance = distance.signum() * 250.0 + distance.signum() * rng.range(0.0, 40.0);
        if distance == 0.0 {
            distance = 280.0;
        }
    }
    let home = 3.0;
    let mut rider = blank(
        roster,
        rng,
        taken,
        mass,
        home,
        distance,
        Some(wkg),
        1.0,
        0.0,
    );
    rider.power_w = wkg * mass;
    rider
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn make_company(
    roster: &mut Roster,
    rng: &mut Rng,
    index: usize,
    center_mass_kg: f32,
    distance_m: f64,
    speed_mps: Option<f32>,
    taken: &[String],
    factor: Option<f32>,
) -> Rider {
    let slot_cycle = [3.0_f32, 4.0, 1.0, 3.0, 2.0, 4.0, 0.0, 1.0];
    let home = slot_cycle[index % slot_cycle.len()];
    let mass = (center_mass_kg + MASS_OFFSET_KG[index % MASS_OFFSET_KG.len()]).clamp(55.0, 100.0);
    let factor = factor.unwrap_or(COMPANY_FACTOR[index % COMPANY_FACTOR.len()]);
    let mut rider = blank(
        roster,
        rng,
        taken,
        mass,
        home,
        distance_m,
        None,
        factor,
        CLIMB_BIAS[index % CLIMB_BIAS.len()],
    );
    if let Some(speed) = speed_mps {
        rider.speed_mps = speed.clamp(0.0, 20.0);
    }
    rider
}

/// Opening offsets for company riders, in metres from the start line.
/// A few just ahead, a few just behind, none stacked.
pub(crate) fn company_offset(index: usize) -> f64 {
    const OFFSETS: [f64; 8] = [42.0, 78.0, -32.0, -70.0, 118.0, -112.0, 156.0, -148.0];
    OFFSETS[index % OFFSETS.len()]
}

#[allow(clippy::too_many_arguments)]
fn blank(
    roster: &mut Roster,
    rng: &mut Rng,
    taken: &[String],
    mass: f32,
    home: f32,
    distance_m: f64,
    pace_wkg: Option<f32>,
    factor: f32,
    climb_bias: f32,
) -> Rider {
    let id = roster.alloc_id();
    let name = roster.alloc_name(taken);
    let preset = PRESETS[usize::try_from(rng.index(PRESETS.len() as u32)).unwrap_or(0)].to_string();
    let kit = KITS[usize::try_from(rng.index(KITS.len() as u32)).unwrap_or(0)].to_string();
    let period = 55.0 + f64::from(id % 5) * 14.0;
    let speed = 1.4 + rng.range(0.0, 0.7) as f32;
    Rider {
        id,
        name,
        preset,
        kit,
        mass_kg: mass,
        profile: profile_for(mass),
        distance_m,
        speed_mps: speed,
        power_w: 0.0,
        cadence_rpm: 80.0,
        slot: home,
        target_slot: home,
        home_slot: home,
        standing: false,
        pace_wkg,
        factor,
        amp: 0.028,
        period_s: period,
        phase: rng.range(0.0, std::f64::consts::TAU),
        climb_bias,
        cadence_offset: rng.range(-4.0, 4.0) as f32,
        stand_phase: rng.range(0.0, 32.0),
        prev_distance_m: distance_m,
        pass_ready_s: 0.0,
    }
}

fn profile_for(mass_kg: f32) -> RiderProfile {
    let mass = mass_kg.clamp(40.0, 120.0);
    RiderProfile {
        rider_mass_kg: mass,
        bike_mass_kg: 9.0,
        cda_m2: (0.32 + 0.0012 * (mass - REFERENCE_MASS_KG)).clamp(0.27, 0.40),
        crr: 0.004,
        ftp_w: 200,
    }
}

pub(crate) fn sanitize_mass(mass_kg: f32) -> f32 {
    if mass_kg.is_finite() && (30.0..=200.0).contains(&mass_kg) {
        mass_kg
    } else {
        REFERENCE_MASS_KG
    }
}

/// Cadence from power and grade. Steep road drops it.
pub(crate) fn cadence_rpm(power_w: f32, mass_kg: f32, grade_pct: f32, offset: f32) -> f32 {
    let mass = mass_kg.max(40.0);
    let wkg = if power_w.is_finite() {
        (power_w.max(0.0) / mass).clamp(0.0, 8.0)
    } else {
        0.0
    };
    let grade = if grade_pct.is_finite() {
        grade_pct.clamp(0.0, 12.0)
    } else {
        0.0
    };
    let base = 76.0 + wkg * 4.0 + offset;
    (base - grade * 1.7).clamp(55.0, 110.0)
}

/// A few seconds out of the saddle on a steep grade, or while the rider surges.
pub(crate) fn standing(grade_pct: f32, surge: f32, elapsed_s: f64, phase_s: f64) -> bool {
    if !elapsed_s.is_finite() {
        return false;
    }
    let cycle = (elapsed_s + phase_s).rem_euclid(32.0);
    let burst = cycle < 5.5;
    let grade = if grade_pct.is_finite() {
        grade_pct
    } else {
        0.0
    };
    let steep = grade >= 6.0;
    let surging = surge >= 0.55 && grade >= 2.0;
    burst && (steep || surging)
}

pub(crate) fn variation(elapsed_s: f64, period_s: f64, phase: f64, amp: f32) -> (f32, f32) {
    let period = if period_s.is_finite() && period_s > 1.0 {
        period_s
    } else {
        60.0
    };
    let angle = elapsed_s / period * std::f64::consts::TAU + phase;
    let sine = angle.sin() as f32;
    (1.0 + amp * sine, sine)
}

/// Scale applied when a company rider has drifted away from the player.
///
/// Inside 100 m this is 1. Farther ahead they ease, farther behind they
/// push, up to about 10%. It is not a speed lock.
pub(crate) fn gap_nudge(gap_ahead_m: f64) -> f32 {
    if !gap_ahead_m.is_finite() {
        return 1.0;
    }
    let mag = gap_ahead_m.abs();
    if mag <= NUDGE_DEAD_M {
        return 1.0;
    }
    let t = ((mag - NUDGE_DEAD_M) / (NUDGE_FULL_M - NUDGE_DEAD_M)).clamp(0.0, 1.0) as f32;
    let delta = 0.10 * t;
    if gap_ahead_m > 0.0 {
        1.0 - delta
    } else {
        1.0 + delta
    }
}

pub(crate) fn pedal_power(
    rider: &Rider,
    ema_w: f32,
    elapsed_s: f64,
    grade_pct: f32,
    player_distance_m: f64,
) -> f32 {
    if let Some(wkg) = rider.pace_wkg {
        return (wkg * rider.mass_kg).clamp(0.0, 900.0);
    }
    let (scale, _) = variation(elapsed_s, rider.period_s, rider.phase, rider.amp);
    let gap = rider.distance_m - player_distance_m;
    let mut effort = rider.factor * scale * gap_nudge(gap);
    effort = effort.clamp(FACTOR_MIN, FACTOR_MAX);
    let grade = if grade_pct.is_finite() {
        grade_pct
    } else {
        0.0
    };
    let climb = 1.0 + rider.climb_bias * (grade / 8.0).clamp(-0.5, 1.0);
    let ema = if ema_w.is_finite() {
        ema_w.max(0.0)
    } else {
        0.0
    };
    let mut power = ema * effort * climb;
    if ema < SOFT_EMA_W {
        power = power.max(SOFT_WPKG * rider.mass_kg);
    }
    if !power.is_finite() || power < 0.0 {
        0.0
    } else {
        power.min(900.0)
    }
}

pub(crate) fn surge_of(rider: &Rider, elapsed_s: f64) -> f32 {
    variation(elapsed_s, rider.period_s, rider.phase, rider.amp).1
}

/// How the field is split. Pace riders are at most four. The rest are company.
pub(crate) fn split_counts(limits: &Limits) -> (usize, usize) {
    let count = usize::from(limits.count);
    if count == 0 {
        return (0, 0);
    }
    if !limits.pace_riders {
        return (0, count);
    }
    let pace = count.min(PACE_WKG.len());
    (pace, count - pace)
}

pub(crate) fn smooth_power(ema: f32, sample: f32, dt_s: f32, tau_s: f32) -> f32 {
    let sample = if sample.is_finite() {
        sample.max(0.0)
    } else {
        0.0
    };
    let tau = if tau_s.is_finite() {
        tau_s.max(0.05)
    } else {
        15.0
    };
    let dt = if dt_s.is_finite() { dt_s.max(0.0) } else { 0.0 };
    let gain = 1.0 - (-dt / tau).exp();
    let next = ema + (sample - ema) * gain;
    if next.is_finite() { next } else { sample }
}
