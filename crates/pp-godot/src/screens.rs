//! Title, pause, summary, course list, confirm, and toasts.
//!
//! Colours are the `docs/ui.md` tokens. When `ui_style` is present the
//! drawing goes through it.

use godot::classes::control::{LayoutPreset, MouseFilter};
use godot::classes::{
    CanvasLayer, ColorRect, HBoxContainer, Image, ImageTexture, Label, Node3D, PanelContainer,
    StyleBoxEmpty, TextureRect, VBoxContainer,
};
use godot::obj::Gd;
use godot::prelude::*;
use pp_view::shell::{Screen, ShellView};

use crate::ui_style::{self, Role, Surface};

/// Overlay for every screen that is not the settings list.
pub struct Screens {
    layer: Gd<CanvasLayer>,
    scrim: Gd<ColorRect>,
    card: Gd<PanelContainer>,
    logo: Gd<TextureRect>,
    wordmark: Gd<Label>,
    wordmark_pass: Gd<Label>,
    title: Gd<Label>,
    rows: Vec<Gd<Label>>,
    row_panels: Vec<Gd<PanelContainer>>,
    detail: Gd<Label>,
    hints: Gd<Label>,
    figures: Vec<Gd<Label>>,
    saved: Gd<Label>,
    saved_folder: Gd<Label>,
    sparks: Vec<[Gd<ColorRect>; 12]>,
    toast_panel: Gd<PanelContainer>,
    toast: Gd<Label>,
    toast_s: f64,
    toasts: bool,
}

impl Screens {
    /// Build a hidden overlay on `parent`.
    pub fn new(parent: &mut Gd<Node3D>) -> Self {
        let mut layer = CanvasLayer::new_alloc();
        layer.set_name("Screens");
        layer.set_layer(24);

        let mut scrim = ColorRect::new_alloc();
        scrim.set_name("Scrim");
        scrim.set_anchors_preset(LayoutPreset::FULL_RECT);
        scrim.set_color(ui_style::with_alpha(ui_style::INK, 0.66));
        scrim.set_mouse_filter(MouseFilter::IGNORE);
        layer.add_child(&scrim);

        let mut card = PanelContainer::new_alloc();
        card.set_name("Card");
        card.set_position(Vector2::new(48.0, 96.0));
        card.set_mouse_filter(MouseFilter::IGNORE);
        let mut card_style = ui_style::surface(Surface::Menu);
        ui_style::pad_box(
            &mut card_style,
            ui_style::space::PANEL as f32,
            ui_style::space::PANEL as f32,
        );
        card.add_theme_stylebox_override("panel", &card_style);

        let mut box_ = VBoxContainer::new_alloc();
        box_.set_name("Rows");
        box_.set_custom_minimum_size(Vector2::new(440.0, 0.0));
        box_.add_theme_constant_override("separation", ui_style::space::GAP);

        let mut logo = TextureRect::new_alloc();
        logo.set_name("Logo");
        logo.set_custom_minimum_size(Vector2::new(96.0, 96.0));
        logo.set_expand_mode(godot::classes::texture_rect::ExpandMode::IGNORE_SIZE);
        if let Some(texture) = load_logo() {
            logo.set_texture(&texture);
        } else {
            logo.set_visible(false);
        }
        box_.add_child(&logo);

        let mut wordmark = ui_style::label("PENGUIN", Role::Title);
        wordmark.set_name("Wordmark");
        wordmark.add_theme_font_size_override("font_size", 56);
        box_.add_child(&wordmark);
        let mut wordmark_pass = ui_style::label("PASS", Role::Title);
        wordmark_pass.set_name("WordmarkPass");
        wordmark_pass.add_theme_font_size_override("font_size", 56);
        wordmark_pass.add_theme_color_override("font_color", ui_style::SUN);
        box_.add_child(&wordmark_pass);

        let mut title = ui_style::label("", Role::Title);
        title.set_name("ScreenTitle");
        box_.add_child(&title);

        let mut rows = Vec::new();
        let mut row_panels = Vec::new();
        let mut sparks = Vec::new();
        for index in 0..8 {
            let mut row_panel = PanelContainer::new_alloc();
            row_panel.set_name(&format!("ScreenPanel{index}"));
            row_panel.set_custom_minimum_size(Vector2::new(400.0, 48.0));
            row_panel.add_theme_stylebox_override("panel", &StyleBoxEmpty::new_gd());
            let mut row_box = HBoxContainer::new_alloc();
            let mut label = ui_style::label("", Role::MenuItem);
            label.set_name(&format!("ScreenRow{index}"));
            label.set_h_size_flags(godot::classes::control::SizeFlags::EXPAND_FILL);
            row_box.add_child(&label);
            let spark: [Gd<ColorRect>; 12] = std::array::from_fn(|_| {
                let mut mark = ColorRect::new_alloc();
                mark.set_custom_minimum_size(Vector2::new(4.0, 8.0));
                mark.set_color(ui_style::SNOW);
                mark.set_visible(false);
                mark
            });
            for mark in &spark {
                row_box.add_child(mark);
            }
            row_panel.add_child(&row_box);
            box_.add_child(&row_panel);
            sparks.push(spark);
            rows.push(label);
            row_panels.push(row_panel);
        }

        let mut figures = Vec::new();
        for index in 0..7 {
            let mut figure = ui_style::label("", Role::ListValue);
            figure.set_name(&format!("Figure{index}"));
            figure.set_visible(false);
            box_.add_child(&figure);
            figures.push(figure);
        }

        let mut saved = ui_style::label("", Role::Caption);
        saved.set_name("SavedRide");
        saved.set_visible(false);
        box_.add_child(&saved);
        let mut saved_folder = ui_style::label("", Role::Unit);
        saved_folder.set_name("SavedFolder");
        saved_folder.set_visible(false);
        box_.add_child(&saved_folder);

        let mut detail = ui_style::label("", Role::Body);
        detail.set_name("ScreenDetail");
        detail.add_theme_color_override("font_color", ui_style::MIST);
        box_.add_child(&detail);

        let mut hints = ui_style::label("Move    Select", Role::Unit);
        hints.set_name("Hints");
        box_.add_child(&hints);

        card.add_child(&box_);
        layer.add_child(&card);

        let mut toast_panel = PanelContainer::new_alloc();
        toast_panel.set_name("ToastPanel");
        toast_panel.set_position(Vector2::new(360.0, 32.0));
        toast_panel.set_custom_minimum_size(Vector2::new(560.0, 48.0));
        let mut toast_style = ui_style::surface(Surface::Dialog);
        ui_style::pad_box(
            &mut toast_style,
            ui_style::space::PANEL as f32,
            ui_style::space::ROW as f32,
        );
        toast_panel.add_theme_stylebox_override("panel", &toast_style);
        toast_panel.set_visible(false);
        let mut toast = ui_style::label("", Role::Body);
        toast.set_name("Toast");
        toast_panel.add_child(&toast);
        layer.add_child(&toast_panel);

        parent.add_child(&layer);
        Self {
            layer,
            scrim,
            card,
            logo,
            wordmark,
            wordmark_pass,
            title,
            rows,
            row_panels,
            detail,
            hints,
            figures,
            saved,
            saved_folder,
            sparks,
            toast_panel,
            toast,
            toast_s: 0.0,
            toasts: true,
        }
    }

    /// `--no-hud` suppresses toasts.
    pub fn set_toasts(&mut self, enabled: bool) {
        self.toasts = enabled;
        if !enabled {
            self.toast_panel.set_visible(false);
            self.toast_s = 0.0;
        }
    }

    /// Tell each row to report hover.
    pub fn bind_pointer(&mut self, game: &Gd<crate::game::Game>) {
        for (index, panel) in self.row_panels.iter().enumerate() {
            let mut panel = panel.clone();
            let callable = Callable::from_object_method(game, "on_screen_hover")
                .bind(&[Variant::from(index as i64)]);
            let _ = panel.connect(&StringName::from("mouse_entered"), &callable);
        }
    }

    /// Draw the current shell view. The ride itself hides this layer.
    pub fn show(
        &mut self,
        view: &ShellView,
        profiles: &[Vec<(f32, f32)>],
        figures: &[(String, String)],
    ) {
        let visible = !matches!(view.screen, Screen::Riding | Screen::Settings);
        self.layer.set_visible(visible);
        if !visible {
            return;
        }
        let scrim = match view.screen {
            Screen::Title => 0.0,
            Screen::Summary | Screen::Course => 0.74,
            Screen::ConfirmEnd | Screen::ConfirmQuit => 0.6,
            _ => 0.66,
        };
        self.scrim
            .set_color(ui_style::with_alpha(ui_style::INK, scrim));
        let on_title = view.screen == Screen::Title;
        if on_title {
            self.card.set_position(Vector2::new(72.0, 80.0));
            self.card
                .add_theme_stylebox_override("panel", &StyleBoxEmpty::new_gd());
        } else {
            self.card.set_position(Vector2::new(48.0, 96.0));
            let mut card_style = ui_style::surface(Surface::Menu);
            ui_style::pad_box(
                &mut card_style,
                ui_style::space::PANEL as f32,
                ui_style::space::PANEL as f32,
            );
            self.card.add_theme_stylebox_override("panel", &card_style);
        }
        let show_logo = on_title && self.logo.get_texture().is_some();
        self.logo.set_visible(show_logo);
        self.wordmark.set_visible(on_title);
        self.wordmark_pass.set_visible(on_title);
        self.title.set_text(&view.title);
        self.title.set_visible(!view.title.is_empty());
        for (index, label) in self.rows.iter_mut().enumerate() {
            let Some(row) = view.rows.get(index) else {
                label.set_visible(false);
                if let Some(panel) = self.row_panels.get_mut(index) {
                    panel.set_visible(false);
                }
                continue;
            };
            let text = if row.detail.is_empty() {
                row.label.clone()
            } else {
                format!("{}    {}", row.label, row.detail)
            };
            let focused = index == view.selected;
            label.set_text(&text);
            label.set_visible(true);
            label.add_theme_color_override(
                "font_color",
                if focused {
                    ui_style::INK
                } else {
                    ui_style::SNOW
                },
            );
            if let Some(panel) = self.row_panels.get_mut(index) {
                panel.set_visible(true);
                if focused {
                    panel.add_theme_stylebox_override("panel", &ui_style::focus_style());
                } else {
                    panel.add_theme_stylebox_override("panel", &StyleBoxEmpty::new_gd());
                }
            }
        }
        self.detail.set_text(&view.detail);
        self.hints
            .set_visible(!matches!(view.screen, Screen::Riding | Screen::Settings));
        self.paint_profiles(view, profiles);
        self.paint_figures(view.screen, figures);
        if view.screen != Screen::Summary {
            self.saved.set_visible(false);
            self.saved_folder.set_visible(false);
        }
    }

    /// The file line under the summary figures. `None` hides it.
    pub fn set_ride_file(&mut self, note: Option<&pp_view::rides::RideFileNote>) {
        let Some(note) = note else {
            self.saved.set_visible(false);
            self.saved_folder.set_visible(false);
            return;
        };
        self.saved.set_text(&note.headline);
        self.saved.add_theme_color_override(
            "font_color",
            if note.saved {
                ui_style::MIST
            } else {
                ui_style::DUSK
            },
        );
        self.saved.set_visible(true);
        if note.folder.is_empty() {
            self.saved_folder.set_visible(false);
        } else {
            self.saved_folder.set_text(&note.folder);
            self.saved_folder.set_visible(true);
        }
    }

    fn paint_profiles(&mut self, view: &ShellView, profiles: &[Vec<(f32, f32)>]) {
        let show = view.screen == Screen::Course;
        for (index, group) in self.sparks.iter_mut().enumerate() {
            let profile = if show { profiles.get(index) } else { None };
            let heights = profile.map(|profile| spark_heights(profile));
            for (bar, mark) in group.iter_mut().enumerate() {
                let Some(heights) = heights else {
                    mark.set_visible(false);
                    continue;
                };
                mark.set_visible(true);
                let height = 6.0 + heights[bar] * 28.0;
                mark.set_custom_minimum_size(Vector2::new(4.0, height));
                mark.set_color(ui_style::SNOW);
            }
        }
    }

    fn paint_figures(&mut self, screen: Screen, figures: &[(String, String)]) {
        let show = screen == Screen::Summary && !figures.is_empty();
        for (index, label) in self.figures.iter_mut().enumerate() {
            if let Some((caption, value)) = figures.get(index).filter(|_| show) {
                label.set_text(&format!("{caption}    {value}"));
                label.set_visible(true);
            } else {
                label.set_visible(false);
            }
        }
    }

    /// Show one toast. A later call replaces it. `--no-hud` leaves this silent.
    pub fn toast(&mut self, text: &str, seconds: f64) {
        if !self.toasts {
            return;
        }
        self.toast.set_text(text);
        self.toast_panel.set_visible(true);
        self.toast_s = seconds;
    }

    /// Count down the toast.
    pub fn tick_toast(&mut self, dt: f64) {
        if self.toast_s <= 0.0 {
            return;
        }
        self.toast_s -= dt;
        if self.toast_s <= 0.0 {
            self.toast_panel.set_visible(false);
        }
    }
}

fn spark_heights(profile: &[(f32, f32)]) -> [f32; 12] {
    let mut heights = [0.0; 12];
    if profile.len() < 2 {
        return heights;
    }
    let min = profile.iter().map(|point| point.1).fold(f32::MAX, f32::min);
    let max = profile.iter().map(|point| point.1).fold(f32::MIN, f32::max);
    let span = (max - min).max(1.0);
    let start = profile[0].0;
    let end = profile[profile.len() - 1].0;
    for (index, height) in heights.iter_mut().enumerate() {
        let distance = start + (end - start) * (index as f32) / 11.0;
        let mut elevation = profile[0].1;
        for window in profile.windows(2) {
            if distance <= window[1].0 {
                let width = (window[1].0 - window[0].0).max(0.001);
                let t = (distance - window[0].0) / width;
                elevation = window[0].1 + (window[1].1 - window[0].1) * t;
                break;
            }
            elevation = window[1].1;
        }
        *height = (elevation - min) / span;
    }
    heights
}

/// Load the title badge. `None` when the SVG cannot be read at run time.
pub fn load_logo() -> Option<Gd<ImageTexture>> {
    let root = godot::classes::ProjectSettings::singleton()
        .globalize_path("res://")
        .to_string();
    let path = format!("{root}../docs/img/logo.svg");
    let mut image = Image::new_gd();
    if image.load(&GString::from(path.as_str())) != godot::global::Error::OK {
        return None;
    }
    let mut texture = ImageTexture::new_gd();
    texture.set_image(&image);
    Some(texture)
}
