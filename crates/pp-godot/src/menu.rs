//! Settings overlay. The ride keeps ticking underneath.
//!
//! The choices live in `pp-view`. This file draws them and applies the result
//! to the session. Keyboard: arrows, Enter, Escape. Controller: D-pad or left
//! stick, A, B. Escape and Start open and close it.

use std::path::Path;

use godot::builtin::Side;
use godot::classes::control::{LayoutPreset, MouseFilter, SizeFlags};
use godot::classes::{CanvasLayer, Label, Node3D, PanelContainer, VBoxContainer};
use godot::obj::Gd;
use godot::prelude::*;
use pp_session::{Session, Settings};
use pp_view::menu::{Action, Device, MenuState, MenuView, Snapshot};

const ROW_COUNT: usize = 12;
const FONT: i32 = 22;
const TITLE_FONT: i32 = 28;
const DETAIL_FONT: i32 = 20;

/// What the player just pressed.
pub use pp_view::menu::Command;

/// What the menu asked the game to do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Stay as we are.
    None,
    /// The player confirmed quit.
    Quit,
    /// The ride was restarted.
    Restarted,
}

/// The overlay and the row that is selected.
pub struct RideMenu {
    layer: Gd<CanvasLayer>,
    title: Gd<Label>,
    rows: Vec<Gd<Label>>,
    detail: Gd<Label>,
    state: MenuState,
}

impl RideMenu {
    /// Build a hidden overlay on `parent`.
    pub fn new(parent: &mut Gd<Node3D>) -> Self {
        let mut layer = CanvasLayer::new_alloc();
        layer.set_name("Menu");
        layer.set_layer(20);
        layer.set_visible(false);

        let mut panel = PanelContainer::new_alloc();
        panel.set_name("Panel");
        panel.set_anchors_preset(LayoutPreset::FULL_RECT);
        panel.set_offset(Side::LEFT, 80.0);
        panel.set_offset(Side::TOP, 70.0);
        panel.set_offset(Side::RIGHT, -80.0);
        panel.set_offset(Side::BOTTOM, -70.0);
        panel.set_mouse_filter(MouseFilter::STOP);
        panel.add_theme_stylebox_override("panel", &panel_style());

        let mut box_ = VBoxContainer::new_alloc();
        box_.set_name("Rows");
        box_.add_theme_constant_override("separation", 6);
        box_.set_h_size_flags(SizeFlags::EXPAND_FILL);

        let mut title = Label::new_alloc();
        title.set_text("Settings");
        title.add_theme_font_size_override("font_size", TITLE_FONT);
        title.add_theme_color_override("font_color", Color::from_rgb(0.96, 0.94, 0.88));
        box_.add_child(&title);

        let mut rows = Vec::with_capacity(ROW_COUNT);
        for index in 0..ROW_COUNT {
            let mut label = Label::new_alloc();
            label.set_name(&format!("Row{index}"));
            label.add_theme_font_size_override("font_size", FONT);
            box_.add_child(&label);
            rows.push(label);
        }

        let mut detail = Label::new_alloc();
        detail.set_name("Detail");
        detail.add_theme_font_size_override("font_size", DETAIL_FONT);
        detail.add_theme_color_override("font_color", Color::from_rgb(0.78, 0.82, 0.74));
        box_.add_child(&detail);

        let mut footer = Label::new_alloc();
        footer.set_text("Arrows move    Enter selects    Esc goes back");
        footer.add_theme_font_size_override("font_size", DETAIL_FONT);
        footer.add_theme_color_override("font_color", Color::from_rgb(0.62, 0.66, 0.7));
        box_.add_child(&footer);

        panel.add_child(&box_);
        layer.add_child(&panel);
        parent.add_child(&layer);

        Self {
            layer,
            title,
            rows,
            detail,
            state: MenuState::default(),
        }
    }

    /// Whether the overlay is showing.
    pub fn is_open(&self) -> bool {
        self.state.open
    }

    /// Page title, for the smoke test.
    pub fn title(&self) -> String {
        self.title.get_text().to_string()
    }

    /// Explanation under the rows.
    pub fn detail(&self) -> String {
        self.detail.get_text().to_string()
    }

    /// Text of the highlighted row.
    pub fn selected_text(&self) -> String {
        self.rows
            .iter()
            .find(|label| label.get_text().to_string().starts_with('>'))
            .map(|label| label.get_text().to_string())
            .unwrap_or_default()
    }

    /// Show the main page.
    pub fn open(&mut self, session: &Session) {
        self.state.open_main();
        self.layer.set_visible(true);
        self.refresh(session);
    }

    /// Hide the overlay.
    pub fn close(&mut self) {
        self.state.close();
        self.layer.set_visible(false);
    }

    /// Apply one command. Saves when a value changes.
    pub fn handle(&mut self, command: Command, session: &mut Session, path: &Path) -> Effect {
        let snap = snapshot(session);
        let action = self.state.handle(command, &snap);
        let effect = self.perform(action, session, path);
        self.refresh(session);
        effect
    }

    /// Redraw from the live session. Call this every frame the menu is open.
    pub fn refresh(&mut self, session: &Session) {
        if !self.state.open {
            return;
        }
        let snap = snapshot(session);
        let view = self.state.view(&snap);
        self.paint(&view);
    }

    fn perform(&mut self, action: Action, session: &mut Session, path: &Path) -> Effect {
        match action {
            Action::None => Effect::None,
            Action::Quit => Effect::Quit,
            Action::Restart => {
                session.restart();
                Effect::Restarted
            }
            Action::Scan => {
                session.start_scan();
                Effect::None
            }
            Action::Connect(id) => {
                let _ = session.connect(&id);
                Effect::None
            }
            Action::Disconnect => {
                session.disconnect();
                self.save(session, path);
                Effect::None
            }
            Action::Apply(settings) => {
                let fullscreen = settings.fullscreen;
                let changed = fullscreen != session.settings().fullscreen;
                session.apply_settings(settings);
                if changed {
                    apply_window_mode(fullscreen);
                }
                self.save(session, path);
                Effect::None
            }
        }
    }

    fn paint(&mut self, view: &MenuView) {
        self.title.set_text(&view.title);
        for (index, label) in self.rows.iter_mut().enumerate() {
            if let Some(line) = view.lines.get(index) {
                let shown = if index == view.selected {
                    format!("> {line}")
                } else {
                    format!("  {line}")
                };
                label.set_text(&shown);
                label.set_visible(true);
                let color = if index == view.selected {
                    Color::from_rgb(1.0, 0.86, 0.45)
                } else {
                    Color::from_rgb(0.9, 0.92, 0.94)
                };
                label.add_theme_color_override("font_color", color);
            } else {
                label.set_text("");
                label.set_visible(false);
            }
        }
        self.detail.set_text(&view.detail);
    }

    fn save(&mut self, session: &Session, path: &Path) {
        match session.settings().save(path) {
            Ok(()) => self.state.save_error.clear(),
            Err(err) => self.state.save_error = format!("Could not save settings: {err}"),
        }
    }
}

fn snapshot(session: &Session) -> Snapshot {
    let status = session.status();
    let settings: Settings = session.settings().clone();
    Snapshot {
        settings,
        status_line: status.line,
        dashboard: status.dashboard,
        dashboard_urls: status.dashboard_urls,
        dashboard_error: status.dashboard_error,
        devices: session
            .discovered()
            .into_iter()
            .map(|device| Device {
                id: device.id,
                name: device.name,
                rssi_dbm: device.rssi_dbm,
            })
            .collect(),
    }
}

/// Apply the fullscreen flag to the game window.
pub fn apply_window_mode(fullscreen: bool) {
    let Some(loop_) = godot::classes::Engine::singleton().get_main_loop() else {
        return;
    };
    let Ok(tree) = loop_.try_cast::<godot::classes::SceneTree>() else {
        return;
    };
    let mut window = tree.get_root();
    let mode = if fullscreen {
        godot::classes::window::Mode::FULLSCREEN
    } else {
        godot::classes::window::Mode::WINDOWED
    };
    window.set_mode(mode);
}

fn panel_style() -> Gd<godot::classes::StyleBoxFlat> {
    let mut style = godot::classes::StyleBoxFlat::new_gd();
    style.set_bg_color(Color::from_rgba(0.06, 0.08, 0.12, 0.92));
    style.set_border_color(Color::from_rgba(0.85, 0.78, 0.55, 0.45));
    style.set_border_width_all(2);
    style.set_corner_radius_all(12);
    style.set_content_margin_all(18.0);
    style
}
