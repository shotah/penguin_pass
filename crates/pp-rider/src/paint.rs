//! Materials used by the builders: style-driven colours and fixed ones.

use crate::mesh::Material;
use crate::style::{RiderStyle, StyleColor};

/// A material plus where its colour came from.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Paint {
    pub material: Material,
    pub style_color: Option<StyleColor>,
}

/// A fixed colour that no style changes.
pub(crate) const fn fixed(rgb: [f32; 3], roughness: f32, metallic: f32) -> Paint {
    Paint {
        material: Material {
            base_color: rgb,
            roughness,
            metallic,
        },
        style_color: None,
    }
}

/// A colour taken from the style.
pub(crate) fn styled(
    style: &RiderStyle,
    which: StyleColor,
    roughness: f32,
    metallic: f32,
) -> Paint {
    Paint {
        material: Material {
            base_color: style.color(which),
            roughness,
            metallic,
        },
        style_color: Some(which),
    }
}

/// Matte black parts: saddle, seatpost, stem, hoods, pedals.
pub(crate) const COMPONENT: Paint = fixed([0.085, 0.085, 0.095], 0.55, 0.1);
/// Soft rubber: brake hoods and the saddle top. A shade lighter than the
/// bar tape and the shorts, so each reads against its neighbour.
pub(crate) const RUBBER: Paint = fixed([0.15, 0.15, 0.165], 0.9, 0.0);
/// Carbon rims and cranks, a little glossier than the components.
pub(crate) const CARBON: Paint = fixed([0.07, 0.07, 0.08], 0.35, 0.2);
/// Bright steel and alloy: spokes, rotors, cassette, chain.
pub(crate) const STEEL: Paint = fixed([0.72, 0.73, 0.76], 0.32, 0.9);
/// Darker metal: chainrings, derailleur, hubs.
pub(crate) const GUNMETAL: Paint = fixed([0.26, 0.27, 0.30], 0.38, 0.8);
/// Brake lever blades.
pub(crate) const ALLOY: Paint = fixed([0.46, 0.47, 0.50], 0.34, 0.9);
/// Chain links.
pub(crate) const CHAIN: Paint = fixed([0.50, 0.50, 0.52], 0.40, 0.9);
/// Rim decals and other white marks.
pub(crate) const DECAL: Paint = fixed([0.93, 0.93, 0.92], 0.5, 0.0);
/// Water bottle body.
pub(crate) const BOTTLE: Paint = fixed([0.93, 0.94, 0.95], 0.45, 0.0);
/// Rear light lens.
pub(crate) const TAIL_LIGHT: Paint = fixed([0.92, 0.07, 0.06], 0.25, 0.0);
/// Head unit screen.
pub(crate) const SCREEN: Paint = fixed([0.55, 0.66, 0.62], 0.2, 0.0);
/// Sunglass lens.
pub(crate) const LENS: Paint = fixed([0.035, 0.04, 0.06], 0.08, 0.6);
/// Helmet vents, straps, and the retention dial.
pub(crate) const HELMET_DARK: Paint = fixed([0.05, 0.05, 0.06], 0.7, 0.0);
/// Hair showing under the helmet.
pub(crate) const HAIR: Paint = fixed([0.20, 0.13, 0.09], 0.8, 0.0);
/// The mouth.
pub(crate) const LIPS: Paint = fixed([0.46, 0.24, 0.21], 0.6, 0.0);
/// Shoe soles and cleats.
pub(crate) const SOLE: Paint = fixed([0.05, 0.05, 0.055], 0.6, 0.0);
/// Seams on the jersey pockets.
pub(crate) const SEAM: Paint = fixed([0.30, 0.31, 0.34], 0.8, 0.0);
