//! Shared colours, type, and surfaces for every screen.
//!
//! The menu and the ride HUD both style from here. Design pixels are the
//! 1280×800 frame in `docs/ui.md`.
//!
//! - Tokens: [`INK`], [`NIGHT`], [`SLATE`], [`SNOW`], [`MIST`], [`DUSK`],
//!   [`RIDDEN`], [`SUN`], [`EMBER`], [`GOOD`], [`WARN`], [`BAD`], [`PRESSED`],
//!   [`RAMP`]
//! - [`zone_color`], [`grade_color`], [`with_alpha`]
//! - [`Weight`], [`size`], [`font`], [`font_text`], [`font_caption`], [`theme`], [`release`]
//! - [`Surface`], [`surface`], [`focus_style`]
//! - [`Role`], [`label`], [`style_label`]
//! - [`design_frame`], [`CONSOLE_PANEL_FRAC`], [`CONSOLE_BLOCK_FRAC`]

use std::cell::RefCell;
use std::mem::ManuallyDrop;

use godot::classes::control::MouseFilter;
use godot::classes::{
    Control, Font, FontFile, FontVariation, Label, Node, StyleBoxFlat, Theme, ThemeDb,
};
use godot::global::Error;
use godot::obj::NewGd;
use godot::prelude::*;

/// `ink`. Every surface, and text on yellow.
pub const INK: Color = hex(0x0B1220);
/// `night`. Raised well on ink.
pub const NIGHT: Color = hex(0x12203F);
/// `slate`. Switch track when off.
pub const SLATE: Color = hex(0x39425A);
/// `snow`. Primary text and numbers.
pub const SNOW: Color = hex(0xF4F8FF);
/// `mist`. Units, labels, explanations.
pub const MIST: Color = hex(0xB4C0D4);
/// `dusk`. Absent values and disabled rows.
pub const DUSK: Color = hex(0x7C8AA3);
/// `ridden`. The part of the course profile behind the rider.
pub const RIDDEN: Color = hex(0x66728A);
/// `sun`. Focus, and power zone 4.
pub const SUN: Color = hex(0xFFD166);
/// `ember`. Brand sunrise. Not used for state.
pub const EMBER: Color = hex(0xF08A4B);
/// `good`. Connected, on, saved.
pub const GOOD: Color = hex(0x3DD68C);
/// `warn`. Reconnecting, or changed and not yet applied.
pub const WARN: Color = hex(0xFFB23E);
/// `bad`. Failed.
pub const BAD: Color = hex(0xFF5A52);
/// Pressed fill. Focus yellow, held down.
pub const PRESSED: Color = hex(0xE9BC52);

/// Effort ramp. Index 0 is zone 1 and flat; index 1 is zone 2 and downhill.
pub const RAMP: [Color; 7] = [
    hex(0x9AA8BD),
    hex(0x4FA8FF),
    hex(0x3DD68C),
    hex(0xFFD166),
    hex(0xFF9A3C),
    hex(0xFF5A52),
    hex(0xD277FF),
];

/// Console panel height divided by the 800 px design frame (124 / 800).
pub const CONSOLE_PANEL_FRAC: f32 = 124.0 / 800.0;

/// Skyline top (y 614) through the bottom of the frame, divided by 800.
///
/// The camera should keep the wheels above this band. On a window that is
/// as tall as the design frame, or wider, this is also the fraction of the
/// screen.
pub const CONSOLE_BLOCK_FRAC: f32 = 186.0 / 800.0;

/// OpenType `tnum`, big-endian. Fixed-width digits.
const TNUM: i32 = 0x746E_756D;

const FONT_FILES: [&str; 3] = [
    "res://assets/fonts/barlow-semi-condensed/BarlowSemiCondensed-Medium.ttf",
    "res://assets/fonts/barlow-semi-condensed/BarlowSemiCondensed-SemiBold.ttf",
    "res://assets/fonts/barlow-semi-condensed/BarlowSemiCondensed-Bold.ttf",
];

/// Spacing, in design pixels, shared by every panel.
pub mod space {
    /// Inner padding on all four sides of a menu or dialog.
    pub const PANEL: i32 = 24;
    /// Text inset inside a focused row.
    pub const ROW: i32 = 12;
    /// Gap between a chip and the next chip.
    pub const GAP: i32 = 8;
}

/// Design-frame sizes, in pixels, before the window scale.
pub mod size {
    /// Hero power.
    pub const HERO: i32 = 88;
    /// HUD primary numbers.
    pub const NUMBER: i32 = 52;
    /// Screen title.
    pub const TITLE: i32 = 36;
    /// Title menu item and dialog title.
    pub const MENU_ITEM: i32 = 32;
    /// Pairing headline.
    pub const HEADLINE: i32 = 28;
    /// Row, button, device name.
    pub const ROW: i32 = 24;
    /// HUD list values.
    pub const LIST_VALUE: i32 = 22;
    /// Explanations, hints, chip names.
    pub const BODY: i32 = 20;
    /// Units, labels, captions.
    pub const MIN: i32 = 18;
}

/// Barlow Semi Condensed weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    /// 500. Body, units, unfocused rows.
    Medium,
    /// 600. Names, values, captions.
    SemiBold,
    /// 700. Numbers, titles, focused values.
    Bold,
}

impl Weight {
    fn index(self) -> usize {
        match self {
            Self::Medium => 0,
            Self::SemiBold => 1,
            Self::Bold => 2,
        }
    }
}

/// Which flat surface to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// Ride console. Ink at 88 %, radius 12.
    Console,
    /// Ride chip. Ink at 88 %, radius 18.
    Chip,
    /// Menu panel. Ink at 94 %, radius 16.
    Menu,
    /// Dialog. Ink at 98 %, radius 20.
    Dialog,
    /// Raised well. Opaque night, radius 12.
    Well,
}

/// Text role. Sizes are the 800 px column of the type scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 88 px Bold, tabular.
    Hero,
    /// 52 px Bold, tabular.
    Number,
    /// 36 px Bold.
    Title,
    /// 32 px SemiBold.
    MenuItem,
    /// 28 px SemiBold.
    Headline,
    /// 24 px Medium.
    Row,
    /// 22 px SemiBold, tabular.
    ListValue,
    /// 20 px Medium.
    Body,
    /// 18 px Medium, mist.
    Unit,
    /// 18 px SemiBold, mist, upper case, tracked.
    Caption,
}

/// The 1280×800 frame centred in `viewport`.
///
/// `scale` is `min(width / 1280, height / 800)`. In the stretched game
/// window the viewport is already about 800 px tall, so `scale` is 1 and
/// the engine's stretch does the rest.
#[derive(Clone, Copy, Debug)]
pub struct DesignFrame {
    /// Top-left of the frame, in viewport pixels.
    pub origin: Vector2,
    /// Design pixel to viewport pixel.
    pub scale: f32,
}

impl DesignFrame {
    /// X of a design-frame coordinate.
    #[must_use]
    pub fn x(self, design: f32) -> f32 {
        self.origin.x + design * self.scale
    }

    /// Y of a design-frame coordinate.
    #[must_use]
    pub fn y(self, design: f32) -> f32 {
        self.origin.y + design * self.scale
    }

    /// A length in design pixels, scaled.
    #[must_use]
    pub fn len(self, design: f32) -> f32 {
        design * self.scale
    }

    /// A font size in design pixels, scaled and at least 1.
    #[must_use]
    pub fn px(self, design: f32) -> i32 {
        (design * self.scale).round().max(1.0) as i32
    }
}

/// Frame for a viewport size, in viewport pixels.
#[must_use]
pub fn design_frame(viewport: Vector2) -> DesignFrame {
    let scale = (viewport.x / 1280.0).min(viewport.y / 800.0).max(0.01);
    let size = Vector2::new(1280.0 * scale, 800.0 * scale);
    DesignFrame {
        origin: (viewport - size) * 0.5,
        scale,
    }
}

/// Zone colour. `index` is 1 to 7. Anything else is [`DUSK`].
#[must_use]
pub fn zone_color(index_1_to_7: u8) -> Color {
    match index_1_to_7 {
        1..=7 => RAMP[usize::from(index_1_to_7 - 1)],
        _ => DUSK,
    }
}

/// Gradient colour for a grade in percent.
#[must_use]
pub fn grade_color(grade_pct: f32) -> Color {
    RAMP[usize::from(pp_view::profile::grade_ramp(grade_pct))]
}

/// The same colour with a new alpha.
#[must_use]
pub const fn with_alpha(color: Color, alpha: f32) -> Color {
    Color {
        r: color.r,
        g: color.g,
        b: color.b,
        a: alpha,
    }
}

/// Barlow Semi Condensed with tabular figures, for numbers that change on screen.
#[must_use]
pub fn font(weight: Weight) -> Gd<Font> {
    with_cache(|cache| cache.tabular[weight.index()].clone())
}

/// Barlow Semi Condensed with proportional figures, for names and sentences.
#[must_use]
pub fn font_text(weight: Weight) -> Gd<Font> {
    with_cache(|cache| cache.proportional[weight.index()].clone())
}

/// SemiBold with caption tracking (about 0.06 em), for `TIME` and zone names.
#[must_use]
pub fn font_caption() -> Gd<Font> {
    with_cache(|cache| cache.caption.clone())
}

/// Default theme: Medium 24, label colour [`SNOW`].
#[must_use]
pub fn theme() -> Gd<Theme> {
    with_cache(|cache| cache.theme.clone())
}

/// A panel, chip, menu, dialog, or well.
#[must_use]
pub fn surface(kind: Surface) -> Gd<StyleBoxFlat> {
    match kind {
        Surface::Console => flat(with_alpha(INK, 0.88), Some(with_alpha(SNOW, 0.14)), 1, 12),
        Surface::Chip => flat(with_alpha(INK, 0.88), Some(with_alpha(SNOW, 0.14)), 1, 18),
        Surface::Menu => flat(with_alpha(INK, 0.94), Some(with_alpha(SNOW, 0.12)), 1, 16),
        Surface::Dialog => flat(with_alpha(INK, 0.98), Some(with_alpha(SNOW, 0.12)), 1, 20),
        Surface::Well => flat(NIGHT, None, 0, 12),
    }
}

/// Focused row: opaque [`SUN`], radius 10.
#[must_use]
pub fn focus_style() -> Gd<StyleBoxFlat> {
    let mut style = flat(SUN, None, 0, 10);
    pad_box(&mut style, space::ROW as f32, 4.0);
    style
}

/// Inset the contents of a style box. Vertical padding can be smaller than the sides.
pub fn pad_box(style: &mut Gd<StyleBoxFlat>, horizontal: f32, vertical: f32) {
    style.set_content_margin(godot::builtin::Side::LEFT, horizontal);
    style.set_content_margin(godot::builtin::Side::RIGHT, horizontal);
    style.set_content_margin(godot::builtin::Side::TOP, vertical);
    style.set_content_margin(godot::builtin::Side::BOTTOM, vertical);
}

/// Every descendant of a menu row panel ignores the pointer so clicks hit the row.
pub fn ignore_pointer_descendants(row: &Gd<Control>) {
    for child in row.get_children().iter_shared() {
        if let Ok(mut control) = child.clone().try_cast::<Control>() {
            control.set_mouse_filter(MouseFilter::IGNORE);
            ignore_pointer_descendants(&control);
        } else if let Ok(node) = child.clone().try_cast::<Node>() {
            ignore_pointer_descendants_node(&node);
        }
    }
}

fn ignore_pointer_descendants_node(node: &Gd<Node>) {
    for child in node.get_children().iter_shared() {
        if let Ok(mut control) = child.clone().try_cast::<Control>() {
            control.set_mouse_filter(MouseFilter::IGNORE);
            ignore_pointer_descendants(&control);
        } else if let Ok(node) = child.clone().try_cast::<Node>() {
            ignore_pointer_descendants_node(&node);
        }
    }
}

/// A label in `role`, mouse-ignored. Captions are upper-cased.
#[must_use]
pub fn label(text: &str, role: Role) -> Gd<Label> {
    let mut label = Label::new_alloc();
    let shown = if role == Role::Caption {
        text.to_uppercase()
    } else {
        text.to_owned()
    };
    label.set_text(&shown);
    style_label(&mut label, role);
    label
}

/// Apply `role` to an existing label. A caption's text is upper-cased.
pub fn style_label(label: &mut Gd<Label>, role: Role) {
    let (weight, px, color, tabular, caption) = role_spec(role);
    let face = if caption {
        with_cache(|cache| cache.caption.clone())
    } else if tabular {
        font(weight)
    } else {
        font_text(weight)
    };
    label.add_theme_font_override("font", &face);
    label.add_theme_font_size_override("font_size", px);
    label.add_theme_color_override("font_color", color);
    label.set_mouse_filter(MouseFilter::IGNORE);
    if caption {
        let upper = label.get_text().to_string().to_uppercase();
        label.set_text(&upper);
    }
}

/// Drop the cached fonts and theme while the engine is still running.
///
/// A thread-local `Gd` must not be dropped from its destructor: by process
/// exit the engine is gone. [`Hud`](crate::hud::Hud) calls this from `exit_tree`.
pub fn release() {
    CACHE.with(|slot| {
        if let Some(cache) = slot.borrow_mut().take() {
            drop(ManuallyDrop::into_inner(cache));
        }
    });
}

fn role_spec(role: Role) -> (Weight, i32, Color, bool, bool) {
    match role {
        Role::Hero => (Weight::Bold, size::HERO, SNOW, true, false),
        Role::Number => (Weight::Bold, size::NUMBER, SNOW, true, false),
        Role::Title => (Weight::Bold, size::TITLE, SNOW, false, false),
        Role::MenuItem => (Weight::SemiBold, size::MENU_ITEM, SNOW, false, false),
        Role::Headline => (Weight::SemiBold, size::HEADLINE, SNOW, false, false),
        Role::Row => (Weight::Medium, size::ROW, SNOW, false, false),
        Role::ListValue => (Weight::SemiBold, size::LIST_VALUE, SNOW, true, false),
        Role::Body => (Weight::Medium, size::BODY, SNOW, false, false),
        Role::Unit => (Weight::Medium, size::MIN, MIST, false, false),
        Role::Caption => (Weight::SemiBold, size::MIN, MIST, false, true),
    }
}

fn flat(fill: Color, border: Option<Color>, border_px: i32, radius: i32) -> Gd<StyleBoxFlat> {
    let mut style = StyleBoxFlat::new_gd();
    style.set_bg_color(fill);
    style.set_corner_radius_all(radius);
    style.set_anti_aliased(true);
    style.set_content_margin_all(0.0);
    if let Some(border) = border {
        style.set_border_color(border);
        style.set_border_width_all(border_px);
    }
    style
}

struct Cache {
    tabular: [Gd<Font>; 3],
    proportional: [Gd<Font>; 3],
    caption: Gd<Font>,
    theme: Gd<Theme>,
}

impl Cache {
    fn load() -> Self {
        let mut files = Vec::with_capacity(3);
        for path in FONT_FILES {
            files.push(load_font(path));
        }
        let proportional = [
            files[0].clone().upcast(),
            files[1].clone().upcast(),
            files[2].clone().upcast(),
        ];
        let tabular = [
            with_features(&files[0], true, 0),
            with_features(&files[1], true, 0),
            with_features(&files[2], true, 0),
        ];
        // 0.06 em at 18 px is about 1 px of glyph spacing.
        let caption = with_features(&files[1], false, 1);
        let mut theme = Theme::new_gd();
        theme.set_default_font(&proportional[0]);
        theme.set_default_font_size(size::ROW);
        theme.set_font("font", "Label", &proportional[0]);
        theme.set_font_size("font_size", "Label", size::ROW);
        theme.set_color("font_color", "Label", SNOW);
        Self {
            tabular,
            proportional,
            caption,
            theme,
        }
    }
}

fn load_font(path: &str) -> Gd<FontFile> {
    let mut font = FontFile::new_gd();
    if font.load_dynamic_font(path) != Error::OK {
        godot_warn!("Penguin Pass: font missing at {path}, using the engine fallback");
    }
    font
}

fn with_features(base: &Gd<FontFile>, tabular: bool, spacing_px: i32) -> Gd<Font> {
    let mut variation = FontVariation::new_gd();
    if base.get_data().is_empty() {
        if let Some(fallback) = ThemeDb::singleton().get_fallback_font() {
            variation.set_base_font(&fallback);
        }
    } else {
        variation.set_base_font(base);
    }
    if tabular {
        let mut features = VarDictionary::new();
        features.set(TNUM, 1);
        variation.set_opentype_features(&features);
    }
    if spacing_px != 0 {
        variation.set_spacing(godot::classes::text_server::SpacingType::GLYPH, spacing_px);
    }
    variation.upcast()
}

fn with_cache<T>(read: impl FnOnce(&Cache) -> T) -> T {
    CACHE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(ManuallyDrop::new(Cache::load()));
        }
        let cache = slot.as_ref().expect("font cache");
        read(cache)
    })
}

thread_local! {
    static CACHE: RefCell<Option<ManuallyDrop<Cache>>> = const { RefCell::new(None) };
}

const fn hex(rgb: u32) -> Color {
    Color::from_rgb(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    )
}
