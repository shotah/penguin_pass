//! Behaviour checks for generated loops and the endless road.

use std::str::FromStr;
use std::time::Instant;

use pp_course::Course;

use crate::endless::EndlessRoute;
use crate::geom::{self, angle_diff};
use crate::spec::{
    CENTERLINE_CLEARANCE_M, CLEARANCE_WINDOW_M, MAX_ELEVATION_M, MAX_FOOTPRINT_FRACTION,
    MIN_FOOTPRINT_FRACTION, RouteSpec, Terrain, TerrainParams, TerrainPreset, ascent_tolerance_m,
};
use crate::{fingerprint, generate, survey};

fn spec(name: &str, seed: u64, distance_m: f64, preset: TerrainPreset) -> RouteSpec {
    RouteSpec::new(name, seed, distance_m, Terrain::Preset(preset))
}

fn heading(course: &Course, distance_m: f64) -> f64 {
    let t = course.tangent_at(distance_m);
    f64::from(t[2]).atan2(f64::from(t[0]))
}

fn flat_run_m(course: &Course) -> f64 {
    let length = course.length_m();
    let h0 = heading(course, 0.0);
    let mut forward = 0.0;
    let mut d = 5.0;
    while d < length {
        let turn = angle_diff(heading(course, d), h0).abs();
        if turn > 0.03 || course.grade_pct_at(d).abs() > 0.3 {
            break;
        }
        forward = d;
        d += 5.0;
    }
    let mut back = 0.0;
    d = 5.0;
    while d < length {
        let at = length - d;
        let turn = angle_diff(heading(course, at), h0).abs();
        if turn > 0.03 || course.grade_pct_at(at).abs() > 0.3 {
            break;
        }
        back = d;
        d += 5.0;
    }
    forward + back
}

fn grade_ok(course: &Course, params: &TerrainParams) {
    let length = course.length_m();
    let mut d = 0.0;
    let mut prev = f64::from(course.grade_pct_at(0.0));
    let mut min_e = 0.0_f64;
    let mut max_e = 0.0_f64;
    while d <= length {
        let g = f64::from(course.grade_pct_at(d));
        assert!(
            g <= params.max_up_grade_pct + 0.08,
            "grade {g} above {}",
            params.max_up_grade_pct
        );
        assert!(
            g >= -params.max_down_grade_pct - 0.08,
            "grade {g} below -{}",
            params.max_down_grade_pct
        );
        let rate = (g - prev).abs();
        assert!(
            rate <= params.max_grade_rate * 1.25 + 0.05,
            "grade changed {rate} in 1 m (limit {})",
            params.max_grade_rate
        );
        let e = f64::from(course.elevation_at(d));
        min_e = min_e.min(e);
        max_e = max_e.max(e);
        prev = g;
        d += 1.0;
    }
    assert!(min_e >= -0.05, "elevation {min_e}");
    assert!(max_e <= MAX_ELEVATION_M + 0.5, "elevation {max_e}");
    let seam = (f64::from(course.grade_pct_at(0.0)) - f64::from(course.grade_pct_at(length))).abs();
    assert!(seam < 0.05, "grade seam {seam}");
}

fn radius_ok(course: &Course, floor_m: f64) {
    let length = course.length_m();
    let step = 2.0;
    let mut d = 0.0;
    while d + step < length {
        let p0 = course.position_at(d);
        let p1 = course.position_at(d + step);
        let horiz = (f64::from(p1[0] - p0[0])).hypot(f64::from(p1[2] - p0[2]));
        let dh = angle_diff(heading(course, d + step), heading(course, d)).abs();
        if dh > 1.0e-3 && horiz > 0.2 {
            let radius = horiz / dh;
            assert!(
                radius + 0.8 >= floor_m,
                "radius {radius:.1} m under {floor_m:.1} at {d:.0} m"
            );
        }
        d += step;
    }
}

fn clearance_ok(course: &Course, step_m: f64) {
    let length = course.length_m();
    let n = ((length / step_m).round() as usize).max(8);
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let p = course.position_at(length * i as f64 / n as f64);
        pts.push((f64::from(p[0]), f64::from(p[2])));
    }
    let along = length / n as f64;
    let gap = geom::closest_approach_brute(&pts, along, length, CLEARANCE_WINDOW_M);
    assert!(
        gap + 1.0 >= CENTERLINE_CLEARANCE_M,
        "centerlines approach within {gap:.1} m"
    );
}

fn footprint_ok(course: &Course, distance_m: f64) {
    let report = survey(course, 8.0);
    let long = report.footprint_east_m.max(report.footprint_north_m);
    let short = report.footprint_east_m.min(report.footprint_north_m);
    assert!(long <= MAX_FOOTPRINT_FRACTION * distance_m + 1.0, "{long}");
    assert!(long + 1.0 >= MIN_FOOTPRINT_FRACTION * distance_m, "{long}");
    assert!(short >= 40.0, "{short}");
    assert!(long <= short * 5.6, "{long} by {short}");
}

fn varied(course: &Course, preset: TerrainPreset) {
    let length = course.length_m();
    let mut min_g = f64::MAX;
    let mut max_g = f64::MIN;
    let mut d = 0.0;
    let mut samples = Vec::new();
    while d < length {
        let g = f64::from(course.grade_pct_at(d));
        min_g = min_g.min(g);
        max_g = max_g.max(g);
        if samples.len() < 4_000 {
            samples.push(g);
        }
        d += (length / 2_000.0).max(2.0);
    }
    assert!(max_g - min_g > 0.4, "grade barely changes");
    match preset {
        TerrainPreset::Flat => {
            assert!(max_g < 2.3 && min_g > -2.3, "{min_g}..{max_g}");
            assert!(max_g > 0.4, "flat route has no rise");
        }
        TerrainPreset::Rolling => {
            assert!((3.0..=6.2).contains(&max_g), "max {max_g}");
            assert!((-6.2..-3.0).contains(&min_g), "min {min_g}");
            assert!(has_breather(&samples, 0.8), "rolling climb has no breather");
        }
        TerrainPreset::Mountain => {
            assert!(max_g >= 5.0, "mountain max grade {max_g}");
            assert!(min_g <= -5.0, "mountain min grade {min_g}");
            assert!(
                has_breather(&samples, 1.2),
                "mountain climb has no breather"
            );
        }
        TerrainPreset::Desert => {
            assert!(max_g >= 4.5, "desert has no steep ramp ({max_g})");
            assert!(min_g < -1.0, "desert has no descent");
            let drags = samples.iter().filter(|g| (1.0..4.3).contains(*g)).count();
            assert!(drags > samples.len() / 20, "desert drags are missing");
        }
    }
}

fn has_breather(samples: &[f64], gap: f64) -> bool {
    for j in 1..samples.len().saturating_sub(1) {
        if samples[j] < 0.8 {
            continue;
        }
        let before = samples[..j].iter().copied().fold(0.0, f64::max);
        let after = samples[j + 1..].iter().copied().fold(0.0, f64::max);
        if before > samples[j] + gap && after > samples[j] + gap {
            return true;
        }
    }
    false
}

fn assert_rideable(preset: TerrainPreset, seed: u64, distance_m: f64, fine_clearance: bool) {
    let params = preset.params();
    let built = generate(&spec("Ride", seed, distance_m, preset)).unwrap_or_else(|err| {
        panic!("{preset} seed {seed} {distance_m} m: {err}");
    });
    let course = Course::from_str(built.json()).expect("json parses");
    assert!((course.length_m() - distance_m).abs() < 1.0);
    let target = params.ascent_per_km * distance_m / 1000.0;
    let ascent = f64::from(course.total_ascent_m());
    assert!(
        (ascent - target).abs() <= ascent_tolerance_m(target) + 0.2,
        "ascent {ascent} vs {target}"
    );
    let flat = flat_run_m(&course);
    assert!(flat >= 150.0, "flat {flat}");
    grade_ok(&course, &params);
    let floor = if preset == TerrainPreset::Mountain {
        params.switchback_radius_m
    } else {
        params.min_radius_m
    };
    radius_ok(&course, floor);
    footprint_ok(&course, distance_m);
    varied(&course, preset);
    let step = if fine_clearance { 4.0 } else { 18.0 };
    clearance_ok(&course, step);
}

#[test]
fn spec_rejects_nonsense() {
    let err = spec(" ", 1, 5_000.0, TerrainPreset::Flat)
        .validate()
        .unwrap_err();
    assert!(err.to_string().contains("empty"), "{err}");
    let err = spec("Loop", 1, f64::NAN, TerrainPreset::Flat)
        .validate()
        .unwrap_err();
    assert!(err.to_string().contains("finite"), "{err}");
    let err = spec("Loop", 1, 500.0, TerrainPreset::Flat)
        .validate()
        .unwrap_err();
    assert!(err.to_string().contains("outside"), "{err}");
    let err = spec("Loop", 1, 200_000.0, TerrainPreset::Desert)
        .validate()
        .unwrap_err();
    assert!(err.to_string().contains("outside"), "{err}");
    let mut bad = TerrainPreset::Rolling.params();
    bad.ascent_per_km = 80.0;
    let err = RouteSpec::new("Loop", 1, 10_000.0, Terrain::Custom(bad))
        .validate()
        .unwrap_err();
    assert!(err.to_string().contains("ascent"), "{err}");
    let mut huge = TerrainPreset::Flat.params();
    huge.min_radius_m = 1_500.0;
    let err = RouteSpec::new("Loop", 1, 2_000.0, Terrain::Custom(huge))
        .validate()
        .unwrap_err();
    assert!(err.to_string().contains("radius"), "{err}");
}

#[test]
fn different_seeds_differ() {
    let a = generate(&spec("A", 1, 8_000.0, TerrainPreset::Rolling)).expect("a");
    let b = generate(&spec("A", 2, 8_000.0, TerrainPreset::Rolling)).expect("b");
    assert_ne!(a.json(), b.json());
    let pa = a.course().position_at(2_000.0);
    let pb = b.course().position_at(2_000.0);
    let dx = f64::from(pa[0] - pb[0]);
    let dz = f64::from(pa[2] - pb[2]);
    assert!(dx.hypot(dz) > 30.0, "seeds produced the same shape");
}

#[test]
fn preset_properties_at_several_distances() {
    let started = Instant::now();
    for preset in [
        TerrainPreset::Flat,
        TerrainPreset::Rolling,
        TerrainPreset::Mountain,
        TerrainPreset::Desert,
    ] {
        for (distance_m, count) in [(5_000.0, 120_u64), (10_000.0, 80_u64)] {
            for seed in 0..count {
                let fine = seed < 2;
                assert_rideable(preset, seed, distance_m, fine);
            }
        }
    }
    eprintln!("property suite {:.1}s", started.elapsed().as_secs_f64());
}

#[test]
fn extremes_two_and_one_hundred_km() {
    for preset in [
        TerrainPreset::Flat,
        TerrainPreset::Rolling,
        TerrainPreset::Mountain,
        TerrainPreset::Desert,
    ] {
        for seed in [0_u64, 7] {
            assert_rideable(preset, seed, 2_000.0, true);
        }
        assert_rideable(preset, 4, 100_000.0, false);
    }
}

#[test]
fn stability_fingerprints() {
    // Locked after the pictures in docs/img/routes were accepted.
    // ChaCha12 (rand_chacha 0.3.1 / rand 0.8.5) plus this JSON writer.
    let cases = [
        (
            TerrainPreset::Desert,
            7_u64,
            10_000.0,
            "Desert 10 km",
            0xccb3_998f_bd11_8f27,
        ),
        (
            TerrainPreset::Mountain,
            11,
            10_000.0,
            "Mountain 10 km",
            0xd6f2_1ae4_986f_71a3,
        ),
        (
            TerrainPreset::Rolling,
            3,
            5_000.0,
            "Rolling 5 km",
            0xd13e_48e5_5dbc_e8f1,
        ),
        (
            TerrainPreset::Flat,
            1,
            2_000.0,
            "Flat 2 km",
            0x1e4c_6612_ac42_1ba8,
        ),
    ];
    for (preset, seed, distance, name, expected) in cases {
        let route = generate(&spec(name, seed, distance, preset)).expect(name);
        let got = fingerprint(route.json());
        assert_eq!(got, expected, "{name} fingerprint changed to {got:016x}");
    }
}

#[test]
fn endless_joins_and_bounds() {
    let road = EndlessRoute::new(42);
    let section = EndlessRoute::section_len_m();
    for index in [1_u64, 2, 5, 40, 499] {
        let d = index as f64 * section;
        let before = road.position_at(d - 0.5);
        let at = road.position_at(d);
        let after = road.position_at(d + 0.5);
        for axis in 0..3 {
            let jump = (f64::from(at[axis]) - f64::from(before[axis])).abs();
            assert!(jump < 2.0, "position jump {jump} at section {index}");
            let step = (f64::from(after[axis]) - f64::from(at[axis])).abs();
            assert!(step < 2.0, "position step {step} at section {index}");
        }
        let g0 = f64::from(road.grade_pct_at(d - 0.25));
        let g1 = f64::from(road.grade_pct_at(d + 0.25));
        assert!((g0 - g1).abs() < 0.15, "grade {g0} vs {g1} at {index}");
        let h0 = {
            let t = road.tangent_at(d - 0.25);
            f64::from(t[2]).atan2(f64::from(t[0]))
        };
        let h1 = {
            let t = road.tangent_at(d + 0.25);
            f64::from(t[2]).atan2(f64::from(t[0]))
        };
        assert!(
            angle_diff(h0, h1).abs() < 0.05,
            "heading jump at section {index}"
        );
    }
    let mut min_e = f64::MAX;
    let mut max_e = f64::MIN;
    let mut prev_x = f64::NEG_INFINITY;
    for index in 0..500 {
        let d = index as f64 * section;
        let e = f64::from(road.elevation_at(d));
        min_e = min_e.min(e);
        max_e = max_e.max(e);
        let x = f64::from(road.position_at(d)[0]);
        assert!(x > prev_x, "x went backwards at section {index}");
        prev_x = x;
        let h = {
            let t = road.tangent_at(d + 100.0);
            f64::from(t[2]).atan2(f64::from(t[0]))
        };
        assert!(h.abs() < 1.05, "heading {h} rad leaves the forward band");
    }
    assert!(min_e > 0.0 && max_e < 1_200.0, "elev {min_e}..{max_e}");
}

#[test]
fn endless_random_access_matches_and_cache_is_bounded() {
    let road = EndlessRoute::new(7);
    let other = EndlessRoute::new(7);
    let mut sequential = Vec::new();
    let mut d = 0.0;
    while d <= 30_000.0 {
        sequential.push(road.position_at(d));
        d += 250.0;
    }
    d = 0.0;
    for point in sequential {
        let jumped = other.position_at(d);
        for axis in 0..3 {
            assert!((point[axis] - jumped[axis]).abs() < 1.0e-3);
        }
        d += 250.0;
    }
    let far = EndlessRoute::new(7);
    let _ = far.position_at(800_000.0);
    let _ = far.position_at(12_000.0);
    let _ = far.position_at(900_000.0);
    assert!(far.cache_len() <= 8);
    assert_eq!(far.position_at(800_000.0), road.position_at(800_000.0));
}

#[test]
fn endless_does_not_approach_itself() {
    let road = EndlessRoute::new(99);
    let mut prev_x = f64::NEG_INFINITY;
    let mut d = 0.0;
    while d < 40_000.0 {
        let p = road.position_at(d);
        let x = f64::from(p[0]);
        assert!(x + 1.0e-6 >= prev_x, "x decreased at {d}");
        prev_x = x;
        let h = {
            let t = road.tangent_at(d);
            f64::from(t[2]).atan2(f64::from(t[0]))
        };
        assert!(h.abs() < 1.05, "heading {h} at {d}");
        d += 10.0;
    }
    // Points more than 80 m apart along a forward road are separated in x
    // by at least that distance times cos(heading cap).
    let p0 = road.position_at(1_000.0);
    let p1 = road.position_at(1_200.0);
    let dx = f64::from(p1[0] - p0[0]);
    assert!(dx > 80.0, "200 m of road only gained {dx:.1} m east");
}
