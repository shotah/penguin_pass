//! Mesh data handed to the renderer.

use crate::style::StyleColor;

/// Simple PBR parameters for one surface. There are no textures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// Base colour as sRGB red, green, blue, each in 0..=1.
    ///
    /// Godot's `Color` takes these numbers as they are. glTF wants linear
    /// values, so the exporter converts.
    pub base_color: [f32; 3],
    /// Roughness in 0..=1. 0 is a mirror, 1 is chalk.
    pub roughness: f32,
    /// Metallic in 0..=1.
    pub metallic: f32,
}

/// One material's worth of triangles in a part.
///
/// Positions are in meters, in the part's own local space. Triangles are
/// counter-clockwise when seen from outside (the glTF convention). Godot's
/// `ArrayMesh` wants clockwise; use [`Surface::indices_clockwise`] there.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    /// Vertex positions, in meters, part-local.
    pub positions: Vec<[f32; 3]>,
    /// Unit vertex normals, one per position.
    pub normals: Vec<[f32; 3]>,
    /// Triangle list, three indices per triangle, counter-clockwise.
    pub indices: Vec<u32>,
    /// How the surface is shaded.
    pub material: Material,
    /// Which [`RiderStyle`](crate::RiderStyle) colour `material.base_color`
    /// was taken from, or `None` for a fixed colour such as chain steel.
    ///
    /// A settings menu can recolour a built model by this tag without
    /// rebuilding any mesh.
    pub style_color: Option<StyleColor>,
}

impl Surface {
    /// Number of triangles in the surface.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The triangle list with each triangle reversed, so front faces are
    /// clockwise. This is the order Godot's `ArrayMesh` expects.
    #[must_use]
    pub fn indices_clockwise(&self) -> Vec<u32> {
        let mut out = self.indices.clone();
        for tri in out.as_chunks_mut::<3>().0 {
            tri.swap(1, 2);
        }
        out
    }
}

/// One rigid piece of the model: a wheel, a shin, the frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub(crate) name: &'static str,
    pub(crate) surfaces: Vec<Surface>,
}

impl Part {
    /// Stable name of the part, the same as [`PartId::name`].
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The part's surfaces, one per material.
    #[must_use]
    pub fn surfaces(&self) -> &[Surface] {
        &self.surfaces
    }

    /// Number of triangles in the part.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.surfaces.iter().map(Surface::triangle_count).sum()
    }
}

/// Identifies a part. The discriminant is the part's index in
/// [`RiderModel::parts`](crate::RiderModel::parts) and in
/// [`Pose::transforms`](crate::Pose::transforms).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(usize)]
pub enum PartId {
    /// Frame, seatpost, saddle, bottle, rear derailleur, and chain.
    Frame,
    /// Fork, stem, handlebar, brake hoods, and head unit. Turns with steering.
    ForkBars,
    /// Front wheel with its brake rotor.
    FrontWheel,
    /// Rear wheel with its cassette and brake rotor.
    RearWheel,
    /// Both crank arms, the spider, and the chainrings.
    Crankset,
    /// Left pedal.
    PedalL,
    /// Right pedal.
    PedalR,
    /// Hips, in bib shorts.
    Pelvis,
    /// Torso in the jersey, with the neck.
    Torso,
    /// Head, helmet, and sunglasses.
    Head,
    /// Left upper arm, with the jersey sleeve.
    UpperArmL,
    /// Right upper arm, with the jersey sleeve.
    UpperArmR,
    /// Left forearm.
    ForearmL,
    /// Right forearm.
    ForearmR,
    /// Left hand.
    HandL,
    /// Right hand.
    HandR,
    /// Left thigh.
    ThighL,
    /// Right thigh.
    ThighR,
    /// Left shin, with the sock.
    ShinL,
    /// Right shin, with the sock.
    ShinR,
    /// Left foot, in its shoe.
    FootL,
    /// Right foot, in its shoe.
    FootR,
}

impl PartId {
    /// Number of parts in the model.
    pub const COUNT: usize = 22;

    /// Every part, in model order.
    pub const ALL: [PartId; Self::COUNT] = [
        PartId::Frame,
        PartId::ForkBars,
        PartId::FrontWheel,
        PartId::RearWheel,
        PartId::Crankset,
        PartId::PedalL,
        PartId::PedalR,
        PartId::Pelvis,
        PartId::Torso,
        PartId::Head,
        PartId::UpperArmL,
        PartId::UpperArmR,
        PartId::ForearmL,
        PartId::ForearmR,
        PartId::HandL,
        PartId::HandR,
        PartId::ThighL,
        PartId::ThighR,
        PartId::ShinL,
        PartId::ShinR,
        PartId::FootL,
        PartId::FootR,
    ];

    /// Index of the part in the model's part list and in a pose.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Stable snake-case name, for example `"rear_wheel"`. Use it as the
    /// scene node name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            PartId::Frame => "frame",
            PartId::ForkBars => "fork_bars",
            PartId::FrontWheel => "front_wheel",
            PartId::RearWheel => "rear_wheel",
            PartId::Crankset => "crankset",
            PartId::PedalL => "pedal_l",
            PartId::PedalR => "pedal_r",
            PartId::Pelvis => "pelvis",
            PartId::Torso => "torso",
            PartId::Head => "head",
            PartId::UpperArmL => "upper_arm_l",
            PartId::UpperArmR => "upper_arm_r",
            PartId::ForearmL => "forearm_l",
            PartId::ForearmR => "forearm_r",
            PartId::HandL => "hand_l",
            PartId::HandR => "hand_r",
            PartId::ThighL => "thigh_l",
            PartId::ThighR => "thigh_r",
            PartId::ShinL => "shin_l",
            PartId::ShinR => "shin_r",
            PartId::FootL => "foot_l",
            PartId::FootR => "foot_r",
        }
    }

    /// Whether the part belongs to the bicycle rather than the rider.
    #[must_use]
    pub const fn is_bike(self) -> bool {
        (self as usize) <= (PartId::PedalR as usize)
    }
}
