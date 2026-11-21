//! Player-selectable colours.

/// Colours of the kit and the bike.
///
/// Every colour is sRGB red, green, blue, each in 0..=1, the same numbers a
/// colour picker shows. [`RiderStyle::default`] is the penguin kit: black
/// back and sleeves, white front, orange trim, on a glacier-blue frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiderStyle {
    /// Jersey back, shoulders, and sleeves.
    pub jersey_primary: [f32; 3],
    /// Jersey front panel and hem, and the stripes on the helmet.
    pub jersey_secondary: [f32; 3],
    /// Trim: collar, sleeve cuffs, the band over the pockets, the hem of
    /// the shorts, and the bottle cap.
    pub jersey_accent: [f32; 3],
    /// Bib shorts.
    pub bibs: [f32; 3],
    /// Helmet shell.
    pub helmet: [f32; 3],
    /// Shoe uppers.
    pub shoes: [f32; 3],
    /// Socks.
    pub socks: [f32; 3],
    /// Skin.
    pub skin: [f32; 3],
    /// Frame, fork, and stem.
    pub frame: [f32; 3],
    /// Handlebar tape.
    pub bar_tape: [f32; 3],
    /// Tyre tread.
    pub tyre: [f32; 3],
    /// Tyre sidewall. Tan by default; set it to `tyre` for all-black tyres.
    pub tyre_wall: [f32; 3],
}

impl Default for RiderStyle {
    fn default() -> Self {
        Self {
            jersey_primary: [0.10, 0.105, 0.13],
            jersey_secondary: [0.96, 0.96, 0.94],
            jersey_accent: [1.0, 0.50, 0.08],
            bibs: [0.045, 0.045, 0.055],
            helmet: [1.0, 0.50, 0.08],
            shoes: [1.0, 0.50, 0.08],
            socks: [0.96, 0.96, 0.94],
            skin: [0.86, 0.64, 0.50],
            frame: [0.04, 0.56, 0.86],
            bar_tape: [0.09, 0.09, 0.10],
            tyre: [0.06, 0.06, 0.065],
            tyre_wall: [0.78, 0.62, 0.42],
        }
    }
}

/// Names one colour of a [`RiderStyle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleColor {
    /// [`RiderStyle::jersey_primary`].
    JerseyPrimary,
    /// [`RiderStyle::jersey_secondary`].
    JerseySecondary,
    /// [`RiderStyle::jersey_accent`].
    JerseyAccent,
    /// [`RiderStyle::bibs`].
    Bibs,
    /// [`RiderStyle::helmet`].
    Helmet,
    /// [`RiderStyle::shoes`].
    Shoes,
    /// [`RiderStyle::socks`].
    Socks,
    /// [`RiderStyle::skin`].
    Skin,
    /// [`RiderStyle::frame`].
    Frame,
    /// [`RiderStyle::bar_tape`].
    BarTape,
    /// [`RiderStyle::tyre`].
    Tyre,
    /// [`RiderStyle::tyre_wall`].
    TyreWall,
}

impl StyleColor {
    /// Every style colour.
    pub const ALL: [StyleColor; 12] = [
        StyleColor::JerseyPrimary,
        StyleColor::JerseySecondary,
        StyleColor::JerseyAccent,
        StyleColor::Bibs,
        StyleColor::Helmet,
        StyleColor::Shoes,
        StyleColor::Socks,
        StyleColor::Skin,
        StyleColor::Frame,
        StyleColor::BarTape,
        StyleColor::Tyre,
        StyleColor::TyreWall,
    ];
}

impl RiderStyle {
    /// The colour this style gives to `which`, as sRGB in 0..=1.
    #[must_use]
    pub fn color(&self, which: StyleColor) -> [f32; 3] {
        match which {
            StyleColor::JerseyPrimary => self.jersey_primary,
            StyleColor::JerseySecondary => self.jersey_secondary,
            StyleColor::JerseyAccent => self.jersey_accent,
            StyleColor::Bibs => self.bibs,
            StyleColor::Helmet => self.helmet,
            StyleColor::Shoes => self.shoes,
            StyleColor::Socks => self.socks,
            StyleColor::Skin => self.skin,
            StyleColor::Frame => self.frame,
            StyleColor::BarTape => self.bar_tape,
            StyleColor::Tyre => self.tyre,
            StyleColor::TyreWall => self.tyre_wall,
        }
    }

    /// Set the colour named by `which`, as sRGB in 0..=1.
    pub fn set_color(&mut self, which: StyleColor, rgb: [f32; 3]) {
        let slot = match which {
            StyleColor::JerseyPrimary => &mut self.jersey_primary,
            StyleColor::JerseySecondary => &mut self.jersey_secondary,
            StyleColor::JerseyAccent => &mut self.jersey_accent,
            StyleColor::Bibs => &mut self.bibs,
            StyleColor::Helmet => &mut self.helmet,
            StyleColor::Shoes => &mut self.shoes,
            StyleColor::Socks => &mut self.socks,
            StyleColor::Skin => &mut self.skin,
            StyleColor::Frame => &mut self.frame,
            StyleColor::BarTape => &mut self.bar_tape,
            StyleColor::Tyre => &mut self.tyre,
            StyleColor::TyreWall => &mut self.tyre_wall,
        };
        *slot = rgb;
    }
}
