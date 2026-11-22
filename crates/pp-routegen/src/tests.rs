//! Behaviour checks for generated loops and the endless road.

use std::str::FromStr;

use pp_course::Course;

use crate::endless::EndlessRoute;
use crate::geom::angle_diff;
use crate::spec::{
    MAX_ELEVATION_M, RouteSpec, Terrain, TerrainParams, TerrainPreset, ascent_tolerance_m,
};
use crate::{fingerprint, generate};

fn spec(name: &str, seed: u64, distance_m: f64, preset: TerrainPreset) -> RouteSpec {
    RouteSpec::new(name, seed, distance_m, Terrain::Preset(preset))
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
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn stability_fingerprints() {
    // Locked after the pictures in docs/img/routes were accepted.
    // ChaCha12 (rand 0.10.4 `rngs::ChaCha12Rng`); locked bytes moved with this bump.
    let cases = [
        (
            TerrainPreset::Desert,
            7_u64,
            10_000.0,
            "Desert 10 km",
            0x8ad6_bf25_f272_a0b5,
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
            0x3b7f_3f2a_a958_6c66,
        ),
        (
            TerrainPreset::Flat,
            1,
            2_000.0,
            "Flat 2 km",
            0xb977_9c73_3e2d_b013,
        ),
    ];
    for (preset, seed, distance, name, expected) in cases {
        let route = generate(&spec(name, seed, distance, preset)).expect(name);
        let got = fingerprint(route.json());
        assert_eq!(got, expected, "{name} fingerprint changed to {got:016x}");
    }
}

#[test]
fn stability_same_host_and_geometry() {
    let cases = [
        (TerrainPreset::Desert, 7_u64, 10_000.0, "Desert 10 km"),
        (TerrainPreset::Mountain, 11, 10_000.0, "Mountain 10 km"),
        (TerrainPreset::Rolling, 3, 5_000.0, "Rolling 5 km"),
        (TerrainPreset::Flat, 1, 2_000.0, "Flat 2 km"),
    ];
    for (preset, seed, distance, name) in cases {
        let route_a = generate(&spec(name, seed, distance, preset)).expect(name);
        let route_b = generate(&spec(name, seed, distance, preset)).expect(name);
        let fp_a = fingerprint(route_a.json());
        let fp_b = fingerprint(route_b.json());
        assert_eq!(fp_a, fp_b, "{name} fingerprint not stable on this host");
        let course = Course::from_str(route_a.json()).expect("json parses");
        assert!((course.length_m() - distance).abs() < 1.0);
        let params = preset.params();
        let target = params.ascent_per_km * distance / 1000.0;
        let ascent = f64::from(course.total_ascent_m());
        assert!(
            (ascent - target).abs() <= ascent_tolerance_m(target) + 0.2,
            "{name} ascent {ascent} vs {target}"
        );
        grade_ok(&course, &params);
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
