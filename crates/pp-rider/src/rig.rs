//! Dimensions of the bike and the rider, shared by the mesh builders and the
//! pose solver so the two cannot drift apart.
//!
//! Model space: meters, right-handed, Y up, the bike faces -Z, the rider's
//! left is -X. The origin is on the road under the bottom bracket.

use glam::{Quat, Vec3};

use crate::build::RiderBuild;

/// Outer radius of the wheel with its tyre (700c x 30), in meters.
pub(crate) const WHEEL_RADIUS: f32 = 0.34;
/// Radius of the tyre's round section, in meters.
pub(crate) const TYRE_RADIUS: f32 = 0.015;
/// Height of the bottom bracket above the road, in meters.
pub(crate) const BB_HEIGHT: f32 = 0.27;
/// Crank length, axle to pedal axle, in meters.
pub(crate) const CRANK_LENGTH: f32 = 0.1725;
/// Chainstay length, bottom bracket to rear axle, in meters.
const CHAINSTAY: f32 = 0.410;
/// Frame stack: bottom bracket up to the top of the head tube, in meters.
const STACK: f32 = 0.565;
/// Frame reach: bottom bracket forward to the top of the head tube.
const REACH: f32 = 0.390;
/// Head tube angle from horizontal, in degrees.
const HEAD_ANGLE_DEG: f32 = 73.0;
/// Head tube length, in meters.
const HEAD_TUBE: f32 = 0.160;
/// Fork offset (rake), in meters.
const FORK_OFFSET: f32 = 0.045;
/// Seat tube angle from horizontal, in degrees.
const SEAT_ANGLE_DEG: f32 = 73.5;
/// Seat tube length, bottom bracket to its top, in meters.
const SEAT_TUBE: f32 = 0.500;
/// Saddle height: bottom bracket to the saddle top along the seat tube.
const SADDLE_HEIGHT: f32 = 0.742;
/// Stem length, in meters.
const STEM_LENGTH: f32 = 0.100;
/// Stem angle below the perpendicular to the steerer, in degrees.
const STEM_DROP_DEG: f32 = 6.0;
/// Steerer above the head tube up to the middle of the stem clamp.
const STEM_STACK: f32 = 0.040;
/// Sideways distance from the bike's centre plane to the middle of a
/// pedal, where the cleat sits, in meters.
pub(crate) const PEDAL_X: f32 = 0.127;
/// Sideways distance from the centre plane to the chain, in meters.
pub(crate) const CHAINLINE: f32 = 0.0435;

/// Half the distance between the hip joints, in meters.
pub(crate) const HIP_HALF: f32 = 0.090;
/// Thigh length, hip joint to knee, in meters.
pub(crate) const THIGH: f32 = 0.445;
/// Shin length, knee to ankle, in meters.
pub(crate) const SHIN: f32 = 0.440;
/// Ankle joint in foot space. The foot's origin is the cleat on the pedal
/// axle; the toe points to -Z.
pub(crate) const ANKLE_LOCAL: Vec3 = Vec3::new(0.0, 0.100, 0.125);
/// Hip joints above the saddle top when seated, in meters.
const HIP_ABOVE_SADDLE: f32 = 0.072;
/// From the mid-hip point up the pelvis to the waist pivot, in meters.
pub(crate) const WAIST_UP: f32 = 0.105;
/// Shoulder joint in torso space, for the right side. The torso's origin is
/// the waist pivot, Y runs up the spine and -Z is the chest.
pub(crate) const SHOULDER_LOCAL: Vec3 = Vec3::new(0.172, 0.385, -0.125);
/// Head pivot (top of the neck) in torso space.
pub(crate) const HEAD_PIVOT_LOCAL: Vec3 = Vec3::new(0.0, 0.497, -0.166);
/// Upper arm length, shoulder to elbow, in meters.
pub(crate) const UPPER_ARM: f32 = 0.320;
/// Forearm length, elbow to wrist, in meters.
pub(crate) const FOREARM: f32 = 0.270;
/// Eye point in head space, after the head's scale-up. The head's origin
/// is its pivot on the neck.
pub(crate) const EYE_LOCAL: Vec3 = Vec3::new(0.0, 0.089, -0.125);

/// Where a brake hood's grip point sits relative to the bar clamp, for the
/// right side, with the bars straight.
pub(crate) const HOOD_FROM_CLAMP: Vec3 = Vec3::new(0.200, 0.025, -0.094);
/// Where the hand grips the hooks of the drops relative to the bar clamp,
/// for the right side: a point on the bar's centreline.
pub(crate) const DROP_FROM_CLAMP: Vec3 = Vec3::new(0.2005, -0.058, -0.093);
/// How steeply the bar rises toward the hood at that point, in radians.
/// The front of the hook is close to upright.
pub(crate) const DROP_SLOPE: f32 = 1.36;
/// Centre of whatever a closed hand is wrapped round, in hand space. The
/// hand's origin is the wrist, the fingers point to -Z and the palm faces -Y.
pub(crate) const GRIP_LOCAL: Vec3 = Vec3::new(0.0, -0.039, -0.118);

/// A point in the bike's centre plane: `forward` meters ahead of the bottom
/// bracket and `y` meters above the road.
pub(crate) const fn at(forward: f32, y: f32) -> Vec3 {
    Vec3::new(0.0, y, -forward)
}

/// A rigid placement: rotate, then translate.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    pub pos: Vec3,
    pub rot: Quat,
}

impl Frame {
    pub const IDENTITY: Frame = Frame {
        pos: Vec3::ZERO,
        rot: Quat::IDENTITY,
    };

    pub fn new(pos: Vec3, rot: Quat) -> Self {
        Self { pos, rot }
    }

    /// Map a point from the frame's local space out to its parent space.
    pub fn point(&self, local: Vec3) -> Vec3 {
        self.pos + self.rot * local
    }

    /// `self` applied after `inner`.
    pub fn then(&self, inner: Frame) -> Frame {
        Frame {
            pos: self.point(inner.pos),
            rot: self.rot * inner.rot,
        }
    }

    /// The same placement on the other side of the bike.
    pub fn mirrored_x(&self) -> Frame {
        Frame {
            pos: Vec3::new(-self.pos.x, self.pos.y, self.pos.z),
            rot: Quat::from_xyzw(self.rot.x, -self.rot.y, -self.rot.z, self.rot.w),
        }
    }
}

/// Key points of the bike in model space, with the bars straight.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rig {
    /// Centre of the bottom bracket.
    pub bb: Vec3,
    /// Centre of the rear axle.
    pub rear_axle: Vec3,
    /// Centre of the front axle.
    pub front_axle: Vec3,
    /// Top of the head tube, on the steering axis.
    pub head_top: Vec3,
    /// Bottom of the head tube, on the steering axis.
    pub head_bottom: Vec3,
    /// Unit vector up the steering axis (it leans back).
    pub steer_axis: Vec3,
    /// Unit vector square to the steering axis, pointing forward and up.
    pub steer_fwd: Vec3,
    /// Unit vector up the seat tube (it leans back).
    pub seat_dir: Vec3,
    /// Top of the seat tube.
    pub seat_top: Vec3,
    /// Middle of the saddle's top surface.
    pub saddle_top: Vec3,
    /// Where the stem clamps the steerer.
    pub stem_base: Vec3,
    /// Centre of the handlebar clamp.
    pub bar_clamp: Vec3,
    /// Mid-hip point with the rider seated.
    pub hip_seated: Vec3,
    /// Half the distance between the hip joints.
    pub hip_half: f32,
    /// Thigh length, hip to knee.
    pub thigh: f32,
    /// Shin length, knee to ankle.
    pub shin: f32,
    /// Mid-hip point up to the waist pivot.
    pub waist_up: f32,
    /// Right shoulder joint in torso space.
    pub shoulder: Vec3,
    /// Top of the neck in torso space.
    pub head_pivot: Vec3,
    /// Upper arm length, shoulder to elbow.
    pub upper_arm: f32,
    /// Forearm length, elbow to wrist.
    pub forearm: f32,
    /// Ankle joint in foot space.
    pub ankle: Vec3,
    /// Eye point in head space.
    pub eye: Vec3,
    /// Centre of the bar inside a closed hand, in hand space.
    pub grip: Vec3,
}

impl Rig {
    /// The reference bike and the reference rider.
    pub fn new() -> Self {
        let build = RiderBuild::default();
        // The build's literals and these constants are the same numbers.
        // Checking here keeps both sides live, and fails a debug build if
        // one of them is edited alone.
        debug_assert_eq!(build.hip_half, HIP_HALF);
        debug_assert_eq!(build.thigh, THIGH);
        debug_assert_eq!(build.shin, SHIN);
        debug_assert_eq!(build.waist_up, WAIST_UP);
        debug_assert_eq!(Vec3::from_array(build.shoulder), SHOULDER_LOCAL);
        debug_assert_eq!(Vec3::from_array(build.head_pivot), HEAD_PIVOT_LOCAL);
        debug_assert_eq!(build.upper_arm, UPPER_ARM);
        debug_assert_eq!(build.forearm, FOREARM);
        debug_assert_eq!(Vec3::from_array(build.ankle), ANKLE_LOCAL);
        debug_assert_eq!(Vec3::from_array(build.eye), EYE_LOCAL);
        debug_assert_eq!(Vec3::from_array(build.grip), GRIP_LOCAL);
        debug_assert_eq!(build.saddle_height, SADDLE_HEIGHT);
        debug_assert_eq!(build.reach, REACH);
        debug_assert_eq!(build.stack, STACK);
        debug_assert_eq!(build.seat_tube, SEAT_TUBE);
        debug_assert_eq!(build.stem_length, STEM_LENGTH);
        Self::from_build(&build)
    }

    /// The same bike, fitted to `build`. Wheels, cranks, and angles stay.
    /// Stack, reach, saddle height, seat tube, and stem follow the build,
    /// and so do the limb lengths the pose solver uses.
    pub fn from_build(build: &RiderBuild) -> Self {
        let bb = at(0.0, BB_HEIGHT);
        let drop = WHEEL_RADIUS - BB_HEIGHT;
        let rear_axle = at(-(CHAINSTAY * CHAINSTAY - drop * drop).sqrt(), WHEEL_RADIUS);

        let (head_sin, head_cos) = HEAD_ANGLE_DEG.to_radians().sin_cos();
        let steer_axis = Vec3::new(0.0, head_sin, head_cos);
        let steer_fwd = Vec3::new(0.0, head_cos, -head_sin);
        let head_top = at(build.reach, BB_HEIGHT + build.stack);
        let head_bottom = head_top - steer_axis * HEAD_TUBE;
        // Slide down the steering axis until the offset axle is at wheel height.
        let down = (head_bottom.y + FORK_OFFSET * head_cos - WHEEL_RADIUS) / head_sin;
        let front_axle = head_bottom - steer_axis * down + steer_fwd * FORK_OFFSET;

        let (seat_sin, seat_cos) = SEAT_ANGLE_DEG.to_radians().sin_cos();
        let seat_dir = Vec3::new(0.0, seat_sin, seat_cos);
        let seat_top = bb + seat_dir * build.seat_tube;
        let saddle_top = bb + seat_dir * build.saddle_height;

        let stem_base = head_top + steer_axis * STEM_STACK;
        let (drop_sin, drop_cos) = STEM_DROP_DEG.to_radians().sin_cos();
        let stem_dir = steer_fwd * drop_cos - steer_axis * drop_sin;
        let bar_clamp = stem_base + stem_dir * build.stem_length;

        let hip_seated = saddle_top + Vec3::new(0.0, HIP_ABOVE_SADDLE, 0.015);

        Self {
            bb,
            rear_axle,
            front_axle,
            head_top,
            head_bottom,
            steer_axis,
            steer_fwd,
            seat_dir,
            seat_top,
            saddle_top,
            stem_base,
            bar_clamp,
            hip_seated,
            hip_half: build.hip_half,
            thigh: build.thigh,
            shin: build.shin,
            waist_up: build.waist_up,
            shoulder: Vec3::from_array(build.shoulder),
            head_pivot: Vec3::from_array(build.head_pivot),
            upper_arm: build.upper_arm,
            forearm: build.forearm,
            ankle: Vec3::from_array(build.ankle),
            eye: Vec3::from_array(build.eye),
            grip: Vec3::from_array(build.grip),
        }
    }

    /// Grip point on a brake hood with the bars straight. `side` is +1 for
    /// the right hood and -1 for the left.
    pub fn hood(&self, side: f32) -> Vec3 {
        self.bar_clamp
            + Vec3::new(
                side * HOOD_FROM_CLAMP.x,
                HOOD_FROM_CLAMP.y,
                HOOD_FROM_CLAMP.z,
            )
    }

    /// The whole bike rolled `roll_rad` to the rider's right. It rolls on
    /// the round section of its tyres, so they stay on the road.
    pub fn rolled(&self, roll_rad: f32) -> Frame {
        let rot = Quat::from_rotation_z(-roll_rad);
        let pivot = Vec3::new(0.0, TYRE_RADIUS, 0.0);
        Frame {
            pos: pivot - rot * pivot,
            rot,
        }
    }

    /// Steering as a placement of anything fixed to the fork, for the bars
    /// turned `steer_rad` to the rider's right.
    pub fn steering(&self, steer_rad: f32) -> Frame {
        let rot = Quat::from_axis_angle(self.steer_axis, -steer_rad);
        Frame {
            pos: self.head_bottom - rot * self.head_bottom,
            rot,
        }
    }
}
