//! Title, pause, summary, course list, confirm, and toasts.
//!
//! Colours are the `docs/ui.md` tokens. When `ui_style` is present the
//! drawing goes through it.

use godot::classes::control::{LayoutPreset, MouseFilter};
use godot::classes::{
    CanvasLayer, ColorRect, Label, Node3D, PanelContainer, StyleBoxEmpty, VBoxContainer,
};
use godot::obj::Gd;
use godot::prelude::*;
use pp_view::shell::{Screen, ShellView};

use crate::ui_style::{self, Role, Surface};

/// Overlay for every screen that is not the settings list.
pub struct Screens {
    layer: Gd<CanvasLayer>,
    scrim: Gd<ColorRect>,
    column: Gd<VBoxContainer>,
    wordmark: Gd<Label>,
    wordmark_pass: Gd<Label>,
    title: Gd<Label>,
    rows: Vec<Gd<Label>>,
    row_panels: Vec<Gd<PanelContainer>>,
    detail: Gd<Label>,
    hints: Gd<Label>,
    toast_panel: Gd<PanelContainer>,
    toast: Gd<Label>,
    toast_s: f64,
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

        let mut box_ = VBoxContainer::new_alloc();
        box_.set_name("Rows");
        box_.set_position(Vector2::new(72.0, 250.0));
        box_.set_custom_minimum_size(Vector2::new(440.0, 0.0));
        box_.add_theme_constant_override("separation", 8);

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
        for index in 0..8 {
            let mut row_panel = PanelContainer::new_alloc();
            row_panel.set_name(&format!("ScreenPanel{index}"));
            row_panel.set_custom_minimum_size(Vector2::new(400.0, 64.0));
            row_panel.add_theme_stylebox_override("panel", &StyleBoxEmpty::new_gd());
            let mut label = ui_style::label("", Role::MenuItem);
            label.set_name(&format!("ScreenRow{index}"));
            row_panel.add_child(&label);
            box_.add_child(&row_panel);
            rows.push(label);
            row_panels.push(row_panel);
        }

        let mut detail = ui_style::label("", Role::Body);
        detail.set_name("ScreenDetail");
        detail.add_theme_color_override("font_color", ui_style::MIST);
        box_.add_child(&detail);

        let mut hints = ui_style::label("Move    Select", Role::Unit);
        hints.set_name("Hints");
        box_.add_child(&hints);

        layer.add_child(&box_);

        let mut toast_panel = PanelContainer::new_alloc();
        toast_panel.set_name("ToastPanel");
        toast_panel.set_position(Vector2::new(360.0, 32.0));
        toast_panel.set_custom_minimum_size(Vector2::new(560.0, 48.0));
        toast_panel.add_theme_stylebox_override("panel", &ui_style::surface(Surface::Dialog));
        toast_panel.set_visible(false);
        let mut toast = ui_style::label("", Role::Body);
        toast.set_name("Toast");
        toast_panel.add_child(&toast);
        layer.add_child(&toast_panel);

        parent.add_child(&layer);
        Self {
            layer,
            scrim,
            column: box_.clone(),
            wordmark,
            wordmark_pass,
            title,
            rows,
            row_panels,
            detail,
            hints,
            toast_panel,
            toast,
            toast_s: 0.0,
        }
    }

    /// Draw the current shell view. The ride itself hides this layer.
    pub fn show(&mut self, view: &ShellView) {
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
        self.column
            .set_position(Vector2::new(72.0, if on_title { 80.0 } else { 160.0 }));
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
    }

    /// Show one toast. A later call replaces it.
    pub fn toast(&mut self, text: &str, seconds: f64) {
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
