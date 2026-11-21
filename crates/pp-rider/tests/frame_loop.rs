//! The crate as the game will use it, through the public API only.

use std::f64::consts::TAU;

use pp_rider::{
    CRANK_LENGTH_M, MAX_STEER_RAD, PartId, PoseInput, RideAnimator, RideSample, RiderModel,
    RiderStyle, StyleColor, WHEEL_RADIUS_M,
};

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn a_ride_can_be_driven_frame_by_frame() {
    let model = RiderModel::build(&RiderStyle::default());

    // What the renderer does once: one mesh per part, one surface per material.
    assert_eq!(model.parts().len(), PartId::COUNT);
    let mut triangles = 0;
    for (part, id) in model.parts().iter().zip(PartId::ALL) {
        assert_eq!(part.name(), id.name());
        for surface in part.surfaces() {
            assert_eq!(surface.positions.len(), surface.normals.len());
            assert_eq!(surface.indices_clockwise().len(), surface.indices.len());
            triangles += surface.triangle_count();
        }
    }
    assert_eq!(triangles, model.triangle_count());

    // What it does every frame: ten seconds at 60 Hz, 90 rpm, 30 km/h.
    let dt = 1.0 / 60.0;
    let cadence_rpm = 90.0;
    let speed_mps = 30.0 / 3.6;
    let mut crank = 0.0_f64;
    let mut distance_m = 0.0_f64;
    for frame in 0..600 {
        crank = (crank + cadence_rpm / 60.0 * TAU * dt) % TAU;
        distance_m += speed_mps * dt;
        let wheel = (distance_m / f64::from(WHEEL_RADIUS_M)) % TAU;
        let pose = model.pose(&PoseInput {
            crank_angle_rad: crank as f32,
            wheel_angle_rad: wheel as f32,
            steer_rad: 0.1 * MAX_STEER_RAD * (frame as f32 * 0.02).sin(),
            effort: 0.6,
            ..PoseInput::default()
        });
        for transform in &pose.transforms {
            assert!(transform.translation.iter().all(|c| c.is_finite()));
            assert!(transform.rotation.iter().all(|c| c.is_finite()));
            assert!(transform.to_cols().iter().flatten().all(|c| c.is_finite()));
        }
        // The feet go round the bottom bracket on the pedals.
        let bb = pose.anchors.bottom_bracket;
        for foot in [PartId::FootL, PartId::FootR] {
            let at = pose.transform(foot).translation;
            let radius = distance([0.0, at[1], at[2]], [0.0, bb[1], bb[2]]);
            assert!((radius - CRANK_LENGTH_M).abs() < 1.0e-3);
        }
        // The rider looks up the road from above the bars.
        assert!(pose.anchors.eye[1] > pose.anchors.hood_l[1] + 0.4);
        assert!(pose.anchors.eye_forward[2] < -0.9);
    }
}

#[test]
fn a_settings_menu_can_recolour_the_model() {
    let mut style = RiderStyle::default();
    let red = [0.85, 0.10, 0.12];
    style.set_color(StyleColor::Frame, red);
    style.jersey_primary = [0.1, 0.3, 0.7];
    let model = RiderModel::build(&style);
    let recoloured = |color: StyleColor, rgb: [f32; 3]| {
        model
            .parts()
            .iter()
            .flat_map(|part| part.surfaces())
            .filter(|surface| surface.style_color == Some(color))
            .all(|surface| surface.material.base_color == rgb)
    };
    assert!(recoloured(StyleColor::Frame, red));
    assert!(recoloured(StyleColor::JerseyPrimary, [0.1, 0.3, 0.7]));
    assert!(recoloured(StyleColor::Helmet, RiderStyle::default().helmet));
}

#[test]
fn telemetry_alone_can_drive_the_rider() {
    // What `Rider::place` in the game does: one sample per frame in, one
    // pose out. A climb at a grinding cadence, then a coasting descent.
    let model = RiderModel::build(&RiderStyle::default());
    let mut animator = RideAnimator::new();
    let dt = 1.0 / 60.0;
    let mut distance_m = 0.0_f64;
    let mut stood = false;
    let mut tucked = false;
    let mut cleat_reach = None;
    for frame in 0..(60 * 20) {
        let climbing = frame < 60 * 8;
        let speed_mps = if climbing { 3.5 } else { 17.0 };
        distance_m += speed_mps * dt;
        let input = animator.step(
            &RideSample {
                distance_m,
                cadence_rpm: Some(if climbing { 58.0 } else { 0.0 }),
                lean_rad: 0.2 * (frame as f32 * 0.01).sin(),
                grade_pct: if climbing { 9.5 } else { -7.0 },
                ..RideSample::default()
            },
            dt,
        );
        stood |= input.stand == 1.0;
        tucked |= input.tuck == 1.0;
        assert!(
            input.stand == 0.0 || input.tuck == 0.0,
            "one posture at a time"
        );
        assert!(input.steer_rad.abs() < MAX_STEER_RAD);
        let pose = model.pose(&input);
        // Whatever the posture, the feet stay on the pedals: each cleat
        // keeps its distance from the bottom bracket, also while the bike
        // rocks under a standing rider.
        let bb = pose.anchors.bottom_bracket;
        for foot in [PartId::FootL, PartId::FootR] {
            let reach = distance(pose.transform(foot).translation, bb);
            let expected = *cleat_reach.get_or_insert(reach);
            assert!(expected > CRANK_LENGTH_M && expected < CRANK_LENGTH_M + 0.06);
            assert!((reach - expected).abs() < 1.0e-3, "frame {frame}");
        }
    }
    assert!(stood, "the rider never got out of the saddle on the climb");
    assert!(tucked, "the rider never tucked on the descent");
    let expected_wheel = distance_m / f64::from(WHEEL_RADIUS_M);
    assert!((animator.wheel_angle_rad() - expected_wheel).abs() < 1.0e-9);
}
