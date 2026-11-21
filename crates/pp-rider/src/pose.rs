//! Posing: one rigid transform per part for a given crank angle.

use std::f32::consts::PI;

use glam::{Mat3, Quat, Vec3};

use crate::mesh::PartId;
use crate::rig::{
    ANKLE_LOCAL, CRANK_LENGTH, DROP_FROM_CLAMP, DROP_SLOPE, EYE_LOCAL, FOREARM, Frame, GRIP_LOCAL,
    HEAD_PIVOT_LOCAL, HIP_HALF, HOOD_FROM_CLAMP, PEDAL_X, Rig, SHIN, SHOULDER_LOCAL, THIGH,
    UPPER_ARM, WAIST_UP,
};

/// Largest steering angle the pose accepts, in radians (about 26 degrees).
/// [`PoseInput::steer_rad`] is clamped to plus or minus this.
pub const MAX_STEER_RAD: f32 = 0.45;

/// Hip rock at no effort and the extra at full effort, in radians.
const HIP_ROCK: (f32, f32) = (0.022, 0.030);
/// Torso roll at full effort, in radians.
const SWAY_ROLL: f32 = 0.055;
/// Torso yaw at full effort, in radians.
const SWAY_YAW: f32 = 0.030;
/// Torso nod at full effort, in radians. It runs at twice the cadence.
const SWAY_NOD: f32 = 0.014;
/// How much of the steering angle the shoulders follow.
const STEER_FOLLOW: f32 = 0.45;
/// Bike roll at full stand, in radians, away from the pushing leg.
const STAND_ROCK: f32 = 0.085;
/// Fraction of the straight arm length the elbow is allowed to reach.
const ARM_REACH: f32 = 0.985;

/// What the rider and bike are doing this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PoseInput {
    /// Crank rotation, in radians.
    ///
    /// At 0 the right crank is horizontal and points forward (3 o'clock seen
    /// from the drive side) and the left crank points back. Positive is
    /// forward pedalling: at `PI / 2` the right pedal is at the bottom, at
    /// `PI` it points back, at `3 * PI / 2` it is at the top. The pose
    /// repeats every `2 * PI`.
    pub crank_angle_rad: f32,
    /// Wheel rotation, in radians. Positive rolls the bike forward: use
    /// `distance_m / 0.34`. Keep the value wrapped (for example to
    /// `0..2 * PI`, in `f64`, before the cast) so `f32` keeps its precision
    /// on a long ride.
    pub wheel_angle_rad: f32,
    /// Steering angle about the head-tube axis, in radians. Positive turns
    /// the bars to the rider's right. Clamped to plus or minus
    /// [`MAX_STEER_RAD`].
    pub steer_rad: f32,
    /// How hard the rider is working, 0..=1. 0 is soft-pedalling with a
    /// still upper body; 1 is a full effort with the shoulders rocking.
    /// Values outside the range are clamped.
    pub effort: f32,
    /// Out-of-saddle blend, 0..=1. 0 is seated; 1 is standing on the pedals
    /// with the bike rocking under the rider. Clamped.
    pub stand: f32,
    /// Descending tuck blend, 0..=1. 0 is on the hoods; 1 is hands in the
    /// drops with a flat back. Clamped, and scaled by `1 - stand`, so
    /// standing wins when both are set.
    pub tuck: f32,
}

/// Where one part sits: rotate by `rotation`, then move by `translation`.
/// Maps the part's local space to model space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartTransform {
    /// Position of the part's origin in model space, in meters.
    pub translation: [f32; 3],
    /// Unit quaternion as `[x, y, z, w]`.
    pub rotation: [f32; 4],
}

impl PartTransform {
    /// The transform that leaves a part where it was built.
    pub const IDENTITY: PartTransform = PartTransform {
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
    };

    /// Map a point from part-local space to model space, in meters.
    #[must_use]
    pub fn transform_point(&self, local: [f32; 3]) -> [f32; 3] {
        let rotated = Quat::from_array(self.rotation) * Vec3::from_array(local);
        (rotated + Vec3::from_array(self.translation)).to_array()
    }

    /// Rotate a direction from part-local space to model space.
    #[must_use]
    pub fn transform_vector(&self, local: [f32; 3]) -> [f32; 3] {
        (Quat::from_array(self.rotation) * Vec3::from_array(local)).to_array()
    }

    /// The transform as four columns: the X, Y, and Z basis vectors, then
    /// the origin. This is the argument order of Godot's
    /// `Transform3D::from_cols`.
    #[must_use]
    pub fn to_cols(&self) -> [[f32; 3]; 4] {
        let m = Mat3::from_quat(Quat::from_array(self.rotation));
        [
            m.x_axis.to_array(),
            m.y_axis.to_array(),
            m.z_axis.to_array(),
            self.translation,
        ]
    }
}

impl From<Frame> for PartTransform {
    fn from(frame: Frame) -> Self {
        Self {
            translation: frame.pos.to_array(),
            rotation: frame.rot.normalize().to_array(),
        }
    }
}

/// Named points on the posed model, in model space, in meters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchors {
    /// Middle of the saddle's top surface.
    pub saddle_top: [f32; 3],
    /// Grip point on top of the left brake hood.
    pub hood_l: [f32; 3],
    /// Grip point on top of the right brake hood.
    pub hood_r: [f32; 3],
    /// Between the rider's eyes. Put the first-person camera here.
    pub eye: [f32; 3],
    /// Unit vector the head faces along. Aim the first-person camera
    /// down it, or a little below it to see more of the bars.
    pub eye_forward: [f32; 3],
    /// Centre of the rear axle.
    pub rear_axle: [f32; 3],
    /// Centre of the front axle.
    pub front_axle: [f32; 3],
    /// Centre of the bottom bracket.
    pub bottom_bracket: [f32; 3],
}

/// The whole model posed for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// One transform per part, indexed by [`PartId::index`], in the same
    /// order as [`RiderModel::parts`](crate::RiderModel::parts).
    pub transforms: [PartTransform; PartId::COUNT],
    /// Named points for cameras and effects.
    pub anchors: Anchors,
}

impl Pose {
    /// The transform of one part.
    #[must_use]
    pub fn transform(&self, part: PartId) -> &PartTransform {
        &self.transforms[part.index()]
    }
}

/// What the solver knows about the shoulders that the part transforms do
/// not show, for the tests. Index 0 is the left side and 1 the right.
#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Joints {
    /// Shoulder joints, in model space.
    pub shoulder: [Vec3; 2],
    /// How far each shoulder was pulled toward the hand to keep it on the
    /// bar, in meters.
    pub shoulder_reach: [f32; 2],
}

/// A solved pose before it is flattened into the public arrays.
pub(crate) struct Solved {
    pub frames: [Frame; PartId::COUNT],
    pub anchors: Anchors,
    /// Read by the tests only.
    #[cfg_attr(not(test), allow(dead_code))]
    pub joints: Joints,
}

impl Solved {
    pub fn into_pose(self) -> Pose {
        Pose {
            transforms: self.frames.map(PartTransform::from),
            anchors: self.anchors,
        }
    }
}

/// Body position that the stand and tuck blends move between.
#[derive(Clone, Copy)]
struct Posture {
    /// Mid-hip point, in model space.
    hip: Vec3,
    /// Forward tilt of the pelvis from upright, in radians.
    pelvis_pitch: f32,
    /// Forward tilt of the torso from upright, in radians.
    torso_pitch: f32,
    /// Downward tilt of the head, in radians.
    head_pitch: f32,
}

impl Posture {
    fn seated(rig: &Rig) -> Self {
        Self {
            hip: rig.hip_seated,
            pelvis_pitch: 26.0_f32.to_radians(),
            torso_pitch: 37.0_f32.to_radians(),
            head_pitch: 7.0_f32.to_radians(),
        }
    }

    fn standing(rig: &Rig) -> Self {
        Self {
            hip: rig.hip_seated + Vec3::new(0.0, 0.050, -0.165),
            pelvis_pitch: 18.0_f32.to_radians(),
            torso_pitch: 37.0_f32.to_radians(),
            head_pitch: 9.0_f32.to_radians(),
        }
    }

    fn tucked(rig: &Rig) -> Self {
        Self {
            hip: rig.hip_seated + Vec3::new(0.0, -0.004, 0.022),
            pelvis_pitch: 40.0_f32.to_radians(),
            torso_pitch: 55.0_f32.to_radians(),
            head_pitch: 2.0_f32.to_radians(),
        }
    }

    fn toward(self, other: Posture, amount: f32) -> Posture {
        Posture {
            hip: self.hip.lerp(other.hip, amount),
            pelvis_pitch: self.pelvis_pitch + (other.pelvis_pitch - self.pelvis_pitch) * amount,
            torso_pitch: self.torso_pitch + (other.torso_pitch - self.torso_pitch) * amount,
            head_pitch: self.head_pitch + (other.head_pitch - self.head_pitch) * amount,
        }
    }
}

/// Toe-down angle of the foot, in radians, for a crank at `theta`.
///
/// The foot is nearly level through the power phase and points down most
/// late in the upstroke, the way riders ankle.
fn toe_down(theta: f32, stand: f32) -> f32 {
    let mean = 16.0_f32.to_radians() + stand * 6.0_f32.to_radians();
    let swing = 11.0_f32.to_radians();
    mean + swing * (theta - 195.0_f32.to_radians()).cos()
}

/// Where a two-bone chain bends.
///
/// `root` and `target` are the ends, `upper` and `lower` the bone lengths.
/// The joint lands on the side of the root-target line that `pole` points
/// to. Returns the joint and the unit direction from the line to it. A
/// target out of reach is treated as being at full stretch.
fn two_bone(root: Vec3, target: Vec3, upper: f32, lower: f32, pole: Vec3) -> (Vec3, Vec3) {
    let to_target = target - root;
    let min = (upper - lower).abs() + 1.0e-4;
    let max = upper + lower - 1.0e-5;
    let distance = to_target.length().clamp(min, max);
    let dir = to_target.normalize_or(Vec3::NEG_Y);
    let out = (pole - dir * pole.dot(dir))
        .try_normalize()
        .unwrap_or_else(|| dir.any_orthonormal_vector());
    let along = (upper * upper + distance * distance - lower * lower) / (2.0 * distance);
    let across = (upper * upper - along * along).max(0.0).sqrt();
    (root + dir * along + out * across, out)
}

/// Placement of a limb segment whose mesh hangs down -Y from its origin.
///
/// `front_out` says which face of the mesh points out of the bend: the front
/// (-Z) for a leg, whose knee leads, or the back (+Z) for an arm, whose
/// elbow trails.
fn bone(from: Vec3, to: Vec3, out: Vec3, front_out: bool) -> Frame {
    let y = (from - to).normalize();
    let out = (out - y * out.dot(y)).normalize();
    let z = if front_out { -out } else { out };
    let x = y.cross(z);
    Frame::new(from, Quat::from_mat3(&Mat3::from_cols(x, y, z)))
}

/// Placement of the right hand relative to the bar clamp, bars straight.
/// The hand's origin is the wrist.
fn hand_on_bar(tuck: f32) -> Frame {
    // On the hood: the wrist behind and above it, the palm turned inward,
    // the fingers round the hood's body.
    let hood = Frame::new(
        HOOD_FROM_CLAMP + Vec3::new(0.022, 0.030, 0.086),
        Quat::from_rotation_y(0.14) * Quat::from_rotation_x(-0.20) * Quat::from_rotation_z(-0.95),
    );
    if tuck <= 0.0 {
        return hood;
    }
    // In the drops: the fist closed round the front of the hook, where the
    // brake lever is in reach, knuckles along the bar, the palm inward.
    let (sin, cos) = DROP_SLOPE.sin_cos();
    let along_bar = Vec3::new(0.0, -sin, cos);
    let grip = Quat::from_axis_angle(along_bar, 0.15)
        * Quat::from_mat3(&Mat3::from_cols(
            along_bar,
            Vec3::X,
            along_bar.cross(Vec3::X),
        ));
    let drop = Frame::new(DROP_FROM_CLAMP - grip * GRIP_LOCAL, grip);
    Frame::new(
        hood.pos.lerp(drop.pos, tuck),
        hood.rot.slerp(drop.rot, tuck),
    )
}

/// Solve the pose.
pub(crate) fn solve(rig: &Rig, input: &PoseInput) -> Solved {
    let crank = input.crank_angle_rad;
    let effort = input.effort.clamp(0.0, 1.0);
    let stand = input.stand.clamp(0.0, 1.0);
    let tuck = input.tuck.clamp(0.0, 1.0) * (1.0 - stand);
    let steer = input.steer_rad.clamp(-MAX_STEER_RAD, MAX_STEER_RAD);
    let sin1 = crank.sin();
    let cos2 = (2.0 * crank).cos();

    let mut frames = [Frame::IDENTITY; PartId::COUNT];

    // The bike. Out of the saddle it rocks on its round tyres, away from the
    // leg that is pushing down.
    let bike_roll = -stand * STAND_ROCK * sin1;
    let bike = rig.rolled(bike_roll);
    let steering = bike.then(rig.steering(steer));
    let spin = Quat::from_rotation_x(-input.wheel_angle_rad);
    frames[PartId::Frame.index()] = bike;
    frames[PartId::ForkBars.index()] = steering;
    frames[PartId::FrontWheel.index()] = steering.then(Frame::new(rig.front_axle, spin));
    frames[PartId::RearWheel.index()] = bike.then(Frame::new(rig.rear_axle, spin));
    frames[PartId::Crankset.index()] = bike.then(Frame::new(rig.bb, Quat::from_rotation_x(-crank)));

    // Posture, with the standing bob: highest when a leg is straight.
    let mut posture = Posture::seated(rig)
        .toward(Posture::standing(rig), stand)
        .toward(Posture::tucked(rig), tuck);
    posture.hip.y -= stand * 0.016 * cos2;

    // Pelvis: the hip on the pushing side drops a few millimetres.
    let rock = (HIP_ROCK.0 + HIP_ROCK.1 * effort) * sin1 * (1.0 - 0.5 * stand);
    let pelvis_pitch = Quat::from_rotation_x(-posture.pelvis_pitch);
    let pelvis = Frame::new(posture.hip, Quat::from_rotation_z(-rock) * pelvis_pitch);
    frames[PartId::Pelvis.index()] = pelvis;

    // Torso: sways with effort, follows the bars a little, and stays over
    // the bike when it rocks.
    let roll = effort * SWAY_ROLL * sin1 + 0.35 * bike_roll;
    let yaw = -effort * SWAY_YAW * (crank + 0.6).sin() - STEER_FOLLOW * steer;
    let nod = -effort * SWAY_NOD * cos2;
    let waist = posture.hip + pelvis_pitch * Vec3::new(0.0, WAIST_UP, 0.0);
    let torso = Frame::new(
        waist,
        Quat::from_rotation_y(yaw)
            * Quat::from_rotation_z(-roll)
            * Quat::from_rotation_x(-(posture.torso_pitch + nod)),
    );
    frames[PartId::Torso.index()] = torso;

    // Head: rides on the neck but keeps facing up the road.
    let head = Frame::new(
        torso.point(HEAD_PIVOT_LOCAL),
        Quat::from_rotation_z(-0.3 * roll) * Quat::from_rotation_x(-posture.head_pitch),
    );
    frames[PartId::Head.index()] = head;

    let mut joints = Joints {
        shoulder: [Vec3::ZERO; 2],
        shoulder_reach: [0.0; 2],
    };
    let hand_right = hand_on_bar(tuck);
    let clamp = Frame::new(rig.bar_clamp, Quat::IDENTITY);

    for (i, side) in [-1.0_f32, 1.0].into_iter().enumerate() {
        let left = i == 0;

        // Leg: the cleat is locked to the pedal axle, the hip to the pelvis.
        let theta = if left { crank + PI } else { crank };
        let (sin, cos) = theta.sin_cos();
        let pedal = bike
            .point(rig.bb + Vec3::new(side * PEDAL_X, -CRANK_LENGTH * sin, -CRANK_LENGTH * cos));
        let foot = Frame::new(
            pedal,
            bike.rot * Quat::from_rotation_x(-toe_down(theta, stand)),
        );
        let ankle = foot.point(ANKLE_LOCAL);
        let hip = pelvis.point(Vec3::new(side * HIP_HALF, 0.0, 0.0));
        // The knee leads forward and tucks in a little over the top. Out of
        // the saddle it opens out instead, to clear the rocking top tube.
        let top = 0.5 * (1.0 - sin);
        let inward = 0.06 * top * (1.0 - stand) - 0.12 * stand;
        let pole = bike.rot * Vec3::new(-side * inward, 0.25, -1.0);
        let (knee, out) = two_bone(hip, ankle, THIGH, SHIN, pole);
        let (pedal_id, thigh_id, shin_id, foot_id) = if left {
            (PartId::PedalL, PartId::ThighL, PartId::ShinL, PartId::FootL)
        } else {
            (PartId::PedalR, PartId::ThighR, PartId::ShinR, PartId::FootR)
        };
        frames[pedal_id.index()] = foot;
        frames[foot_id.index()] = foot;
        frames[thigh_id.index()] = bone(hip, knee, out, true);
        frames[shin_id.index()] = bone(knee, ankle, out, true);

        // Arm: the hand is fixed to the bar; the elbow hangs down and out.
        let hand_local = if left {
            hand_right.mirrored_x()
        } else {
            hand_right
        };
        let hand = steering.then(clamp.then(hand_local));
        let wrist = hand.pos;
        let mut shoulder = torso.point(Vec3::new(
            side * SHOULDER_LOCAL.x,
            SHOULDER_LOCAL.y,
            SHOULDER_LOCAL.z,
        ));
        // A hand that the turned bars carry out of reach pulls the
        // shoulder forward, as a real shoulder does.
        let reach = (UPPER_ARM + FOREARM) * ARM_REACH;
        let gap = (wrist - shoulder).length() - reach;
        if gap > 0.0 {
            shoulder += (wrist - shoulder).normalize() * gap;
            joints.shoulder_reach[i] = gap;
        }
        let pole = Vec3::new(side * 0.30, -1.0, 0.40);
        let (elbow, out) = two_bone(shoulder, wrist, UPPER_ARM, FOREARM, pole);
        let (upper_id, fore_id, hand_id) = if left {
            (PartId::UpperArmL, PartId::ForearmL, PartId::HandL)
        } else {
            (PartId::UpperArmR, PartId::ForearmR, PartId::HandR)
        };
        frames[upper_id.index()] = bone(shoulder, elbow, out, false);
        frames[fore_id.index()] = bone(elbow, wrist, out, false);
        frames[hand_id.index()] = hand;
        joints.shoulder[i] = shoulder;
    }

    let anchors = Anchors {
        saddle_top: bike.point(rig.saddle_top).to_array(),
        hood_l: steering.point(rig.hood(-1.0)).to_array(),
        hood_r: steering.point(rig.hood(1.0)).to_array(),
        eye: head.point(EYE_LOCAL).to_array(),
        eye_forward: (head.rot * Vec3::NEG_Z).to_array(),
        rear_axle: bike.point(rig.rear_axle).to_array(),
        front_axle: steering.point(rig.front_axle).to_array(),
        bottom_bracket: bike.point(rig.bb).to_array(),
    };

    Solved {
        frames,
        anchors,
        joints,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_bone_keeps_both_lengths_and_bends_toward_the_pole() {
        let root = Vec3::new(0.1, 1.0, 0.2);
        let target = Vec3::new(0.15, 0.3, -0.1);
        let pole = Vec3::new(0.0, 0.2, -1.0);
        let (joint, out) = two_bone(root, target, 0.45, 0.44, pole);
        assert!((joint.distance(root) - 0.45).abs() < 1.0e-5);
        assert!((joint.distance(target) - 0.44).abs() < 1.0e-5);
        assert!((out.length() - 1.0).abs() < 1.0e-5);
        assert!(out.dot(pole) > 0.0, "the joint is on the pole's side");
        assert!(out.dot((target - root).normalize()).abs() < 1.0e-5);
    }

    #[test]
    fn two_bone_stretches_straight_at_a_target_out_of_reach() {
        let root = Vec3::ZERO;
        let far = Vec3::new(0.0, -2.0, 0.0);
        let (joint, _) = two_bone(root, far, 0.45, 0.44, Vec3::NEG_Z);
        assert!(joint.is_finite());
        assert!((joint.distance(root) - 0.45).abs() < 1.0e-4);
        // Fully stretched: the joint sits on the line to the target.
        assert!(joint.x.abs() < 1.0e-3 && joint.z.abs() < 0.01);
        // A target on top of the root is still finite.
        let (joint, out) = two_bone(root, root, 0.45, 0.44, Vec3::NEG_Z);
        assert!(joint.is_finite() && out.is_finite());
    }

    #[test]
    fn bone_points_its_mesh_down_the_segment() {
        let from = Vec3::new(0.1, 1.0, 0.0);
        let to = Vec3::new(0.2, 0.6, -0.2);
        let out = Vec3::NEG_Z;
        let leg = bone(from, to, out, true);
        let length = from.distance(to);
        assert!(leg.point(Vec3::new(0.0, -length, 0.0)).distance(to) < 1.0e-5);
        // A leg's front (-Z) faces out of the bend; an arm's back does.
        assert!((leg.rot * Vec3::NEG_Z).dot(out) > 0.5);
        let arm = bone(from, to, out, false);
        assert!((arm.rot * Vec3::Z).dot(out) > 0.5);
    }

    #[test]
    fn a_mirrored_frame_mirrors_points() {
        let frame = Frame::new(
            Vec3::new(0.2, 0.9, -0.5),
            Quat::from_rotation_y(0.3) * Quat::from_rotation_z(-0.8),
        );
        let local = Vec3::new(0.03, -0.02, -0.1);
        let mirrored_local = Vec3::new(-local.x, local.y, local.z);
        let a = frame.point(local);
        let b = frame.mirrored_x().point(mirrored_local);
        assert!(b.distance(Vec3::new(-a.x, a.y, a.z)) < 1.0e-6);
    }
}
