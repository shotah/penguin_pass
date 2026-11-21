//! Body proportions and the named riders a player can pick.
//!
//! [`RiderBuild`] is the shape and the bike fit. [`RiderStyle`] is the
//! colours. A [`RiderPreset`] is one of each, with a stable id for settings.

use crate::style::RiderStyle;

/// Body proportions and how the bike is fitted to them.
///
/// Lengths are meters. [`RiderBuild::default`] is the reference rider the
/// rest of the crate was drawn around: the same shape `RiderModel::build`
/// has always produced. A preset may change these numbers. Scales of `1`
/// and a pinch of `0` leave that part of the mesh alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiderBuild {
    /// Half the distance between the hip joints.
    pub hip_half: f32,
    /// Thigh length, hip joint to knee.
    pub thigh: f32,
    /// Shin length, knee to ankle.
    pub shin: f32,
    /// Mid-hip point up to the waist pivot.
    pub waist_up: f32,
    /// Right shoulder joint in torso space: sideways, up the spine, forward
    /// (negative Z).
    pub shoulder: [f32; 3],
    /// Top of the neck in torso space.
    pub head_pivot: [f32; 3],
    /// Upper arm length, shoulder to elbow.
    pub upper_arm: f32,
    /// Forearm length, elbow to wrist.
    pub forearm: f32,
    /// Ankle joint in foot space. The origin is the cleat.
    pub ankle: [f32; 3],
    /// Eye point in head space, after the head's scale.
    pub eye: [f32; 3],
    /// Centre of the bar inside a closed hand, in hand space.
    pub grip: [f32; 3],
    /// Extra scale of the hand mesh. `1` keeps the reference size.
    pub hand: f32,
    /// Scale of the shoe about the cleat. `1` keeps the reference size.
    pub foot: f32,
    /// Scale of the head about the neck pivot. The reference is `1.08`.
    pub head: f32,
    /// Cross-section scale of the thighs and shins. `1` is the reference.
    pub leg_girth: f32,
    /// Cross-section scale of the arms. `1` is the reference.
    pub arm_girth: f32,
    /// Chest and shoulder width of the jersey. `1` is the reference.
    pub torso_width: f32,
    /// Chest depth of the jersey. `1` is the reference.
    pub torso_depth: f32,
    /// How far the waist draws in, from `0` (the reference) toward `1`.
    pub waist_pinch: f32,
    /// Added to the pelvis half-width at the hips, in meters.
    pub hip_flare: f32,
    /// Scale of the torso's height. The shoulder and the neck are stored
    /// already scaled, in [`Self::shoulder`] and [`Self::head_pivot`].
    pub torso_length: f32,
    /// A braid leaving the nape under the helmet tail.
    pub ponytail: bool,
    /// Hair colour, sRGB. `None` is the reference brown.
    pub hair: Option<[f32; 3]>,
    /// Saddle height along the seat tube, bottom bracket to the saddle top.
    pub saddle_height: f32,
    /// Frame reach.
    pub reach: f32,
    /// Frame stack.
    pub stack: f32,
    /// Seat tube length, bottom bracket to the top of the tube.
    pub seat_tube: f32,
    /// Stem length.
    pub stem_length: f32,
}

impl Default for RiderBuild {
    /// The reference proportions. These literals are the rig's constants;
    /// `reference_build_matches_the_rig` keeps the two from drifting.
    fn default() -> Self {
        Self {
            hip_half: 0.090,
            thigh: 0.445,
            shin: 0.440,
            waist_up: 0.105,
            shoulder: [0.172, 0.385, -0.125],
            head_pivot: [0.0, 0.497, -0.166],
            upper_arm: 0.320,
            forearm: 0.270,
            ankle: [0.0, 0.100, 0.125],
            eye: [0.0, 0.089, -0.125],
            grip: [0.0, -0.039, -0.118],
            hand: 1.0,
            foot: 1.0,
            head: 1.08,
            leg_girth: 1.0,
            arm_girth: 1.0,
            torso_width: 1.0,
            torso_depth: 1.0,
            waist_pinch: 0.0,
            hip_flare: 0.0,
            torso_length: 1.0,
            ponytail: false,
            hair: None,
            saddle_height: 0.742,
            reach: 0.390,
            stack: 0.565,
            seat_tube: 0.500,
            stem_length: 0.100,
        }
    }
}

impl RiderBuild {
    /// Sigrid: an athletic woman on a slightly smaller frame.
    ///
    /// Narrower shoulders, a waist, hips wider relative to the shoulders,
    /// a shorter torso than the reference for the leg length, smaller hands
    /// and feet, and a little less overall height. The saddle, stack, and
    /// reach come down with her so the arms and legs are not stretched out.
    fn sigrid() -> Self {
        let torso = 0.90;
        Self {
            hip_half: 0.104,
            thigh: 0.428,
            shin: 0.422,
            waist_up: 0.098,
            shoulder: [0.146, 0.385 * torso, -0.125],
            head_pivot: [0.0, 0.497 * torso, -0.166],
            upper_arm: 0.300,
            forearm: 0.252,
            ankle: [0.0, 0.090, 0.112],
            eye: [0.0, 0.084, -0.118],
            grip: [0.0, -0.033, -0.100],
            hand: 0.86,
            foot: 0.90,
            head: 1.02,
            leg_girth: 1.04,
            arm_girth: 0.90,
            torso_width: 0.86,
            torso_depth: 0.94,
            waist_pinch: 0.18,
            hip_flare: 0.022,
            torso_length: torso,
            ponytail: true,
            hair: Some([0.28, 0.16, 0.10]),
            saddle_height: 0.684,
            reach: 0.356,
            stack: 0.530,
            seat_tube: 0.460,
            stem_length: 0.090,
        }
    }
}

/// One rider the player can choose: an id for the settings file, a display
/// name, a build, and a kit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiderPreset {
    /// Stable id stored in settings. Does not change when the display name does.
    pub id: &'static str,
    /// Name shown in the settings menu.
    pub name: &'static str,
    /// Body proportions and bike fit.
    pub build: RiderBuild,
    /// Kit and bike colours.
    pub style: RiderStyle,
}

impl RiderPreset {
    /// Every preset, reference rider first.
    #[must_use]
    pub fn all() -> [RiderPreset; 2] {
        [Self::nils(), Self::sigrid()]
    }

    /// The preset with this id, if it is one of [`Self::all`].
    #[must_use]
    pub fn get(id: &str) -> Option<RiderPreset> {
        Self::all().into_iter().find(|preset| preset.id == id)
    }

    /// The reference rider. Dark kit, the build [`RiderModel::build`](crate::RiderModel::build) uses.
    #[must_use]
    pub fn nils() -> Self {
        Self {
            id: "nils",
            name: "Nils",
            build: RiderBuild::default(),
            style: RiderStyle::default(),
        }
    }

    /// The second rider.
    #[must_use]
    pub fn sigrid() -> Self {
        Self {
            id: "sigrid",
            name: "Sigrid",
            build: RiderBuild::sigrid(),
            style: sigrid_style(),
        }
    }
}

/// Sigrid's kit. The chase camera sees the jersey back, so that panel is a
/// clear blue rather than the reference rider's near-black, with sun yellow
/// where the reference uses orange. The colours are the interface palette:
/// a mid blue, snow, sun, night, and a deep green taken from the "good" green.
fn sigrid_style() -> RiderStyle {
    RiderStyle {
        jersey_primary: [0.239, 0.518, 0.769],
        jersey_secondary: [0.957, 0.973, 1.0],
        jersey_accent: [1.0, 0.820, 0.400],
        bibs: [0.071, 0.125, 0.247],
        helmet: [1.0, 0.820, 0.400],
        shoes: [0.086, 0.243, 0.420],
        socks: [0.957, 0.973, 1.0],
        skin: [0.86, 0.64, 0.50],
        frame: [0.110, 0.420, 0.282],
        bar_tape: [0.071, 0.125, 0.247],
        tyre: [0.06, 0.06, 0.065],
        tyre_wall: [0.78, 0.62, 0.42],
    }
}
