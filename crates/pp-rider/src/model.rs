//! The built model: every part's mesh plus the rig that poses them.

use crate::build::{RiderBuild, RiderPreset};
use crate::mesh::{Part, PartId};
use crate::pose::{Pose, PoseInput, solve};
use crate::rig::Rig;
use crate::style::RiderStyle;
use crate::{bike, body};

/// A road bike and its rider as rigid parts.
///
/// Build it once, turn each [`Part`] into a mesh, then call
/// [`RiderModel::pose`] every frame and copy the transforms across.
#[derive(Debug, Clone)]
pub struct RiderModel {
    parts: Vec<Part>,
    rig: Rig,
}

impl RiderModel {
    /// Generate every mesh in the colours of `style`, with the reference build.
    ///
    /// The result depends only on `style`: two calls with equal styles give
    /// equal models, vertex for vertex. Only material colours differ between
    /// styles; the geometry and the surface layout stay the same.
    #[must_use]
    pub fn build(style: &RiderStyle) -> Self {
        Self::build_with(style, &RiderBuild::default())
    }

    /// Generate every mesh for one [`RiderPreset`].
    ///
    /// The reference preset matches [`Self::build`] with [`RiderStyle::default`],
    /// vertex for vertex.
    #[must_use]
    pub fn build_preset(preset: &RiderPreset) -> Self {
        Self::build_with(&preset.style, &preset.build)
    }

    /// Generate every mesh in the colours of `style` and the proportions of `build`.
    ///
    /// Two calls with equal styles and equal builds give equal models, vertex
    /// for vertex. The reference build is what [`Self::build`] uses.
    #[must_use]
    pub fn build_with(style: &RiderStyle, build: &RiderBuild) -> Self {
        let rig = if *build == RiderBuild::default() {
            Rig::new()
        } else {
            Rig::from_build(build)
        };
        let parts = PartId::ALL
            .iter()
            .map(|&id| match id {
                PartId::Frame => bike::frame(style, &rig),
                PartId::ForkBars => bike::fork_bars(style, &rig),
                PartId::FrontWheel | PartId::RearWheel => bike::wheel(style, id),
                PartId::Crankset => bike::crankset(),
                PartId::PedalL | PartId::PedalR => bike::pedal(id),
                _ => body::part(style, build, id),
            })
            .collect();
        Self { parts, rig }
    }

    /// The parts in model order: `parts()[id.index()]` is the part `id`.
    #[must_use]
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// One part by identifier.
    #[must_use]
    pub fn part(&self, id: PartId) -> &Part {
        &self.parts[id.index()]
    }

    /// Total number of triangles in the model.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(Part::triangle_count).sum()
    }

    /// Pose the model: one rigid transform per part, plus the anchors.
    ///
    /// A pure function of `input`. It does not allocate and is cheap enough
    /// to call every frame.
    #[must_use]
    pub fn pose(&self, input: &PoseInput) -> Pose {
        solve(&self.rig, input).into_pose()
    }

    #[cfg(test)]
    pub(crate) fn rig(&self) -> &Rig {
        &self.rig
    }
}

impl Default for RiderModel {
    /// The model in the default penguin kit.
    fn default() -> Self {
        Self::build(&RiderStyle::default())
    }
}
