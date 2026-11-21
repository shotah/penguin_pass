//! Fifteen built-in workouts, from 20 to 90 minutes.
//!
//! The shapes are ordinary (sweet spot, threshold, a ramp test). The names
//! and the files are this game's. Nothing here is copied from another app.

use crate::model::{Cue, IntervalStep, RampStep, SteadyStep, Step, Workout};

/// The fifteen workouts shipped with the game, in list order.
#[must_use]
pub fn workouts() -> Vec<Workout> {
    vec![
        easy_spin(),
        endurance_20(),
        sweet_spot(),
        threshold(),
        over_unders(),
        vo2(),
        pyramid(),
        thirties(),
        sprints(),
        ramp_test(),
        over_under_sets(),
        vo2max_4x4(),
        micro_bursts(),
        climb_pyramid(),
        endurance_base(),
    ]
}

fn plan(id: &str, name: &str, description: &str, steps: Vec<Step>, ramp_test: bool) -> Workout {
    Workout {
        id: id.to_owned(),
        name: name.to_owned(),
        description: description.to_owned(),
        steps,
        ramp_test,
    }
}

fn warmup(seconds: f32, low: f32, high: f32) -> Step {
    Step::Warmup(RampStep {
        duration_s: seconds,
        power_low: low,
        power_high: high,
        cadence_rpm: Some(85),
        cues: Vec::new(),
    })
}

fn cooldown(seconds: f32, low: f32, high: f32) -> Step {
    Step::Cooldown(RampStep {
        duration_s: seconds,
        power_low: low,
        power_high: high,
        cadence_rpm: Some(85),
        cues: Vec::new(),
    })
}

fn hold(seconds: f32, power: f32, label: &str, cadence: Option<u16>, cue: Option<&str>) -> Step {
    Step::Steady(SteadyStep {
        duration_s: seconds,
        power,
        cadence_rpm: cadence,
        cues: cue
            .map(|text| Cue {
                offset_s: 0.0,
                text: text.to_owned(),
            })
            .into_iter()
            .collect(),
        label: Some(label.to_owned()),
    })
}

#[allow(clippy::too_many_arguments)]
fn repeats(
    count: u16,
    on_s: f32,
    off_s: f32,
    on_power: f32,
    off_power: f32,
    on_label: &str,
    off_label: &str,
    on_cadence: Option<u16>,
    off_cadence: Option<u16>,
    cue: Option<&str>,
) -> Step {
    Step::Intervals(IntervalStep {
        repeat: count,
        on_s,
        off_s,
        on_power,
        off_power,
        on_cadence,
        off_cadence,
        cues: cue
            .map(|text| Cue {
                offset_s: 0.0,
                text: text.to_owned(),
            })
            .into_iter()
            .collect(),
        on_label: Some(on_label.to_owned()),
        off_label: Some(off_label.to_owned()),
    })
}

fn easy_spin() -> Workout {
    plan(
        "easy-spin",
        "Easy spin",
        "Thirty minutes at endurance pace, with a short warm-up and cool-down.",
        vec![
            warmup(180.0, 0.40, 0.55),
            hold(1320.0, 0.60, "Easy spin", Some(85), Some("Stay light")),
            cooldown(300.0, 0.55, 0.40),
        ],
        false,
    )
}

fn endurance_20() -> Workout {
    plan(
        "endurance-20",
        "Endurance",
        "Twenty minutes at endurance pace.",
        vec![
            warmup(120.0, 0.45, 0.60),
            hold(960.0, 0.65, "Endurance", Some(85), Some("Smooth pedals")),
            cooldown(120.0, 0.60, 0.45),
        ],
        false,
    )
}

fn sweet_spot() -> Workout {
    plan(
        "sweet-spot-3x10",
        "Sweet spot",
        "Three ten-minute efforts at 91% of FTP, with five minutes easy between them.",
        vec![
            warmup(360.0, 0.45, 0.75),
            repeats(
                3,
                600.0,
                300.0,
                0.91,
                0.55,
                "Sweet spot",
                "Recover",
                Some(90),
                Some(85),
                Some("Settle into the effort"),
            ),
            cooldown(300.0, 0.55, 0.40),
        ],
        false,
    )
}

fn threshold() -> Workout {
    plan(
        "threshold-2x20",
        "Threshold",
        "Two twenty-minute efforts at FTP, with five minutes easy between them.",
        vec![
            warmup(480.0, 0.45, 0.80),
            hold(
                1200.0,
                1.00,
                "Threshold 1 of 2",
                Some(90),
                Some("Hold this"),
            ),
            hold(300.0, 0.55, "Recover", Some(85), None),
            hold(1200.0, 1.00, "Threshold 2 of 2", Some(90), None),
            cooldown(420.0, 0.55, 0.40),
        ],
        false,
    )
}

fn over_unders() -> Workout {
    plan(
        "over-unders",
        "Over-unders",
        "Two sets of four: two minutes at 105% of FTP, then two minutes at 95%.",
        vec![
            warmup(480.0, 0.45, 0.75),
            repeats(
                4,
                120.0,
                120.0,
                1.05,
                0.95,
                "Over",
                "Under",
                Some(90),
                Some(90),
                Some("Over, then under"),
            ),
            hold(240.0, 0.55, "Recover", Some(85), None),
            repeats(
                4,
                120.0,
                120.0,
                1.05,
                0.95,
                "Over",
                "Under",
                Some(90),
                Some(90),
                None,
            ),
            cooldown(240.0, 0.55, 0.40),
        ],
        false,
    )
}

fn vo2() -> Workout {
    plan(
        "vo2-5x3",
        "VO2",
        "Five three-minute efforts at 120% of FTP, with three minutes easy between them.",
        vec![
            warmup(600.0, 0.40, 0.70),
            repeats(
                5,
                180.0,
                180.0,
                1.20,
                0.50,
                "VO2",
                "Recover",
                Some(95),
                Some(85),
                Some("Hard, then easy"),
            ),
            cooldown(360.0, 0.50, 0.40),
        ],
        false,
    )
}

fn pyramid() -> Workout {
    let ons = [60.0_f32, 120.0, 180.0, 240.0, 180.0, 120.0, 60.0];
    let recoveries = [60.0_f32, 120.0, 180.0, 180.0, 120.0, 60.0];
    let mut steps = vec![warmup(480.0, 0.40, 0.70)];
    for (index, on) in ons.iter().enumerate() {
        let minutes = (*on / 60.0).round();
        steps.push(hold(
            *on,
            1.20,
            &format!("{minutes:.0} min"),
            Some(95),
            (index == 0).then_some("Build up, then back down"),
        ));
        if let Some(recovery) = recoveries.get(index) {
            steps.push(hold(*recovery, 0.50, "Recover", Some(85), None));
        }
    }
    steps.push(cooldown(240.0, 0.50, 0.40));
    plan(
        "pyramid",
        "Pyramid",
        "One to four minutes at 120% of FTP, and back down, with easy time between.",
        steps,
        false,
    )
}

fn thirties() -> Workout {
    plan(
        "thirties",
        "30/30s",
        "Fifteen repeats of thirty seconds at 150% of FTP and thirty seconds easy.",
        vec![
            warmup(600.0, 0.40, 0.70),
            repeats(
                15,
                30.0,
                30.0,
                1.50,
                0.45,
                "30/30",
                "Recover",
                Some(100),
                Some(85),
                Some("Thirty seconds on"),
            ),
            cooldown(300.0, 0.50, 0.40),
        ],
        false,
    )
}

fn sprints() -> Workout {
    plan(
        "sprints",
        "Sprints",
        "Eight fifteen-second sprints at 200% of FTP, with two minutes fifteen easy.",
        vec![
            warmup(600.0, 0.40, 0.70),
            repeats(
                8,
                15.0,
                135.0,
                2.00,
                0.50,
                "Sprint",
                "Recover",
                Some(110),
                Some(85),
                Some("Jump, then spin"),
            ),
            cooldown(360.0, 0.50, 0.40),
        ],
        false,
    )
}

fn over_under_sets() -> Workout {
    let mut steps = vec![warmup(360.0, 0.45, 0.75)];
    for index in 0..3 {
        steps.push(repeats(
            3,
            120.0,
            60.0,
            0.95,
            1.05,
            "Under",
            "Over",
            Some(90),
            Some(95),
            (index == 0).then_some("Under, then over"),
        ));
        if index < 2 {
            steps.push(hold(180.0, 0.55, "Recover", Some(85), None));
        }
    }
    steps.push(cooldown(360.0, 0.55, 0.40));
    plan(
        "over-unders-3x9",
        "Over-unders 3x9",
        "Three nine-minute sets alternating two minutes at 95% of FTP and one minute at 105%, \
         with three minutes easy between them.",
        steps,
        false,
    )
}

fn vo2max_4x4() -> Workout {
    plan(
        "vo2max-4x4",
        "VO2max 4x4",
        "Four four-minute efforts at 112% of FTP, with four minutes easy between them.",
        vec![
            warmup(300.0, 0.40, 0.70),
            repeats(
                4,
                240.0,
                240.0,
                1.12,
                0.55,
                "VO2",
                "Recover",
                Some(95),
                Some(85),
                Some("Hard, then easy"),
            ),
            cooldown(180.0, 0.55, 0.40),
        ],
        false,
    )
}

fn micro_bursts() -> Workout {
    plan(
        "micro-bursts-30-30",
        "Micro-bursts",
        "Two sets of eight: thirty seconds at 125% of FTP and thirty seconds easy, \
         with five minutes between the sets.",
        vec![
            warmup(540.0, 0.40, 0.70),
            repeats(
                8,
                30.0,
                30.0,
                1.25,
                0.55,
                "Burst",
                "Recover",
                Some(100),
                Some(90),
                Some("Thirty seconds on"),
            ),
            hold(300.0, 0.55, "Recover", Some(85), None),
            repeats(
                8,
                30.0,
                30.0,
                1.25,
                0.55,
                "Burst",
                "Recover",
                Some(100),
                Some(90),
                None,
            ),
            cooldown(300.0, 0.55, 0.40),
        ],
        false,
    )
}

fn climb_pyramid() -> Workout {
    let climbs = [
        (60.0_f32, 0.95_f32),
        (120.0, 1.00),
        (180.0, 1.05),
        (240.0, 1.15),
        (180.0, 1.05),
        (120.0, 1.00),
        (60.0, 0.95),
    ];
    let mut steps = vec![warmup(540.0, 0.40, 0.70)];
    for (index, (seconds, power)) in climbs.iter().enumerate() {
        let minutes = (*seconds / 60.0).round();
        steps.push(hold(
            *seconds,
            *power,
            &format!("{minutes:.0} min"),
            Some(90),
            (index == 0).then_some("Climb, then ease"),
        ));
        if index + 1 < climbs.len() {
            steps.push(hold(*seconds, 0.50, "Recover", Some(85), None));
        }
    }
    steps.push(cooldown(300.0, 0.50, 0.40));
    plan(
        "climb-pyramid",
        "Climb pyramid",
        "One to four minutes climbing from 95% to 115% of FTP and back down, \
         with an equal easy spell after each climb but the last.",
        steps,
        false,
    )
}

fn endurance_base() -> Workout {
    plan(
        "endurance-base-90",
        "Endurance base",
        "Ninety minutes of aerobic riding, holding 68% to 72% of FTP, \
         with three one-minute surges at 85%.",
        vec![
            warmup(600.0, 0.45, 0.68),
            hold(1080.0, 0.68, "Endurance", Some(85), Some("Stay aerobic")),
            hold(60.0, 0.85, "Surge", Some(95), Some("Spin a little faster")),
            hold(1080.0, 0.70, "Endurance", Some(85), None),
            hold(60.0, 0.85, "Surge", Some(95), None),
            hold(1080.0, 0.72, "Endurance", Some(85), None),
            hold(60.0, 0.85, "Surge", Some(95), None),
            hold(1080.0, 0.70, "Endurance", Some(85), None),
            cooldown(300.0, 0.68, 0.45),
        ],
        false,
    )
}

fn ramp_test() -> Workout {
    // One-minute steps from 60% to 180% of the current FTP, adding 6% each minute.
    // 75% of the last minute the rider finishes is the estimate. See docs/workouts.md.
    let mut steps = vec![warmup(300.0, 0.40, 0.55)];
    let mut first = true;
    let mut percent = 60_u16;
    while percent <= 180 {
        let label = format!("{percent}%");
        steps.push(hold(
            60.0,
            f32::from(percent) / 100.0,
            &label,
            Some(90),
            first.then_some("Finish each minute. Stop when you cannot."),
        ));
        first = false;
        percent += 6;
    }
    steps.push(cooldown(180.0, 0.50, 0.40));
    plan(
        "ramp-test",
        "Ramp test",
        "One-minute steps from 60% of FTP, adding 6% each minute, up to 180%. \
         The estimate is 75% of the last minute you finish.",
        steps,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_adds_up_and_stays_in_bounds() {
        let expected = [
            ("easy-spin", 1800.0, false),
            ("endurance-20", 1200.0, false),
            ("sweet-spot-3x10", 3360.0, false),
            ("threshold-2x20", 3600.0, false),
            ("over-unders", 2880.0, false),
            ("vo2-5x3", 2760.0, false),
            ("pyramid", 2400.0, false),
            ("thirties", 1800.0, false),
            ("sprints", 2160.0, false),
            ("ramp-test", 1740.0, true),
            ("over-unders-3x9", 2700.0, false),
            ("vo2max-4x4", 2400.0, false),
            ("micro-bursts-30-30", 2100.0, false),
            ("climb-pyramid", 2700.0, false),
            ("endurance-base-90", 5400.0, false),
        ];
        let all = workouts();
        assert_eq!(all.len(), expected.len());
        let mut ids = Vec::new();
        for (workout, (id, seconds, ramp)) in all.iter().zip(expected) {
            assert_eq!(workout.id, id);
            assert_eq!(workout.ramp_test, ramp);
            assert!(!workout.name.is_empty());
            assert!(!workout.description.is_empty());
            assert!((workout.duration_s() - seconds).abs() < 1e-3, "{id}");
            let bars = workout.profile();
            let mut cursor = 0.0_f32;
            for bar in &bars {
                assert!((bar.start_s - cursor).abs() < 1e-3, "{id} {}", bar.label);
                assert!(bar.duration_s > 0.0);
                cursor += bar.duration_s;
                match (bar.start_power, bar.end_power) {
                    (Some(start), Some(end)) => {
                        assert!((0.25..=2.0).contains(&start), "{id} {start}");
                        assert!((0.25..=2.0).contains(&end), "{id} {end}");
                    }
                    (None, None) => assert!(bar.open),
                    other => panic!("{id} uneven powers {other:?}"),
                }
                assert!(!bar.open, "{id} built-ins hold a target");
            }
            assert!((cursor - seconds).abs() < 1e-3, "{id} profile");
            let step_sum: f32 = workout.steps.iter().map(Step::duration_s).sum();
            assert!((step_sum - seconds).abs() < 1e-3);
            let intensity = workout.intensity();
            assert!(
                (0.40..=1.20).contains(&intensity.average_ftp_fraction),
                "{id} {}",
                intensity.average_ftp_fraction
            );
            let score = intensity.stress_score.expect(id);
            assert!((10.0..=150.0).contains(&score), "{id} {score}");
            ids.push(workout.id.clone());
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), expected.len());
    }

    #[test]
    fn sweet_spot_and_the_ramp_test_expand_as_planned() {
        let sweet = workouts()
            .into_iter()
            .find(|workout| workout.id == "sweet-spot-3x10")
            .expect("sweet spot");
        let bars = sweet.profile();
        assert_eq!(bars.len(), 8);
        assert_eq!(bars[1].label, "Sweet spot 1 of 3");
        assert_eq!(bars[2].label, "Recover");
        assert_eq!(bars[5].label, "Sweet spot 3 of 3");
        assert!((bars[1].start_power.unwrap() - 0.91).abs() < 1e-6);
        assert!((bars[1].duration_s - 600.0).abs() < 1e-3);

        let ramp = workouts()
            .into_iter()
            .find(|workout| workout.id == "ramp-test")
            .expect("ramp");
        let scored: Vec<_> = ramp
            .profile()
            .into_iter()
            .filter(|bar| bar.label.ends_with('%'))
            .collect();
        assert_eq!(scored.len(), 21);
        assert_eq!(scored[0].label, "60%");
        assert_eq!(scored[20].label, "180%");
        assert!((scored[0].duration_s - 60.0).abs() < 1e-3);
        assert!((scored[0].start_power.unwrap() - 0.60).abs() < 1e-6);
        assert!((scored[20].start_power.unwrap() - 1.80).abs() < 1e-5);
    }

    #[test]
    fn endurance_average_is_the_time_weighted_fraction() {
        let ride = workouts()
            .into_iter()
            .find(|workout| workout.id == "endurance-20")
            .expect("endurance");
        // 2 min at the midpoint of 0.45→0.60, 16 min at 0.65, 2 min at the midpoint of 0.60→0.45.
        // (120 * 0.525 + 960 * 0.65 + 120 * 0.525) / 1200 = 0.625.
        let average = ride.intensity().average_ftp_fraction;
        assert!((average - 0.625).abs() < 1e-4, "{average}");
    }
}
