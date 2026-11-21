//! Title, pause, summary, course list, confirm, and toasts.
//!
//! Colours are the `docs/ui.md` tokens. When `ui_style` is present the
//! drawing goes through it.

use godot::classes::control::{LayoutPreset, MouseFilter};
use godot::classes::{
    CanvasLayer, ColorRect, HBoxContainer, Image, ImageTexture, Label, Node3D, PanelContainer,
    ScrollContainer, StyleBoxEmpty, TextureRect, VBoxContainer,
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
    title: Gd<Label>,
    rows: Vec<Gd<Label>>,
    row_panels: Vec<Gd<PanelContainer>>,
    detail: Gd<Label>,
    hints: Gd<Label>,
    figures: Vec<Gd<Label>>,
    saved: Gd<Label>,
    saved_folder: Gd<Label>,
    sparks: Vec<[Gd<ColorRect>; 12]>,
    progress_scroll: Gd<ScrollContainer>,
    progress_totals: Gd<Label>,
    progress_badges: Vec<BadgeBlock>,
    progress_page: ProgressPage,
    workout_samples: Vec<Vec<f32>>,
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
        card.set_position(Vector2::new(24.0, 24.0));
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
        box_.add_theme_constant_override("separation", 4);

        let mut lockup = godot::classes::HBoxContainer::new_alloc();
        lockup.set_name("Lockup");
        lockup.add_theme_constant_override("separation", 16);
        lockup.set_h_size_flags(godot::classes::control::SizeFlags::SHRINK_BEGIN);

        let mut logo = TextureRect::new_alloc();
        logo.set_name("Logo");
        logo.set_custom_minimum_size(Vector2::new(84.0, 84.0));
        logo.set_h_size_flags(godot::classes::control::SizeFlags::SHRINK_CENTER);
        logo.set_v_size_flags(godot::classes::control::SizeFlags::SHRINK_CENTER);
        logo.set_expand_mode(godot::classes::texture_rect::ExpandMode::IGNORE_SIZE);
        logo.set_stretch_mode(godot::classes::texture_rect::StretchMode::KEEP_ASPECT_CENTERED);
        if let Some(texture) = load_logo() {
            logo.set_texture(&texture);
        } else {
            logo.set_visible(false);
        }
        lockup.add_child(&logo);

        let mut wordmark = ui_style::label("PENGUIN PASS", Role::Title);
        wordmark.set_name("Wordmark");
        wordmark.add_theme_font_size_override("font_size", 40);
        wordmark.add_theme_color_override("font_color", ui_style::SNOW);
        wordmark.add_theme_color_override("font_outline_color", ui_style::INK);
        wordmark.add_theme_constant_override("outline_size", 10);
        wordmark.add_theme_color_override(
            "font_shadow_color",
            ui_style::with_alpha(ui_style::INK, 0.55),
        );
        wordmark.add_theme_constant_override("shadow_offset_x", 0);
        wordmark.add_theme_constant_override("shadow_offset_y", 4);
        wordmark.add_theme_constant_override("shadow_outline_size", 6);
        wordmark.set_v_size_flags(godot::classes::control::SizeFlags::SHRINK_CENTER);
        lockup.add_child(&wordmark);
        box_.add_child(&lockup);

        let mut title = ui_style::label("", Role::Title);
        title.set_name("ScreenTitle");
        box_.add_child(&title);

        let mut rows = Vec::new();
        let mut row_panels = Vec::new();
        let mut sparks = Vec::new();
        for index in 0..16 {
            let mut row_panel = PanelContainer::new_alloc();
            row_panel.set_name(&format!("ScreenPanel{index}"));
            row_panel.set_custom_minimum_size(Vector2::new(400.0, 40.0));
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
        let mut progress_scroll = ScrollContainer::new_alloc();
        progress_scroll.set_name("ProgressScroll");
        progress_scroll.set_custom_minimum_size(Vector2::new(480.0, 520.0));
        progress_scroll.set_visible(false);
        progress_scroll
            .set_horizontal_scroll_mode(godot::classes::scroll_container::ScrollMode::SHOW_NEVER);
        let mut progress_list = VBoxContainer::new_alloc();
        progress_list.set_name("ProgressList");
        progress_list.add_theme_constant_override("separation", 10);
        let mut progress_totals = ui_style::label("", Role::Body);
        progress_totals.set_name("ProgressTotals");
        progress_totals.add_theme_color_override("font_color", ui_style::MIST);
        progress_list.add_child(&progress_totals);
        let mut progress_badges = Vec::new();
        for index in 0..24 {
            progress_badges.push(BadgeBlock::new(&mut progress_list, index));
        }
        progress_scroll.add_child(&progress_list);
        box_.add_child(&progress_scroll);

        for index in 0..14 {
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
        // Top-right, growing left, so it stays off the menu column.
        toast_panel.set_anchor(godot::builtin::Side::LEFT, 1.0);
        toast_panel.set_anchor(godot::builtin::Side::RIGHT, 1.0);
        toast_panel.set_anchor(godot::builtin::Side::TOP, 0.0);
        toast_panel.set_anchor(godot::builtin::Side::BOTTOM, 0.0);
        toast_panel.set_offset(godot::builtin::Side::LEFT, -24.0);
        toast_panel.set_offset(godot::builtin::Side::RIGHT, -24.0);
        toast_panel.set_offset(godot::builtin::Side::TOP, 24.0);
        toast_panel.set_offset(godot::builtin::Side::BOTTOM, 72.0);
        toast_panel.set_h_grow_direction(godot::classes::control::GrowDirection::BEGIN);
        toast_panel.set_v_grow_direction(godot::classes::control::GrowDirection::END);
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
            title,
            rows,
            row_panels,
            detail,
            hints,
            figures,
            saved,
            saved_folder,
            sparks,
            progress_scroll,
            progress_totals,
            progress_badges,
            progress_page: ProgressPage::default(),
            workout_samples: Vec::new(),
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
            self.card.set_position(Vector2::new(24.0, 24.0));
            self.card
                .add_theme_stylebox_override("panel", &StyleBoxEmpty::new_gd());
        } else {
            self.card.set_position(Vector2::new(24.0, 24.0));
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
        let progress = view.screen == Screen::Progress;
        self.progress_scroll.set_visible(progress);
        if progress {
            for panel in &mut self.row_panels {
                panel.set_visible(false);
            }
            self.paint_progress(view.selected);
            let offset = (view.selected as i32).saturating_sub(2) * 72;
            self.progress_scroll.set_v_scroll(offset.max(0));
        }
        self.detail.set_text(&view.detail);
        self.hints
            .set_visible(!matches!(view.screen, Screen::Riding | Screen::Settings));
        self.paint_profiles(view, profiles);
        self.paint_workout_bars(view.screen);
        self.paint_figures(view.screen, figures);
        if view.screen != Screen::Summary {
            self.saved.set_visible(false);
            self.saved_folder.set_visible(false);
        }
        self.fit_card();
    }

    /// Twelve samples per workout row. Empty means no bars.
    pub fn set_workout_samples(&mut self, samples: Vec<Vec<f32>>) {
        self.workout_samples = samples;
    }

    /// Totals and badges for the Progress screen.
    pub fn set_progress_page(&mut self, page: ProgressPage) {
        self.progress_page = page;
    }

    fn paint_progress(&mut self, selected: usize) {
        self.progress_totals.set_text(&self.progress_page.totals);
        let badges = self.progress_page.badges.clone();
        for (index, block) in self.progress_badges.iter_mut().enumerate() {
            let Some(badge) = badges.get(index) else {
                block.box_.set_visible(false);
                continue;
            };
            block.box_.set_visible(true);
            let mark = if badge.earned { "    ✓" } else { "" };
            block.title.set_text(&format!("{}{mark}", badge.name));
            block.title.add_theme_color_override(
                "font_color",
                if badge.earned {
                    ui_style::GOOD
                } else if index == selected {
                    ui_style::SUN
                } else {
                    ui_style::SNOW
                },
            );
            block.blurb.set_text(&badge.description);
            block.blurb.set_visible(!badge.description.is_empty());
            let width = 440.0 * badge.fraction.clamp(0.0, 1.0);
            block.fill.set_size(Vector2::new(width, 8.0));
            block.fill.set_visible(width > 1.0);
            if badge.kit.is_empty() {
                block.kit.set_visible(false);
            } else {
                block.kit.set_text(&badge.kit);
                block.kit.set_visible(true);
            }
        }
    }

    /// Drop any leftover height from the previous screen.
    fn fit_card(&mut self) {
        let minimum = self.card.get_combined_minimum_size();
        self.card.set_size(minimum);
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
        self.fit_card();
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

    fn paint_workout_bars(&mut self, screen: Screen) {
        let show = screen == Screen::Workout;
        for (index, group) in self.sparks.iter_mut().enumerate() {
            let samples = if show {
                self.workout_samples.get(index)
            } else {
                None
            };
            for (bar, mark) in group.iter_mut().enumerate() {
                let Some(value) = samples.and_then(|row| row.get(bar)).copied() else {
                    if show {
                        mark.set_visible(false);
                    }
                    continue;
                };
                mark.set_visible(true);
                let height = if value < 0.0 {
                    8.0
                } else {
                    8.0 + value.clamp(0.15, 1.5) * 20.0
                };
                mark.set_custom_minimum_size(Vector2::new(6.0, height));
                mark.set_color(if value < 0.0 {
                    ui_style::MIST
                } else if value < 0.76 {
                    ui_style::GOOD
                } else if value < 0.91 {
                    ui_style::WARN
                } else if value < 1.06 {
                    ui_style::SUN
                } else {
                    ui_style::BAD
                });
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
    let path = format!("{root}assets/logo-mark.svg");
    let mut image = Image::new_gd();
    if image.load(&GString::from(path.as_str())) != godot::global::Error::OK {
        return None;
    }
    let mut texture = ImageTexture::new_gd();
    texture.set_image(&image);
    Some(texture)
}

/// Lifetime totals and the badge list for the Progress screen.
#[derive(Clone, Debug, Default)]
pub struct ProgressPage {
    /// Two short lines, not a column of menu rows.
    pub totals: String,
    /// Every badge, earned or not.
    pub badges: Vec<ProgressBadge>,
}

/// One badge on the Progress screen.
#[derive(Clone, Debug)]
pub struct ProgressBadge {
    /// Badge name.
    pub name: String,
    /// One sentence, drawn in the muted colour.
    pub description: String,
    /// The rider has earned it.
    pub earned: bool,
    /// 0 to 1. Earned badges are full.
    pub fraction: f32,
    /// `Unlocks Sunrise kit`, or empty.
    pub kit: String,
}

struct BadgeBlock {
    box_: Gd<VBoxContainer>,
    title: Gd<Label>,
    blurb: Gd<Label>,
    fill: Gd<ColorRect>,
    kit: Gd<Label>,
}

impl BadgeBlock {
    fn new(parent: &mut Gd<VBoxContainer>, index: usize) -> Self {
        let mut box_ = VBoxContainer::new_alloc();
        box_.set_name(&format!("Badge{index}"));
        box_.add_theme_constant_override("separation", 2);
        let title = ui_style::label("", Role::MenuItem);
        let mut blurb = ui_style::label("", Role::Caption);
        blurb.add_theme_color_override("font_color", ui_style::MIST);
        let mut track = ColorRect::new_alloc();
        track.set_custom_minimum_size(Vector2::new(440.0, 8.0));
        track.set_color(ui_style::with_alpha(ui_style::SNOW, 0.22));
        let mut fill = ColorRect::new_alloc();
        fill.set_color(ui_style::SUN);
        fill.set_size(Vector2::new(0.0, 8.0));
        track.add_child(&fill);
        let mut kit = ui_style::label("", Role::Unit);
        kit.add_theme_color_override("font_color", ui_style::MIST);
        box_.add_child(&title);
        box_.add_child(&blurb);
        box_.add_child(&track);
        box_.add_child(&kit);
        parent.add_child(&box_);
        Self {
            box_,
            title,
            blurb,
            fill,
            kit,
        }
    }
}
