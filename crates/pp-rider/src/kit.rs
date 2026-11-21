//! Colours laid over a rider. A kit does not change the body or the bike fit.

use crate::style::RiderStyle;

/// One set of jersey, shorts, and helmet colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kit {
    /// Stable id stored in settings. `default` keeps the rider's own colours.
    pub id: &'static str,
    /// Name shown in the settings row, such as `Sunrise`.
    pub name: &'static str,
    colors: Option<KitColors>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct KitColors {
    /// Jersey back and sleeves. The chase camera sees this.
    jersey: [f32; 3],
    /// Jersey front.
    front: [f32; 3],
    /// Collar, cuffs, and the pocket band.
    trim: [f32; 3],
    /// Bib shorts.
    shorts: [f32; 3],
    /// Helmet shell.
    helmet: [f32; 3],
}

impl Kit {
    /// `default`, then the kits a badge can unlock, in that order.
    #[must_use]
    pub fn all() -> &'static [Kit] {
        &[DEFAULT, SUNRISE, GLACIER, KOM_DOTS]
    }

    /// The kit with this id. An unknown id is [`Self::all`]'s first entry.
    #[must_use]
    pub fn get(id: &str) -> &'static Kit {
        Self::all()
            .iter()
            .find(|kit| kit.id == id)
            .unwrap_or(&DEFAULT)
    }

    /// Paint `base` with this kit. `default` returns `base` unchanged.
    ///
    /// Skin, the bike, and the shoes stay with the rider. The style has no
    /// dot pattern, so KOM dots is a white kit with red trim.
    #[must_use]
    pub fn paint(self, base: RiderStyle) -> RiderStyle {
        let Some(colors) = self.colors else {
            return base;
        };
        RiderStyle {
            jersey_primary: colors.jersey,
            jersey_secondary: colors.front,
            jersey_accent: colors.trim,
            bibs: colors.shorts,
            helmet: colors.helmet,
            ..base
        }
    }
}

const DEFAULT: Kit = Kit {
    id: "default",
    name: "Default",
    colors: None,
};

const SUNRISE: Kit = Kit {
    id: "kit-sunrise",
    name: "Sunrise",
    colors: Some(KitColors {
        jersey: [1.0, 0.45, 0.12],
        front: [1.0, 0.82, 0.45],
        trim: [0.72, 0.16, 0.08],
        shorts: [0.28, 0.08, 0.06],
        helmet: [1.0, 0.72, 0.20],
    }),
};

const GLACIER: Kit = Kit {
    id: "kit-glacier",
    name: "Glacier",
    colors: Some(KitColors {
        jersey: [0.62, 0.82, 0.92],
        front: [0.94, 0.97, 1.0],
        trim: [0.18, 0.42, 0.62],
        shorts: [0.10, 0.22, 0.34],
        helmet: [0.82, 0.93, 0.98],
    }),
};

/// White cloth and a red collar, cuff, and pocket band. No dot texture exists.
const KOM_DOTS: Kit = Kit {
    id: "kit-kom-dots",
    name: "KOM dots",
    colors: Some(KitColors {
        jersey: [0.97, 0.97, 0.96],
        front: [0.97, 0.97, 0.96],
        trim: [0.86, 0.12, 0.16],
        shorts: [0.97, 0.97, 0.96],
        helmet: [0.97, 0.97, 0.96],
    }),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_starts_with_default_and_keeps_the_unlock_ids() {
        let ids: Vec<_> = Kit::all().iter().map(|kit| kit.id).collect();
        assert_eq!(
            ids,
            vec!["default", "kit-sunrise", "kit-glacier", "kit-kom-dots"]
        );
        assert_eq!(Kit::get("kit-sunrise").name, "Sunrise");
        assert_eq!(Kit::get("kit-glacier").name, "Glacier");
        assert_eq!(Kit::get("kit-kom-dots").name, "KOM dots");
        assert_eq!(Kit::get("nope").id, "default");
    }

    #[test]
    fn default_keeps_the_rider_and_a_kit_changes_only_the_cloth() {
        let base = RiderStyle::default();
        assert_eq!(Kit::get("default").paint(base), base);
        let worn = Kit::get("kit-sunrise").paint(base);
        assert_ne!(worn.jersey_primary, base.jersey_primary);
        assert_ne!(worn.helmet, base.helmet);
        assert_eq!(worn.skin, base.skin);
        assert_eq!(worn.frame, base.frame);
        assert_eq!(worn.shoes, base.shoes);
        let kom = Kit::get("kit-kom-dots").paint(base);
        assert!(kom.jersey_primary[0] > 0.9);
        assert!(kom.jersey_accent[0] > 0.7 && kom.jersey_accent[1] < 0.3);
    }
}
