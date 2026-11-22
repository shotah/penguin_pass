//! Slow full-ride peloton simulations (run via `cargo test --test long_rides`).

use std::str::FromStr;

use pp_bots::{BotConfig, BotEvent, MIN_SAME_SLOT_M, Peloton, PlayerInput, draft};
use pp_core::{RiderProfile, TrainerSample};
use pp_course::{Course, Ride};

fn flat_loop(length_m: f64) -> Course {
    let radius = length_m / std::f64::consts::TAU;
    let angle = std::f64::consts::TAU;
    let text = format!(
        "{{\"name\":\"Flat loop\",\"start\":{{\"x\":0.0,\"z\":0.0,\"heading_rad\":0.0}},\"segments\":[{{\"type\":\"arc\",\"radius_m\":{radius},\"angle_rad\":{angle}}}],\"grade\":[{{\"type\":\"hold\",\"length_m\":{length_m},\"grade_pct\":0.0}}]}}"
    );
    Course::from_str(&text).expect("flat loop")
}

fn overlaps(peloton: &Peloton, player: &PlayerInput) -> Option<String> {
    let riders = peloton.riders();
    for i in 0..riders.len() {
        for j in (i + 1)..riders.len() {
            let gap = (riders[i].total_distance_m - riders[j].total_distance_m).abs();
            let slot = (riders[i].slot - riders[j].slot).abs();
            if slot < 0.5 && gap < MIN_SAME_SLOT_M {
                return Some(format!(
                    "riders {} and {} are {gap:.2} m apart in slots {:.2} and {:.2}",
                    riders[i].id, riders[j].id, riders[i].slot, riders[j].slot
                ));
            }
        }
        let gap = (riders[i].total_distance_m - player.total_distance_m).abs();
        let slot = (riders[i].slot - player.slot).abs();
        if slot < 0.5 && gap < MIN_SAME_SLOT_M {
            return Some(format!(
                "rider {} is {gap:.2} m from the player in slot {:.2}",
                riders[i].id, riders[i].slot
            ));
        }
    }
    None
}

fn near_count(peloton: &Peloton, player: &PlayerInput) -> usize {
    peloton
        .riders()
        .iter()
        .filter(|rider| (rider.total_distance_m - player.total_distance_m).abs() <= 200.0)
        .count()
}

struct Share {
    course: &'static str,
    power_w: u16,
    fraction: f64,
    min_near: usize,
    max_near: usize,
}

fn player_from_ride(ride: &Ride, power_w: f32) -> PlayerInput {
    let frame = ride.telemetry();
    PlayerInput {
        total_distance_m: frame.distance_m,
        speed_mps: frame.speed_mps,
        power_w,
        slot: 2.0,
        mass_kg: 75.0,
    }
}

fn check_population(
    events: &[BotEvent],
    before: &[(u32, f64)],
    peloton: &Peloton,
    player: &PlayerInput,
    course: &str,
    power_w: u16,
) {
    for event in events {
        match event {
            BotEvent::Left { id } => {
                let Some((_, distance)) = before.iter().find(|(rider, _)| rider == id) else {
                    panic!("left id {id} was not on the road");
                };
                let gap = (distance - player.total_distance_m).abs();
                assert!(
                    gap >= 250.0,
                    "{course} {power_w} W rider {id} left {gap:.0} m away"
                );
            }
            BotEvent::Joined { id } => {
                let Some(rider) = peloton.riders().iter().find(|rider| rider.id == *id) else {
                    panic!("joined id {id} is missing");
                };
                let gap = (rider.total_distance_m - player.total_distance_m).abs();
                assert!(
                    gap >= 250.0,
                    "{course} {power_w} W rider {id} joined {gap:.0} m away"
                );
            }
            BotEvent::Passed { .. } | BotEvent::PassedBy { .. } => {}
        }
    }
    for rider in peloton.riders() {
        if before.iter().all(|(id, _)| *id != rider.id) {
            let gap = (rider.total_distance_m - player.total_distance_m).abs();
            assert!(
                gap >= 250.0,
                "{course} {power_w} W new id {} appeared {gap:.0} m away",
                rider.id
            );
        }
    }
    for (id, distance) in before {
        if peloton.riders().iter().all(|rider| rider.id != *id) {
            let gap = (distance - player.total_distance_m).abs();
            assert!(
                gap >= 250.0,
                "{course} {power_w} W id {id} vanished {gap:.0} m away"
            );
        }
    }
}

fn check_draft_matches(peloton: &Peloton, player: &PlayerInput) {
    let mut expect = 1.0_f32;
    for rider in peloton.riders() {
        let gap = rider.total_distance_m - player.total_distance_m;
        expect = expect.min(draft(gap, (rider.slot - player.slot).abs()));
    }
    let got = peloton.draft_factor(player);
    assert!((got - expect).abs() < 1.0e-5, "draft {got} vs {expect}");
}

fn ride_share(course: &Course, name: &'static str, power_w: u16, seed: u64) -> Share {
    let seconds = 20.0 * 60.0;
    let dt = 1.0 / 30.0;
    let steps = (seconds / dt) as i32;
    let mut ride = Ride::new(course.clone(), RiderProfile::default());
    let mut peloton = Peloton::new(course, seed, BotConfig::default());
    let base = RiderProfile::default();
    let mut in_band = 0_i32;
    let mut min_near = usize::MAX;
    let mut max_near = 0_usize;
    let mut pace_power: Vec<(u32, f32)> = Vec::new();
    for _ in 0..steps {
        let posed = player_from_ride(&ride, f32::from(power_w));
        let mut profile = base;
        profile.cda_m2 *= peloton.draft_factor(&posed);
        ride.set_rider(profile);
        let sample = TrainerSample {
            power_w: Some(power_w),
            ..TrainerSample::default()
        };
        let frame = ride.step(dt, &sample);
        let player = PlayerInput {
            total_distance_m: frame.distance_m,
            speed_mps: frame.speed_mps,
            power_w: f32::from(power_w),
            slot: 2.0,
            mass_kg: base.rider_mass_kg,
        };
        let before: Vec<(u32, f64)> = peloton
            .riders()
            .iter()
            .map(|rider| (rider.id, rider.total_distance_m))
            .collect();
        let events = peloton.tick(dt as f32, &player);
        if let Some(hit) = overlaps(&peloton, &player) {
            panic!("{name} {power_w} W overlap: {hit}");
        }
        check_population(&events, &before, &peloton, &player, name, power_w);
        check_draft_matches(&peloton, &player);
        for rider in peloton.riders() {
            if let Some(wkg) = rider.pace_wkg {
                match pace_power.iter().find(|(id, _)| *id == rider.id) {
                    Some((_, power)) => {
                        assert!(
                            (rider.power_w - power).abs() < 0.05,
                            "pace rider {} drifted from {power} W to {} W",
                            rider.id,
                            rider.power_w
                        );
                        let held = rider.power_w / wkg;
                        assert!(
                            (held - power / wkg).abs() < 0.05,
                            "pace rider {} left their watts per kilogram",
                            rider.id
                        );
                    }
                    None => pace_power.push((rider.id, rider.power_w)),
                }
            }
        }
        let near = near_count(&peloton, &player);
        min_near = min_near.min(near);
        max_near = max_near.max(near);
        if (2..=6).contains(&near) {
            in_band += 1;
        }
    }
    Share {
        course: name,
        power_w,
        fraction: f64::from(in_band) / f64::from(steps),
        min_near,
        max_near,
    }
}

#[test]
fn company_stays_nearby_on_the_long_rides() {
    let penguin = Course::penguin_pass();
    let flat = flat_loop(5_000.0);
    let mut shares = Vec::new();
    for power in [100_u16, 200, 350] {
        shares.push(ride_share(&penguin, "penguin", power, 21));
        shares.push(ride_share(&flat, "flat", power, 21));
    }
    for share in &shares {
        assert!(
            share.fraction >= 0.80,
            "{} at {} W was nearby {:.1}% of the time (near {}..{})",
            share.course,
            share.power_w,
            share.fraction * 100.0,
            share.min_near,
            share.max_near
        );
    }
}
