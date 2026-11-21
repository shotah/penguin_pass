//! Tests of the built model and its pose.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec3};

use crate::check;
use crate::pose::solve;
use crate::rig::{
    ANKLE_LOCAL, BB_HEIGHT, CRANK_LENGTH, FOREARM, GRIP_LOCAL, PEDAL_X, SHIN, THIGH, UPPER_ARM,
    WHEEL_RADIUS,
};
use crate::{
    MAX_STEER_RAD, PartId, PartTransform, Pose, PoseInput, RiderModel, RiderStyle, StyleColor,
};

/// Triangle budget for the whole bike and rider.
const TRIANGLE_BUDGET: usize = 20_000;

fn model() -> RiderModel {
    RiderModel::build(&RiderStyle::default())
}

fn at_crank(deg: f32) -> PoseInput {
    PoseInput {
        crank_angle_rad: deg.to_radians(),
        ..PoseInput::default()
    }
}

/// The ways of riding that every leg test covers.
fn riding_modes() -> [(&'static str, PoseInput); 4] {
    [
        ("seated, easy", PoseInput::default()),
        (
            "seated, full effort",
            PoseInput {
                effort: 1.0,
                ..PoseInput::default()
            },
        ),
        (
            "standing",
            PoseInput {
                effort: 1.0,
                stand: 1.0,
                ..PoseInput::default()
            },
        ),
        (
            "tucked",
            PoseInput {
                tuck: 1.0,
                ..PoseInput::default()
            },
        ),
    ]
}

fn origin(pose: &Pose, id: PartId) -> Vec3 {
    Vec3::from_array(pose.transform(id).translation)
}

fn rotation(pose: &Pose, id: PartId) -> Quat {
    Quat::from_array(pose.transform(id).rotation)
}

fn place(pose: &Pose, id: PartId, local: Vec3) -> Vec3 {
    Vec3::from_array(pose.transform(id).transform_point(local.to_array()))
}

/// Angle between two rotations, in radians.
fn turn_between(a: Quat, b: Quat) -> f32 {
    let d = a.inverse() * b;
    2.0 * d.xyz().length().atan2(d.w.abs())
}

/// Distance from `p` to the segment from `a` to `b`.
fn distance_to_segment(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let along = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
    p.distance(a + (b - a) * along)
}

/// The parts of one leg: pedal, thigh, shin, foot. `side` is -1 left, +1 right.
fn leg(side: f32) -> [PartId; 4] {
    if side < 0.0 {
        [PartId::PedalL, PartId::ThighL, PartId::ShinL, PartId::FootL]
    } else {
        [PartId::PedalR, PartId::ThighR, PartId::ShinR, PartId::FootR]
    }
}

/// The parts of one arm: upper arm, forearm, hand.
fn arm(side: f32) -> [PartId; 3] {
    if side < 0.0 {
        [PartId::UpperArmL, PartId::ForearmL, PartId::HandL]
    } else {
        [PartId::UpperArmR, PartId::ForearmR, PartId::HandR]
    }
}

/// Hip, knee, and ankle of one leg, read back from the part transforms.
fn leg_joints(pose: &Pose, side: f32) -> (Vec3, Vec3, Vec3) {
    let [_, thigh, shin, _] = leg(side);
    (
        origin(pose, thigh),
        origin(pose, shin),
        place(pose, shin, Vec3::new(0.0, -SHIN, 0.0)),
    )
}

/// Angle at the knee between the thigh and the shin, in degrees. A straight
/// leg is 180.
fn knee_angle_deg(pose: &Pose, side: f32) -> f32 {
    let (hip, knee, ankle) = leg_joints(pose, side);
    (hip - knee).angle_between(ankle - knee).to_degrees()
}

/// Every vertex of the posed model, with the part it belongs to.
fn posed_vertices(model: &RiderModel, pose: &Pose) -> Vec<(PartId, Vec3)> {
    let mut out = Vec::new();
    for id in PartId::ALL {
        for surface in model.part(id).surfaces() {
            for p in &surface.positions {
                out.push((id, place(pose, id, Vec3::from_array(*p))));
            }
        }
    }
    out
}

#[test]
fn every_part_is_a_valid_mesh_of_closed_outward_shells() {
    let model = model();
    for part in model.parts() {
        assert!(!part.surfaces().is_empty(), "{}: no geometry", part.name());
        check::assert_part_valid(part);
        for shell in check::shells(part) {
            assert!(
                shell.closed,
                "{}: open shell of {} triangles near {:?}",
                part.name(),
                shell.triangles,
                shell.sample
            );
            assert!(
                shell.volume_m3 > 0.0,
                "{}: inward-facing shell of {} triangles near {:?}",
                part.name(),
                shell.triangles,
                shell.sample
            );
        }
    }
}

#[test]
fn parts_come_in_a_fixed_order_with_stable_names() {
    let model = model();
    let names: Vec<&str> = model.parts().iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        [
            "frame",
            "fork_bars",
            "front_wheel",
            "rear_wheel",
            "crankset",
            "pedal_l",
            "pedal_r",
            "pelvis",
            "torso",
            "head",
            "upper_arm_l",
            "upper_arm_r",
            "forearm_l",
            "forearm_r",
            "hand_l",
            "hand_r",
            "thigh_l",
            "thigh_r",
            "shin_l",
            "shin_r",
            "foot_l",
            "foot_r",
        ]
    );
    for (i, id) in PartId::ALL.into_iter().enumerate() {
        assert_eq!(id.index(), i);
        assert_eq!(model.part(id).name(), id.name());
        assert_eq!(id.is_bike(), i < 7);
    }
}

#[test]
fn triangle_count_is_within_budget() {
    let model = model();
    let total = model.triangle_count();
    let by_surfaces: usize = model
        .parts()
        .iter()
        .flat_map(|p| p.surfaces())
        .map(|s| s.indices.len() / 3)
        .sum();
    assert_eq!(total, by_surfaces);
    println!("triangles: {total} of {TRIANGLE_BUDGET}");
    assert!(total <= TRIANGLE_BUDGET, "{total} triangles is over budget");
    assert!(total > 8_000, "{total} triangles is suspiciously few");
}

#[test]
fn build_is_deterministic() {
    let style = RiderStyle::default();
    let a = RiderModel::build(&style);
    let b = RiderModel::build(&style);
    assert_eq!(a.parts(), b.parts());
    assert_eq!(RiderModel::default().parts(), a.parts());
}

#[test]
fn tyres_touch_the_road_and_nothing_dips_below_it() {
    let model = model();
    let rest = model.pose(&PoseInput::default());
    for id in [PartId::FrontWheel, PartId::RearWheel] {
        let lowest = posed_vertices(&model, &rest)
            .into_iter()
            .filter(|(part, _)| *part == id)
            .map(|(_, p)| p.y)
            .fold(f32::MAX, f32::min);
        assert!(
            lowest.abs() < 1.0e-4,
            "{}: lowest point {lowest}",
            id.name()
        );
    }
    for (name, mode) in riding_modes() {
        for step in 0..24 {
            let input = PoseInput {
                crank_angle_rad: (step as f32 * 15.0).to_radians(),
                wheel_angle_rad: step as f32 * 0.37,
                steer_rad: (step as f32 - 12.0) * 0.03,
                ..mode
            };
            let pose = model.pose(&input);
            for (part, p) in posed_vertices(&model, &pose) {
                assert!(
                    p.y > -2.0e-4,
                    "{name}, step {step}: {} dips to y = {}",
                    part.name(),
                    p.y
                );
            }
        }
    }
}

#[test]
fn bounding_box_is_a_road_bike_with_a_rider() {
    let model = model();
    let pose = model.pose(&PoseInput::default());
    let (mut min, mut max) = (Vec3::MAX, Vec3::MIN);
    for (_, p) in posed_vertices(&model, &pose) {
        min = min.min(p);
        max = max.max(p);
    }
    let size = max - min;
    println!("bounding box: min {min:?} max {max:?} size {size:?}");
    assert!(min.y.abs() < 1.0e-4, "the model stands on y = 0");
    assert!((0.44..0.62).contains(&size.x), "width {}", size.x);
    assert!((1.60..1.76).contains(&size.y), "height {}", size.y);
    assert!((1.64..1.72).contains(&size.z), "length {}", size.z);
    assert!(
        (min.x + max.x).abs() < 0.03,
        "the model is centred sideways"
    );
    assert!(
        min.z < -0.90 && max.z > 0.70,
        "the origin is under the cranks"
    );
}

#[test]
fn road_bike_dimensions_are_real() {
    let model = model();
    let pose = model.pose(&PoseInput::default());
    let a = pose.anchors;
    let (rear, front, bb) = (
        Vec3::from_array(a.rear_axle),
        Vec3::from_array(a.front_axle),
        Vec3::from_array(a.bottom_bracket),
    );
    assert_eq!(bb, Vec3::new(0.0, BB_HEIGHT, 0.0));
    assert!((BB_HEIGHT - 0.27).abs() < 1.0e-6);
    assert!((CRANK_LENGTH - 0.1725).abs() < 1.0e-6);
    assert!((rear.y - 0.34).abs() < 1.0e-5 && (front.y - 0.34).abs() < 1.0e-5);
    assert_eq!((rear.x, front.x), (0.0, 0.0));
    let wheelbase = rear.z - front.z;
    println!("wheelbase {wheelbase} m");
    assert!((0.97..1.02).contains(&wheelbase), "wheelbase {wheelbase}");
    assert!(front.z < 0.0 && rear.z > 0.0, "the bike faces -Z");

    let saddle = Vec3::from_array(a.saddle_top);
    let saddle_height = saddle.distance(bb);
    assert!(
        (0.72..0.76).contains(&saddle_height),
        "saddle {saddle_height}"
    );
    assert!((0.95..1.00).contains(&saddle.y));

    let (hood_l, hood_r) = (Vec3::from_array(a.hood_l), Vec3::from_array(a.hood_r));
    assert!(hood_l.x < 0.0 && hood_r.x > 0.0, "the rider's left is -X");
    assert!((hood_l + hood_r).x.abs() < 1.0e-6);
    assert!((0.38..0.42).contains(&(hood_r.x - hood_l.x)));
    let drop = saddle.y - hood_r.y;
    assert!((0.02..0.10).contains(&drop), "saddle-to-hood drop {drop}");

    // Each wheel is 0.34 m to the outside of its tyre.
    for id in [PartId::FrontWheel, PartId::RearWheel] {
        let radius = model
            .part(id)
            .surfaces()
            .iter()
            .flat_map(|s| &s.positions)
            .map(|p| p[1].hypot(p[2]))
            .fold(0.0, f32::max);
        assert!((radius - WHEEL_RADIUS).abs() < 1.0e-5, "{radius}");
    }
}

#[test]
fn crank_angle_zero_points_the_right_crank_forward() {
    let model = model();
    let bb = Vec3::new(0.0, BB_HEIGHT, 0.0);
    let right = |deg: f32| origin(&model.pose(&at_crank(deg)), PartId::PedalR) - bb;
    let left = |deg: f32| origin(&model.pose(&at_crank(deg)), PartId::PedalL) - bb;
    let close = |a: Vec3, b: Vec3| assert!(a.distance(b) < 1.0e-5, "{a:?} is not {b:?}");
    close(right(0.0), Vec3::new(PEDAL_X, 0.0, -CRANK_LENGTH));
    close(right(90.0), Vec3::new(PEDAL_X, -CRANK_LENGTH, 0.0));
    close(right(180.0), Vec3::new(PEDAL_X, 0.0, CRANK_LENGTH));
    close(right(270.0), Vec3::new(PEDAL_X, CRANK_LENGTH, 0.0));
    close(left(0.0), Vec3::new(-PEDAL_X, 0.0, CRANK_LENGTH));
    close(left(90.0), Vec3::new(-PEDAL_X, CRANK_LENGTH, 0.0));
}

#[test]
fn cleats_stay_on_the_pedal_axles() {
    let model = model();
    for (name, mode) in riding_modes() {
        for step in 0..360 {
            let pose = model.pose(&PoseInput {
                crank_angle_rad: (step as f32).to_radians(),
                ..mode
            });
            for side in [-1.0_f32, 1.0] {
                let [pedal, _, _, foot] = leg(side);
                // Where the crank arm, as the renderer draws it, holds the axle.
                let axle = place(
                    &pose,
                    PartId::Crankset,
                    Vec3::new(side * PEDAL_X, 0.0, -side * CRANK_LENGTH),
                );
                let cleat = origin(&pose, foot);
                assert!(
                    cleat.distance(axle) < 1.0e-3,
                    "{name}, {step} deg: cleat is {} m off the axle",
                    cleat.distance(axle)
                );
                assert!(origin(&pose, pedal).distance(axle) < 1.0e-3);
                assert!(turn_between(rotation(&pose, pedal), rotation(&pose, foot)) < 1.0e-4);
            }
        }
    }
}

#[test]
fn leg_segments_keep_their_length_and_meet_at_the_joints() {
    let model = model();
    for (name, mode) in riding_modes() {
        for step in 0..360 {
            let pose = model.pose(&PoseInput {
                crank_angle_rad: (step as f32).to_radians(),
                ..mode
            });
            for side in [-1.0_f32, 1.0] {
                let [_, thigh, _, foot] = leg(side);
                let (hip, knee, ankle) = leg_joints(&pose, side);
                let thigh_end = place(&pose, thigh, Vec3::new(0.0, -THIGH, 0.0));
                let foot_ankle = place(&pose, foot, ANKLE_LOCAL);
                let context = format!("{name}, {step} deg, side {side}");
                assert!(
                    (hip.distance(knee) - THIGH).abs() < 1.0e-4,
                    "{context}: thigh"
                );
                assert!(
                    (knee.distance(ankle) - SHIN).abs() < 1.0e-4,
                    "{context}: shin"
                );
                assert!(thigh_end.distance(knee) < 1.0e-4, "{context}: knee gap");
                assert!(ankle.distance(foot_ankle) < 1.0e-4, "{context}: ankle gap");
            }
        }
    }
}

#[test]
fn knee_angle_stays_in_a_cycling_range() {
    let model = model();
    for (name, mode) in riding_modes() {
        // Out of the saddle the leg straightens further at the bottom.
        let (low, high) = if mode.stand > 0.0 {
            (65.0, 165.0)
        } else {
            (65.0, 150.0)
        };
        let (mut min, mut max) = (f32::MAX, f32::MIN);
        for step in 0..360 {
            let pose = model.pose(&PoseInput {
                crank_angle_rad: (step as f32).to_radians(),
                ..mode
            });
            for side in [-1.0_f32, 1.0] {
                let angle = knee_angle_deg(&pose, side);
                assert!(
                    (low..=high).contains(&angle),
                    "{name}, {step} deg: knee angle {angle}"
                );
                min = min.min(angle);
                max = max.max(angle);
            }
        }
        println!("knee angle, {name}: {min:.1} to {max:.1} degrees");
        assert!(min < 82.0, "{name}: the knee never closes past {min}");
        assert!(max > 135.0, "{name}: the leg never opens past {max}");
    }
}

#[test]
fn knees_lead_forward_and_never_bow_out() {
    let model = model();
    for (name, mode) in riding_modes() {
        for step in 0..360 {
            let pose = model.pose(&PoseInput {
                crank_angle_rad: (step as f32).to_radians(),
                ..mode
            });
            let bike_right = rotation(&pose, PartId::Frame) * Vec3::X;
            for side in [-1.0_f32, 1.0] {
                let (hip, knee, ankle) = leg_joints(&pose, side);
                let line = (ankle - hip).normalize();
                let off_line = (knee - hip) - line * (knee - hip).dot(line);
                let context = format!("{name}, {step} deg, side {side}");
                // In front of the line from the hip to the ankle, never behind.
                assert!(
                    off_line.z < -0.03,
                    "{context}: knee {off_line:?} off the line"
                );
                // Sideways it stays near the line from the hip to the foot:
                // inside it when seated, and at most a little outside when
                // out of the saddle, where the knees open to clear the bike.
                let out = |p: Vec3| side * p.dot(bike_right);
                let widest = out(hip).max(out(ankle));
                let allowed = if mode.stand > 0.0 { 0.05 } else { 0.002 };
                assert!(out(knee) < widest + allowed, "{context}: knee bows out");
                assert!(out(knee) > 0.05, "{context}: knee crosses the bike");
            }
        }
    }
    // Over the top of the stroke the knee comes in a little.
    let top = model.pose(&at_crank(270.0));
    let bottom = model.pose(&at_crank(90.0));
    let (_, knee_top, _) = leg_joints(&top, 1.0);
    let (_, knee_bottom, _) = leg_joints(&bottom, 1.0);
    println!(
        "knee comes in {:.0} mm over the top of the stroke",
        (knee_bottom.x - knee_top.x) * 1000.0
    );
    assert!(knee_top.x < knee_bottom.x - 0.004);
}

#[test]
fn hips_rock_a_few_millimetres_on_the_saddle() {
    let model = model();
    for effort in [0.0, 1.0] {
        let hips: Vec<Vec3> = (0..360)
            .map(|step| {
                let pose = model.pose(&PoseInput {
                    effort,
                    ..at_crank(step as f32)
                });
                origin(&pose, PartId::ThighR)
            })
            .collect();
        let mean = hips.iter().copied().sum::<Vec3>() / hips.len() as f32;
        let drift = hips.iter().map(|h| h.distance(mean)).fold(0.0, f32::max);
        let low = hips.iter().map(|h| h.y).fold(f32::MAX, f32::min);
        let high = hips.iter().map(|h| h.y).fold(f32::MIN, f32::max);
        println!(
            "hip rock at effort {effort}: {:.1} mm",
            (high - low) * 1000.0
        );
        assert!(drift < 0.006, "hip wanders {drift} m from the saddle");
        assert!((0.002..0.010).contains(&(high - low)));
        // The hip drops on the side that is pushing down.
        let down = origin(&model.pose(&at_crank(90.0)), PartId::ThighR).y;
        let up = origin(&model.pose(&at_crank(270.0)), PartId::ThighR).y;
        assert!(down < up);
    }
}

#[test]
fn pose_is_continuous() {
    let model = model();
    for (name, mode) in riding_modes() {
        let pose_at = |step: usize| {
            model.pose(&PoseInput {
                crank_angle_rad: (step as f32).to_radians(),
                ..mode
            })
        };
        let mut before = pose_at(0);
        for step in 1..=360 {
            let after = pose_at(step);
            for id in PartId::ALL {
                let moved = origin(&before, id).distance(origin(&after, id));
                let turned = turn_between(rotation(&before, id), rotation(&after, id));
                assert!(
                    moved < 0.008,
                    "{name}: {} jumps {moved} m at {step} deg",
                    id.name()
                );
                assert!(
                    turned < 4.0_f32.to_radians(),
                    "{name}: {} snaps {} deg at {step} deg",
                    id.name(),
                    turned.to_degrees()
                );
            }
            before = after;
        }
    }
}

#[test]
fn left_and_right_legs_are_half_a_revolution_apart() {
    let model = model();
    let mirror_point = |p: Vec3| Vec3::new(-p.x, p.y, p.z);
    let mirror_turn = |q: Quat| Quat::from_xyzw(q.x, -q.y, -q.z, q.w);
    for step in 0..360 {
        let here = model.pose(&at_crank(step as f32));
        let half_on = model.pose(&PoseInput {
            crank_angle_rad: (step as f32).to_radians() + PI,
            ..PoseInput::default()
        });
        for (left, right) in leg(-1.0).into_iter().zip(leg(1.0)) {
            let gap = origin(&here, left).distance(mirror_point(origin(&half_on, right)));
            let turn = turn_between(
                rotation(&here, left),
                mirror_turn(rotation(&half_on, right)),
            );
            assert!(gap < 1.0e-4, "{} at {step} deg: {gap} m", left.name());
            assert!(turn < 1.0e-3, "{} at {step} deg: {turn} rad", left.name());
        }
    }
}

#[test]
fn pose_repeats_every_revolution() {
    let model = model();
    for (name, mode) in riding_modes() {
        for step in 0..72 {
            let a = (step as f32 * 5.0).to_radians();
            let once = model.pose(&PoseInput {
                crank_angle_rad: a,
                ..mode
            });
            let again = model.pose(&PoseInput {
                crank_angle_rad: a + TAU,
                ..mode
            });
            for id in PartId::ALL {
                assert!(
                    origin(&once, id).distance(origin(&again, id)) < 1.0e-4,
                    "{name}: {} differs after a revolution",
                    id.name()
                );
                assert!(turn_between(rotation(&once, id), rotation(&again, id)) < 2.0e-3);
            }
        }
    }
}

#[test]
fn wheels_turn_about_their_axles_by_the_wheel_angle() {
    let model = model();
    for steer in [0.0_f32, 0.2, -0.3] {
        let still = model.pose(&PoseInput {
            steer_rad: steer,
            ..PoseInput::default()
        });
        for step in 1..60 {
            let angle = step as f32 * 0.1;
            let pose = model.pose(&PoseInput {
                wheel_angle_rad: angle,
                steer_rad: steer,
                ..PoseInput::default()
            });
            for (id, axle) in [
                (PartId::RearWheel, pose.anchors.rear_axle),
                (PartId::FrontWheel, pose.anchors.front_axle),
            ] {
                // The hub stays on the axle.
                assert!(origin(&pose, id).distance(Vec3::from_array(axle)) < 1.0e-6);
                assert_eq!(origin(&pose, id), origin(&still, id));
                // The turn since the still pose is `angle` about the axle,
                // which is the wheel's own X axis; rolling forward turns it
                // about -X.
                let turn = rotation(&still, id).inverse() * rotation(&pose, id);
                let expected = Quat::from_axis_angle(Vec3::NEG_X, angle);
                assert!(
                    turn_between(turn, expected) < 1.0e-4,
                    "{} at {angle} rad",
                    id.name()
                );
            }
        }
    }
    // Rolling forward carries the top of the tyre toward -Z.
    let pose = model.pose(&PoseInput {
        wheel_angle_rad: 0.2,
        ..PoseInput::default()
    });
    let top = place(&pose, PartId::RearWheel, Vec3::new(0.0, WHEEL_RADIUS, 0.0));
    assert!(top.z < origin(&pose, PartId::RearWheel).z - 0.05);
}

#[test]
fn steering_turns_the_fork_about_the_head_tube() {
    let model = model();
    let rig = model.rig();
    let straight = model.pose(&PoseInput::default());
    for step in -9..=9 {
        let steer = step as f32 * 0.05;
        let pose = model.pose(&PoseInput {
            steer_rad: steer,
            ..PoseInput::default()
        });
        // Points on the steering axis do not move.
        for on_axis in [rig.head_bottom, rig.head_top] {
            let moved = place(&pose, PartId::ForkBars, on_axis);
            assert!(moved.distance(on_axis) < 1.0e-5);
        }
        let turn = rotation(&pose, PartId::ForkBars);
        let expected = Quat::from_axis_angle(rig.steer_axis, -steer);
        assert!(turn_between(turn, expected) < 1.0e-4);
        // The frame and rear wheel ignore it.
        assert_eq!(
            pose.transform(PartId::Frame),
            straight.transform(PartId::Frame)
        );
        assert_eq!(
            pose.transform(PartId::RearWheel),
            straight.transform(PartId::RearWheel)
        );
        // Positive steer is to the rider's right.
        let axle = Vec3::from_array(pose.anchors.front_axle);
        let hood = Vec3::from_array(pose.anchors.hood_r);
        if steer > 0.0 {
            assert!(axle.x > 0.0 && hood.z > straight.anchors.hood_r[2]);
        } else if steer < 0.0 {
            assert!(axle.x < 0.0 && hood.z < straight.anchors.hood_r[2]);
        }
    }
    let over = model.pose(&PoseInput {
        steer_rad: 2.0,
        ..PoseInput::default()
    });
    let limit = model.pose(&PoseInput {
        steer_rad: MAX_STEER_RAD,
        ..PoseInput::default()
    });
    assert_eq!(over, limit);
}

#[test]
fn hands_stay_on_the_hoods_when_steering() {
    let model = model();
    let rig = model.rig();
    let rest = model.pose(&PoseInput::default());
    let in_bar_space = |pose: &Pose, id: PartId| {
        let bars = rotation(pose, PartId::ForkBars);
        let offset = bars.inverse() * (origin(pose, id) - origin(pose, PartId::ForkBars));
        (offset, bars.inverse() * rotation(pose, id))
    };
    let steps = (2.0 * MAX_STEER_RAD / 0.01) as i32;
    let mut furthest_reach = 0.0_f32;
    for step in 0..=steps {
        let input = PoseInput {
            steer_rad: -MAX_STEER_RAD + step as f32 * 0.01,
            crank_angle_rad: step as f32 * 0.21,
            effort: 1.0,
            ..PoseInput::default()
        };
        let pose = model.pose(&input);
        let joints = solve(rig, &input).joints;
        for (i, side) in [-1.0_f32, 1.0].into_iter().enumerate() {
            let [upper, fore, hand] = arm(side);
            let context = format!("steer {:.2}, side {side}", input.steer_rad);
            // The hand does not move on the bar.
            let (offset, turn) = in_bar_space(&pose, hand);
            let (rest_offset, rest_turn) = in_bar_space(&rest, hand);
            assert!(
                offset.distance(rest_offset) < 1.0e-4,
                "{context}: hand slid"
            );
            assert!(
                turn_between(turn, rest_turn) < 1.0e-3,
                "{context}: hand twisted"
            );
            // It sits on its hood.
            let hood = Vec3::from_array(if side < 0.0 {
                pose.anchors.hood_l
            } else {
                pose.anchors.hood_r
            });
            let wrist = origin(&pose, hand);
            assert!(
                wrist.distance(hood) < 0.11,
                "{context}: wrist left the hood"
            );
            // The arm reaches it without stretching.
            let shoulder = origin(&pose, upper);
            let elbow = origin(&pose, fore);
            let upper_end = place(&pose, upper, Vec3::new(0.0, -UPPER_ARM, 0.0));
            let fore_end = place(&pose, fore, Vec3::new(0.0, -FOREARM, 0.0));
            assert!(
                (shoulder.distance(elbow) - UPPER_ARM).abs() < 1.0e-4,
                "{context}"
            );
            assert!(upper_end.distance(elbow) < 1.0e-4, "{context}: elbow gap");
            assert!(fore_end.distance(wrist) < 1.0e-4, "{context}: wrist gap");
            // The elbow hangs below the line from shoulder to wrist.
            assert!(elbow.y < shoulder.y.max(wrist.y), "{context}: elbow is up");
            furthest_reach = furthest_reach.max(joints.shoulder_reach[i]);
            assert_eq!(joints.shoulder[i], shoulder);
        }
    }
    println!("furthest shoulder reach when steering: {furthest_reach} m");
    assert!(furthest_reach < 0.05, "shoulder pulled {furthest_reach} m");
}

#[test]
fn arms_reach_the_bars_in_every_mode() {
    let model = model();
    let rig = model.rig();
    for (name, mode) in riding_modes() {
        let mut furthest_reach = 0.0_f32;
        for step in 0..360 {
            let input = PoseInput {
                crank_angle_rad: (step as f32).to_radians(),
                steer_rad: 0.1 * (step as f32 * 0.05).sin(),
                ..mode
            };
            let pose = model.pose(&input);
            let joints = solve(rig, &input).joints;
            for (i, side) in [-1.0_f32, 1.0].into_iter().enumerate() {
                let [upper, fore, hand] = arm(side);
                let (shoulder, elbow, wrist) = (
                    origin(&pose, upper),
                    origin(&pose, fore),
                    origin(&pose, hand),
                );
                let context = format!("{name}, {step} deg, side {side}");
                assert!(
                    (shoulder.distance(elbow) - UPPER_ARM).abs() < 1.0e-4,
                    "{context}: upper arm"
                );
                assert!(
                    (elbow.distance(wrist) - FOREARM).abs() < 1.0e-4,
                    "{context}: forearm"
                );
                // Bent, not locked straight and not folded up.
                let angle = (shoulder - elbow).angle_between(wrist - elbow).to_degrees();
                assert!((80.0..172.0).contains(&angle), "{context}: elbow {angle}");
                furthest_reach = furthest_reach.max(joints.shoulder_reach[i]);
            }
        }
        println!("shoulder reach, {name}: {:.0} mm", furthest_reach * 1000.0);
        assert!(
            furthest_reach < 0.05,
            "{name}: shoulder pulled {furthest_reach} m"
        );
    }
}

#[test]
fn elbows_are_softly_bent() {
    let model = model();
    let pose = model.pose(&PoseInput::default());
    for side in [-1.0_f32, 1.0] {
        let [upper, fore, hand] = arm(side);
        let (shoulder, elbow, wrist) = (
            origin(&pose, upper),
            origin(&pose, fore),
            origin(&pose, hand),
        );
        let angle = (shoulder - elbow).angle_between(wrist - elbow).to_degrees();
        println!("elbow angle, side {side}: {angle:.1} degrees");
        assert!((125.0..165.0).contains(&angle), "elbow angle {angle}");
    }
}

#[test]
fn upper_body_is_still_at_no_effort_and_sways_about_zero_with_effort() {
    let model = model();
    let upper_body = [
        PartId::Torso,
        PartId::Head,
        PartId::UpperArmL,
        PartId::UpperArmR,
        PartId::ForearmL,
        PartId::ForearmR,
        PartId::HandL,
        PartId::HandR,
    ];
    let rest = model.pose(&PoseInput::default());
    for step in 0..360 {
        let pose = model.pose(&at_crank(step as f32));
        for id in upper_body {
            assert_eq!(
                pose.transform(id),
                rest.transform(id),
                "{} moves at no effort",
                id.name()
            );
        }
        assert_eq!(pose.anchors.eye, rest.anchors.eye);
    }

    // With effort: the shoulders swing side to side about their rest
    // position, further the harder the rider works.
    let swing = |effort: f32| -> (f32, f32, f32) {
        let (mut sum_x, mut sum_roll, mut widest) = (0.0, 0.0, 0.0_f32);
        for step in 0..360 {
            let pose = model.pose(&PoseInput {
                effort,
                ..at_crank(step as f32)
            });
            let neck = place(&pose, PartId::Torso, Vec3::new(0.0, 0.40, -0.14));
            let rest_neck = place(&rest, PartId::Torso, Vec3::new(0.0, 0.40, -0.14));
            let across = rotation(&pose, PartId::Torso) * Vec3::X;
            sum_x += neck.x - rest_neck.x;
            sum_roll += across.y;
            widest = widest.max((neck.x - rest_neck.x).abs());
        }
        (sum_x / 360.0, sum_roll / 360.0, widest)
    };
    let (mean_x, mean_roll, half) = swing(0.5);
    let (mean_x_full, mean_roll_full, full) = swing(1.0);
    println!("shoulder sway: {half:.4} m at half effort, {full:.4} m at full");
    assert!(mean_x.abs() < 1.0e-4 && mean_x_full.abs() < 1.0e-4);
    assert!(mean_roll.abs() < 1.0e-4 && mean_roll_full.abs() < 1.0e-4);
    assert!(half > 0.004 && full > 1.8 * half && full < 0.04);

    // The shoulders dip toward the leg that is pushing down.
    let push_right = model.pose(&PoseInput {
        effort: 1.0,
        ..at_crank(90.0)
    });
    let across = rotation(&push_right, PartId::Torso) * Vec3::X;
    assert!(
        across.y < 0.0,
        "right shoulder should dip on the right downstroke"
    );

    // The head goes with the shoulders but keeps facing up the road.
    let rest_forward = Vec3::from_array(rest.anchors.eye_forward);
    assert!(rest_forward.z < -0.98 && rest_forward.x == 0.0);
    for step in 0..360 {
        let pose = model.pose(&PoseInput {
            effort: 1.0,
            steer_rad: 0.2,
            ..at_crank(step as f32)
        });
        let forward = Vec3::from_array(pose.anchors.eye_forward);
        assert!(forward.angle_between(rest_forward) < 1.0_f32.to_radians());
        assert!((forward.length() - 1.0).abs() < 1.0e-5);
    }
}

#[test]
fn effort_stand_and_tuck_are_clamped() {
    let model = model();
    let base = at_crank(40.0);
    let limit = model.pose(&PoseInput {
        effort: 1.0,
        stand: 1.0,
        ..base
    });
    let over = model.pose(&PoseInput {
        effort: 7.0,
        stand: 3.0,
        ..base
    });
    assert_eq!(over, limit);
    let under = model.pose(&PoseInput {
        effort: -2.0,
        stand: -1.0,
        tuck: -1.0,
        ..base
    });
    assert_eq!(under, model.pose(&base));
    // Standing wins over the tuck.
    let both = model.pose(&PoseInput {
        stand: 1.0,
        tuck: 1.0,
        ..base
    });
    let stand = model.pose(&PoseInput { stand: 1.0, ..base });
    assert_eq!(both, stand);
}

#[test]
fn standing_lifts_the_hips_and_rocks_the_bike_against_the_downstroke() {
    let model = model();
    let seated = model.pose(&PoseInput::default());
    let seated_hips = (origin(&seated, PartId::ThighL) + origin(&seated, PartId::ThighR)) * 0.5;
    let mut sum_lean = 0.0;
    for step in 0..360 {
        let pose = model.pose(&PoseInput {
            stand: 1.0,
            effort: 1.0,
            ..at_crank(step as f32)
        });
        let hips = (origin(&pose, PartId::ThighL) + origin(&pose, PartId::ThighR)) * 0.5;
        assert!(
            hips.y > seated_hips.y + 0.015,
            "hips stay up off the saddle"
        );
        assert!(hips.z < seated_hips.z - 0.10, "hips come forward");
        // Every bike part leans by the same amount.
        let lean = (rotation(&pose, PartId::Frame) * Vec3::Y).x;
        for id in [PartId::RearWheel, PartId::Crankset] {
            let part_lean = (rotation(&pose, id) * Vec3::X).y;
            assert!((part_lean + lean).abs() < 1.0e-5);
        }
        sum_lean += lean;
        assert!(lean.abs() < 0.13);
    }
    assert!(
        (sum_lean / 360.0_f32).abs() < 1.0e-4,
        "the rocking is zero-mean"
    );
    // Right pedal going down: the bike leans left, away from that leg.
    let right_down = model.pose(&PoseInput {
        stand: 1.0,
        ..at_crank(90.0)
    });
    assert!((rotation(&right_down, PartId::Frame) * Vec3::Y).x < -0.08);
    let left_down = model.pose(&PoseInput {
        stand: 1.0,
        ..at_crank(270.0)
    });
    assert!((rotation(&left_down, PartId::Frame) * Vec3::Y).x > 0.08);
    // Half-way into the stand is half-way between.
    let half = model.pose(&PoseInput {
        stand: 0.5,
        ..PoseInput::default()
    });
    let full = model.pose(&PoseInput {
        stand: 1.0,
        ..PoseInput::default()
    });
    let hip = |pose: &Pose| origin(pose, PartId::Pelvis);
    assert!(hip(&half).distance((hip(&seated) + hip(&full)) * 0.5) < 1.0e-5);
}

#[test]
fn tuck_puts_the_hands_in_the_drops_and_flattens_the_back() {
    let model = model();
    let hoods = model.pose(&PoseInput::default());
    let tuck = model.pose(&PoseInput {
        tuck: 1.0,
        ..PoseInput::default()
    });
    for side in [-1.0_f32, 1.0] {
        let [upper, _, hand] = arm(side);
        let hood = Vec3::from_array(if side < 0.0 {
            tuck.anchors.hood_l
        } else {
            tuck.anchors.hood_r
        });
        // The closed hand is wrapped round the bar, well below the hood.
        let grip = place(&tuck, hand, GRIP_LOCAL);
        assert!(
            grip.y < hood.y - 0.06,
            "the hand is below the hood, in the drops"
        );
        let to_bar = model
            .part(PartId::ForkBars)
            .surfaces()
            .iter()
            .filter(|s| s.style_color == Some(StyleColor::BarTape))
            .flat_map(|s| &s.positions)
            .map(|p| place(&tuck, PartId::ForkBars, Vec3::from_array(*p)).distance(grip))
            .fold(f32::MAX, f32::min);
        assert!(to_bar < 0.02, "the grip is {to_bar} m from the bar tape");
        assert!(
            origin(&tuck, upper).y < origin(&hoods, upper).y - 0.06,
            "the shoulders come down"
        );
    }
    let spine = |pose: &Pose| rotation(pose, PartId::Torso) * Vec3::Y;
    assert!(
        spine(&tuck).y < spine(&hoods).y - 0.15,
        "the back is flatter"
    );
    let eye = |pose: &Pose| Vec3::from_array(pose.anchors.eye);
    assert!(eye(&tuck).y < eye(&hoods).y - 0.08, "the head comes down");
    let forward = Vec3::from_array(tuck.anchors.eye_forward);
    assert!(forward.z < -0.98, "the head still faces up the road");
    // Cranks level if the caller holds them level.
    let (left, right) = (origin(&tuck, PartId::PedalL), origin(&tuck, PartId::PedalR));
    assert!((left.y - right.y).abs() < 1.0e-5);
}

#[test]
fn anchors_sit_on_the_model() {
    let model = model();
    let rig = model.rig();
    let pose = model.pose(&PoseInput {
        steer_rad: 0.15,
        effort: 0.7,
        ..at_crank(120.0)
    });
    let a = pose.anchors;
    assert_eq!(Vec3::from_array(a.saddle_top), rig.saddle_top);
    assert_eq!(Vec3::from_array(a.rear_axle), rig.rear_axle);
    assert_eq!(Vec3::from_array(a.bottom_bracket), rig.bb);
    assert_eq!(a.front_axle, pose.transform(PartId::FrontWheel).translation);
    assert_eq!(
        Vec3::from_array(a.hood_r),
        place(&pose, PartId::ForkBars, rig.hood(1.0))
    );
    assert_eq!(
        Vec3::from_array(a.hood_l),
        place(&pose, PartId::ForkBars, rig.hood(-1.0))
    );
    // The eye is inside the head's bounds, at the front, above the bars.
    let eye = Vec3::from_array(a.eye);
    let head = origin(&pose, PartId::Head);
    assert!(eye.distance(head) < 0.2 && eye.y > head.y && eye.z < head.z);
    assert!(eye.y > 1.45 && eye.y < 1.70);
    // The hood grip points are on the surface of the hoods: some vertex of
    // the bars is within a few millimetres.
    let nearest = model
        .part(PartId::ForkBars)
        .surfaces()
        .iter()
        .flat_map(|s| &s.positions)
        .map(|p| Vec3::from_array(*p).distance(rig.hood(1.0)))
        .fold(f32::MAX, f32::min);
    assert!(nearest < 0.012, "hood anchor is {nearest} m from the bars");
    // The saddle anchor is level with the top of the saddle's middle.
    let top = model
        .part(PartId::Frame)
        .surfaces()
        .iter()
        .flat_map(|s| &s.positions)
        .map(|p| Vec3::from_array(*p))
        .filter(|p| p.x.abs() < 0.03 && (p.z - rig.saddle_top.z).abs() < 0.04)
        .map(|p| p.y)
        .fold(f32::MIN, f32::max);
    assert!(
        (top - rig.saddle_top.y).abs() < 0.004,
        "saddle top is at {top}"
    );
}

#[test]
fn default_kit_colours_land_on_the_expected_parts() {
    let style = RiderStyle::default();
    let model = RiderModel::build(&style);
    let expected: [(PartId, &[StyleColor]); 14] = [
        (PartId::Frame, &[StyleColor::Frame]),
        (PartId::ForkBars, &[StyleColor::Frame, StyleColor::BarTape]),
        (
            PartId::FrontWheel,
            &[StyleColor::Tyre, StyleColor::TyreWall],
        ),
        (PartId::RearWheel, &[StyleColor::Tyre, StyleColor::TyreWall]),
        (PartId::Pelvis, &[StyleColor::Bibs]),
        (
            PartId::Torso,
            &[
                StyleColor::JerseyPrimary,
                StyleColor::JerseySecondary,
                StyleColor::JerseyAccent,
                StyleColor::Skin,
            ],
        ),
        (PartId::Head, &[StyleColor::Helmet, StyleColor::Skin]),
        (
            PartId::UpperArmR,
            &[
                StyleColor::JerseyPrimary,
                StyleColor::JerseyAccent,
                StyleColor::Skin,
            ],
        ),
        (PartId::ForearmL, &[StyleColor::Skin]),
        (PartId::HandR, &[StyleColor::Skin]),
        (PartId::ThighL, &[StyleColor::Bibs, StyleColor::Skin]),
        (PartId::ShinR, &[StyleColor::Skin, StyleColor::Socks]),
        (PartId::FootL, &[StyleColor::Shoes]),
        (PartId::FootR, &[StyleColor::Shoes]),
    ];
    for (id, colors) in expected {
        for &color in colors {
            let surface = model
                .part(id)
                .surfaces()
                .iter()
                .find(|s| s.style_color == Some(color))
                .unwrap_or_else(|| panic!("{} has no {color:?} surface", id.name()));
            assert_eq!(surface.material.base_color, style.color(color));
            assert!(surface.triangle_count() > 0);
        }
    }
    // Every surface tagged with a style colour carries that colour, and
    // every style colour is used somewhere.
    for color in StyleColor::ALL {
        let mut used = false;
        for surface in model.parts().iter().flat_map(|p| p.surfaces()) {
            if surface.style_color == Some(color) {
                used = true;
                assert_eq!(surface.material.base_color, style.color(color));
            }
        }
        assert!(used, "{color:?} is not used by any surface");
    }
    // The penguin kit: black back, white front, orange trim.
    assert!(style.jersey_primary.iter().all(|c| *c < 0.15));
    assert!(style.jersey_secondary.iter().all(|c| *c > 0.9));
    assert!(style.jersey_accent[0] > 0.9 && style.jersey_accent[2] < 0.2);
}

#[test]
fn recolouring_changes_only_the_surfaces_of_that_colour() {
    let base_style = RiderStyle::default();
    let base = RiderModel::build(&base_style);
    let new_rgb = [0.31, 0.72, 0.13];
    for color in StyleColor::ALL {
        let mut style = base_style;
        style.set_color(color, new_rgb);
        assert_eq!(style.color(color), new_rgb);
        let model = RiderModel::build(&style);
        let mut changed = 0;
        for (before, after) in base.parts().iter().zip(model.parts()) {
            assert_eq!(before.surfaces().len(), after.surfaces().len());
            for (b, a) in before.surfaces().iter().zip(after.surfaces()) {
                assert_eq!(b.positions, a.positions, "{color:?} moved geometry");
                assert_eq!(b.normals, a.normals);
                assert_eq!(b.indices, a.indices);
                assert_eq!(b.style_color, a.style_color);
                assert_eq!(b.material.roughness, a.material.roughness);
                assert_eq!(b.material.metallic, a.material.metallic);
                if b.style_color == Some(color) {
                    assert_eq!(a.material.base_color, new_rgb);
                    changed += 1;
                } else {
                    assert_eq!(
                        b.material.base_color,
                        a.material.base_color,
                        "{color:?} recoloured a {:?} surface of {}",
                        b.style_color,
                        before.name()
                    );
                }
            }
        }
        assert!(changed > 0, "{color:?} changed nothing");
    }
}

#[test]
fn transform_helpers_agree() {
    let model = model();
    let pose = model.pose(&PoseInput {
        steer_rad: 0.2,
        effort: 1.0,
        ..at_crank(77.0)
    });
    let local = [0.03, -0.2, 0.11];
    for transform in &pose.transforms {
        let [x, y, z, o] = transform.to_cols().map(Vec3::from_array);
        let by_cols = o + x * local[0] + y * local[1] + z * local[2];
        let direct = Vec3::from_array(transform.transform_point(local));
        assert!(by_cols.distance(direct) < 1.0e-5);
        let turned = Vec3::from_array(transform.transform_vector(local));
        assert!((turned + o).distance(direct) < 1.0e-5);
        // A rigid transform: orthonormal, right-handed.
        assert!((x.length() - 1.0).abs() < 1.0e-5);
        assert!(x.cross(y).distance(z) < 1.0e-5);
        let q = Quat::from_array(transform.rotation);
        assert!((q.length() - 1.0).abs() < 1.0e-5);
    }
    assert_eq!(
        PartTransform::IDENTITY.transform_point(local),
        local,
        "identity leaves a point alone"
    );
    assert_eq!(pose.transform(PartId::Frame), &PartTransform::IDENTITY);
}

#[test]
fn clockwise_indices_reverse_each_triangle() {
    let model = model();
    let surface = &model.part(PartId::Frame).surfaces()[0];
    let flipped = surface.indices_clockwise();
    assert_eq!(flipped.len(), surface.indices.len());
    let before = surface.indices.as_chunks::<3>().0;
    let after = flipped.as_chunks::<3>().0;
    for (a, b) in before.iter().zip(after) {
        assert_eq!([a[0], a[2], a[1]], *b);
    }
}

#[test]
fn the_rider_clears_the_bike() {
    // Not a full collision test: the places a cyclist would notice.
    let model = model();
    let rig = model.rig();
    for (name, mode) in riding_modes() {
        let mut closest = f32::MAX;
        for step in 0..360 {
            let pose = model.pose(&PoseInput {
                crank_angle_rad: (step as f32).to_radians(),
                ..mode
            });
            let bike_right = rotation(&pose, PartId::Frame) * Vec3::X;
            let bike_origin = origin(&pose, PartId::Frame);
            // The top tube, from the seat tube to the head tube.
            let tube_a = place(&pose, PartId::Frame, rig.seat_top);
            let tube_b = place(&pose, PartId::Frame, rig.head_top);
            for side in [-1.0_f32, 1.0] {
                let (_, knee, ankle) = leg_joints(&pose, side);
                // Knee (0.057 m round) clear of the top tube (0.022 m round).
                let gap = distance_to_segment(knee, tube_a, tube_b) - 0.057 - 0.022;
                closest = closest.min(gap);
                assert!(
                    gap > 0.0,
                    "{name}, {step} deg: knee is {gap} m into the tube"
                );
                // Ankle (0.042 m round) outside the crank arm's face.
                let out = side * (ankle - bike_origin).dot(bike_right);
                assert!(
                    out - 0.042 > 0.076,
                    "{name}, {step} deg: ankle on the crank"
                );
            }
        }
        println!("knee to top tube, {name}: {:.0} mm clear", closest * 1000.0);
    }
}
