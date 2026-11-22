//! Rides long enough to see whether anyone is still nearby.

use std::str::FromStr;

use pp_course::Course;

use crate::lane::MIN_SAME_SLOT_M;
use crate::rider::{cadence_rpm, gap_nudge, standing};
use crate::{BotConfig, BotEvent, Peloton, PlayerInput};

fn flat_loop(length_m: f64) -> Course {
    let radius = length_m / std::f64::consts::TAU;
    let angle = std::f64::consts::TAU;
    let text = format!(
        "{{\"name\":\"Flat loop\",\"start\":{{\"x\":0.0,\"z\":0.0,\"heading_rad\":0.0}},\"segments\":[{{\"type\":\"arc\",\"radius_m\":{radius},\"angle_rad\":{angle}}}],\"grade\":[{{\"type\":\"hold\",\"length_m\":{length_m},\"grade_pct\":0.0}}]}}"
    );
    Course::from_str(&text).expect("flat loop")
}

fn player_at(distance_m: f64, speed_mps: f32, power_w: f32) -> PlayerInput {
    PlayerInput {
        total_distance_m: distance_m,
        speed_mps,
        power_w,
        slot: 2.0,
        mass_kg: 75.0,
    }
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

#[test]
fn riders_start_spread_and_rolling() {
    let peloton = Peloton::new(&Course::penguin_pass(), 3, BotConfig::default());
    let riders = peloton.riders();
    assert_eq!(riders.len(), 8);
    let mut ahead = false;
    let mut behind = false;
    for rider in riders {
        assert!(rider.speed_mps > 0.8, "{} is not rolling", rider.name);
        assert!(!rider.name.is_empty());
        assert!(rider.preset == "nils" || rider.preset == "sigrid");
        assert!(matches!(
            rider.kit.as_str(),
            "default" | "kit-sunrise" | "kit-glacier" | "kit-kom-dots"
        ));
        assert!(
            (0.0..=7.0).contains(&rider.slot),
            "{} sat outside the paved slots at {:.2}",
            rider.name,
            rider.slot
        );
        if rider.total_distance_m > 20.0 {
            ahead = true;
        }
        if rider.total_distance_m < -20.0 {
            behind = true;
        }
    }
    assert!(ahead && behind, "the start should have riders both ways");
    let player = player_at(0.0, 0.0, 0.0);
    assert!(overlaps(&peloton, &player).is_none());
    let paces: Vec<f32> = riders.iter().filter_map(|rider| rider.pace_wkg).collect();
    assert_eq!(paces.len(), 4);
    for expected in [1.5, 2.2, 3.0, 3.8] {
        assert!(
            paces.iter().any(|wkg| (wkg - expected).abs() < 1.0e-4),
            "missing {expected} W/kg"
        );
    }
    let near = near_count(&peloton, &player);
    assert!(
        (2..=6).contains(&near),
        "start has {near} riders within 200 m"
    );
}

#[test]
fn bots_leave_the_shoulder_clear_when_the_player_rides_there() {
    let course = flat_loop(5_000.0);
    let mut peloton = Peloton::new(&course, 5, BotConfig::default());
    let dt = 1.0 / 30.0;
    let speed = 8.0_f32;
    for step in 0..(40.0 / dt) as i32 {
        let t = dt * (step + 1) as f32;
        let mut player = player_at(f64::from(speed) * f64::from(t), speed, 220.0);
        player.slot = 8.0;
        peloton.tick(dt, &player);
        for rider in peloton.riders() {
            assert!(
                (0.0..=7.0).contains(&rider.slot),
                "rider {} sat on the shoulder at {:.2} while the player was in slot 8",
                rider.id,
                rider.slot
            );
        }
    }
}

#[test]
fn zero_count_is_an_empty_road() {
    let course = Course::penguin_pass();
    let mut peloton = Peloton::new(
        &course,
        1,
        BotConfig {
            count: 0,
            ..BotConfig::default()
        },
    );
    let player = player_at(0.0, 0.0, 200.0);
    assert!(peloton.riders().is_empty());
    assert!(peloton.tick(1.0 / 60.0, &player).is_empty());
    assert_eq!(peloton.draft_factor(&player), 1.0);
    assert!(peloton.nearest_ahead(&player).is_none());
    assert!(peloton.nearest_behind(&player).is_none());
    assert!(peloton.tick(0.0, &player).is_empty());
    assert!(peloton.tick(f32::NAN, &player).is_empty());
}

#[test]
fn the_same_seed_repeats_and_another_seed_does_not() {
    let course = flat_loop(5_000.0);
    let once = scripted(&course, 9, 1.0 / 60.0, 30.0);
    let twice = scripted(&course, 9, 1.0 / 60.0, 30.0);
    assert_eq!(once, twice);
    let other = scripted(&course, 10, 1.0 / 60.0, 30.0);
    assert_ne!(once, other);
}

#[test]
fn frame_rate_changes_little() {
    let course = flat_loop(5_000.0);
    let fine = scripted(&course, 11, 1.0 / 120.0, 90.0);
    let coarse = scripted(&course, 11, 1.0 / 30.0, 90.0);
    assert_eq!(
        fine.len(),
        coarse.len(),
        "a rider was retired on only one step"
    );
    for (left, right) in fine.iter().zip(coarse.iter()) {
        assert_eq!(left.id, right.id);
        let gap = (left.total_distance_m - right.total_distance_m).abs();
        assert!(
            gap < 8.0,
            "rider {} moved {gap:.2} m between 120 Hz and 30 Hz",
            left.id
        );
        assert!(
            (left.slot - right.slot).abs() < 0.2,
            "rider {} slot {:.3} vs {:.3}",
            left.id,
            left.slot,
            right.slot
        );
    }
}

fn scripted(course: &Course, seed: u64, dt: f32, seconds: f32) -> Vec<crate::BotState> {
    let mut peloton = Peloton::new(course, seed, BotConfig::default());
    let speed = 8.0_f32;
    let steps = (seconds / dt).round() as i32;
    for step in 0..steps {
        let t = dt * (step + 1) as f32;
        let player = player_at(f64::from(speed) * f64::from(t), speed, 200.0);
        peloton.tick(dt, &player);
    }
    peloton.riders().to_vec()
}

#[test]
fn a_stopped_player_does_not_stop_the_company() {
    let course = flat_loop(8_000.0);
    let mut peloton = Peloton::new(
        &course,
        4,
        BotConfig {
            count: 6,
            pace_riders: false,
            ..BotConfig::default()
        },
    );
    let player = player_at(0.0, 0.0, 0.0);
    let dt = 1.0 / 30.0;
    let mut peak = 0.0_f32;
    for _ in 0..(120.0 / dt) as i32 {
        peloton.tick(dt, &player);
        for rider in peloton.riders() {
            peak = peak.max(rider.speed_mps);
        }
        assert!(
            overlaps(&peloton, &player).is_none(),
            "{}",
            overlaps(&peloton, &player).unwrap_or_default()
        );
    }
    assert!(peak > 4.0, "company peak speed was {peak} m/s");
    let parked = peloton
        .riders()
        .iter()
        .all(|rider| rider.speed_mps < 0.4 && rider.total_distance_m.abs() < 20.0);
    assert!(!parked, "company sat on the stopped player");
}

#[test]
fn passing_moves_toward_the_centre_and_back_out() {
    let course = flat_loop(800.0);
    let mut peloton = Peloton::new(
        &course,
        2,
        BotConfig {
            count: 2,
            pace_riders: true,
            ..BotConfig::default()
        },
    );
    let fast_id = peloton
        .riders()
        .iter()
        .max_by(|left, right| {
            left.pace_wkg
                .unwrap_or(0.0)
                .total_cmp(&right.pace_wkg.unwrap_or(0.0))
        })
        .expect("fast pace rider")
        .id;
    let slow_id = peloton
        .riders()
        .iter()
        .min_by(|left, right| {
            left.pace_wkg
                .unwrap_or(0.0)
                .total_cmp(&right.pace_wkg.unwrap_or(0.0))
        })
        .expect("slow pace rider")
        .id;
    let home = peloton
        .riders()
        .iter()
        .find(|rider| rider.id == fast_id)
        .expect("fast")
        .slot;
    let mut min_slot = home;
    let mut returned = false;
    let dt = 1.0 / 30.0;
    let player = player_at(0.0, 0.0, 0.0);
    for _ in 0..(6.0 * 60.0 / dt) as i32 {
        peloton.tick(dt, &player);
        assert!(overlaps(&peloton, &player).is_none());
        let fast = peloton
            .riders()
            .iter()
            .find(|rider| rider.id == fast_id)
            .expect("fast stays");
        let slow = peloton
            .riders()
            .iter()
            .find(|rider| rider.id == slow_id)
            .expect("slow stays");
        min_slot = min_slot.min(fast.slot);
        if fast.total_distance_m > slow.total_distance_m + 25.0 && (fast.slot - home).abs() < 0.35 {
            returned = true;
        }
    }
    assert!(
        min_slot < home - 0.45,
        "fast rider started at {home:.2} and only reached {min_slot:.2}"
    );
    assert!(returned, "fast rider did not drift back out after the pass");
}

#[test]
fn riders_who_leave_the_group_do_it_out_of_sight() {
    let course = flat_loop(10_000.0);
    let mut peloton = Peloton::new(
        &course,
        6,
        BotConfig {
            count: 4,
            pace_riders: false,
            ..BotConfig::default()
        },
    );
    let dt = 1.0 / 30.0;
    let speed = 18.0_f32;
    let mut left = false;
    let mut joined = false;
    for step in 0..(80.0 / dt) as i32 {
        let t = dt * (step + 1) as f32;
        let player = player_at(f64::from(speed) * f64::from(t), speed, 70.0);
        let before: Vec<(u32, f64)> = peloton
            .riders()
            .iter()
            .map(|rider| (rider.id, rider.total_distance_m))
            .collect();
        let events = peloton.tick(dt, &player);
        for event in &events {
            match event {
                BotEvent::Left { id } => {
                    left = true;
                    let distance = before
                        .iter()
                        .find(|(rider, _)| rider == id)
                        .expect("was here")
                        .1;
                    assert!((distance - player.total_distance_m).abs() >= 250.0);
                }
                BotEvent::Joined { id } => {
                    joined = true;
                    let rider = peloton
                        .riders()
                        .iter()
                        .find(|rider| rider.id == *id)
                        .expect("joined");
                    assert!((rider.total_distance_m - player.total_distance_m).abs() >= 250.0);
                }
                BotEvent::Passed { .. } | BotEvent::PassedBy { .. } => {}
            }
        }
    }
    assert!(left, "nobody retired");
    assert!(joined, "nobody was introduced");
}

#[test]
fn cadence_is_lower_on_a_steep_grade_and_standing_is_brief() {
    let flat = cadence_rpm(250.0, 75.0, 0.0, 0.0);
    let climb = cadence_rpm(250.0, 75.0, 8.0, 0.0);
    assert!(
        climb < flat - 8.0,
        "{climb} rpm on the climb, {flat} on the flat"
    );
    assert!(standing(7.0, 0.0, 1.0, 0.0));
    assert!(!standing(7.0, 0.0, 10.0, 0.0));
    assert!(!standing(0.0, 0.9, 1.0, 0.0));
    assert!(standing(3.0, 0.8, 1.0, 0.0));
    assert!((gap_nudge(40.0) - 1.0).abs() < 1.0e-6);
    assert!(gap_nudge(300.0) < 0.95);
    assert!(gap_nudge(-300.0) > 1.05);
}
