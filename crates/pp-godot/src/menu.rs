//! Settings overlay. The ride keeps ticking underneath.
//!
//! The choices live in `pp-view`. This file draws them and applies the result
//! to the session. Keyboard: arrows, Enter, Escape. Controller: D-pad or left
//! stick, A, B. Escape and Start open and close it.

use std::path::Path;

use godot::builtin::Side;
use godot::classes::control::{LayoutPreset, MouseFilter, SizeFlags};
use godot::classes::{CanvasLayer, Label, Node3D, PanelContainer, StyleBoxEmpty, VBoxContainer};

use crate::ui_style::{self, Role, Surface};
use godot::obj::Gd;
use godot::prelude::*;
use pp_session::{Session, Settings};
use pp_view::menu::{Action, Device, MenuState, MenuView, Snapshot};

const ROW_COUNT: usize = 12;

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
    /// The music-folder row asked for a directory picker.
    ChooseMusicFolder,
}

/// The overlay and the row that is selected.
pub struct RideMenu {
    layer: Gd<CanvasLayer>,
    title: Gd<Label>,
    rows: Vec<Gd<Label>>,
    row_panels: Vec<Gd<PanelContainer>>,
    detail: Gd<Label>,
    selected: usize,
    state: MenuState,
    music_report: String,
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
        panel.add_theme_stylebox_override("panel", &ui_style::surface(Surface::Menu));

        let mut box_ = VBoxContainer::new_alloc();
        box_.set_name("Rows");
        box_.add_theme_constant_override("separation", 2);
        box_.set_h_size_flags(SizeFlags::EXPAND_FILL);

        let title = ui_style::label("Settings", Role::Title);
        box_.add_child(&title);

        let mut rows = Vec::with_capacity(ROW_COUNT);
        let mut row_panels = Vec::with_capacity(ROW_COUNT);
        for index in 0..ROW_COUNT {
            let mut row_panel = PanelContainer::new_alloc();
            row_panel.set_name(&format!("RowPanel{index}"));
            row_panel.set_custom_minimum_size(Vector2::new(0.0, 48.0));
            row_panel.add_theme_stylebox_override("panel", &StyleBoxEmpty::new_gd());
            let mut label = ui_style::label("", Role::Row);
            label.set_name(&format!("Row{index}"));
            row_panel.add_child(&label);
            box_.add_child(&row_panel);
            rows.push(label);
            row_panels.push(row_panel);
        }

        let mut detail = ui_style::label("", Role::Body);
        detail.set_name("Detail");
        detail.add_theme_color_override("font_color", ui_style::MIST);
        box_.add_child(&detail);

        let footer = ui_style::label("Move    Select    Back", Role::Unit);
        box_.add_child(&footer);

        panel.add_child(&box_);
        layer.add_child(&panel);
        parent.add_child(&layer);

        Self {
            layer,
            title,
            rows,
            row_panels,
            detail,
            selected: 0,
            state: MenuState::default(),
            music_report: String::new(),
        }
    }

    /// Whether the overlay is showing.
    pub fn is_open(&self) -> bool {
        self.state.open
    }

    /// Whether the settings panel is actually on screen.
    pub fn is_visible(&self) -> bool {
        self.layer.is_visible()
    }

    /// Folder scan line shown under the music folder row.
    pub fn set_music_report(&mut self, report: &str) {
        self.music_report = report.to_owned();
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
            .get(self.selected)
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
        let snap = snapshot(session, &self.music_report);
        let action = self.state.handle(command, &snap);
        let effect = self.perform(action, session, path);
        self.refresh(session);
        // Closing the state used to leave the panel drawn. Input had already
        // returned to the ride, so Escape looked like it did nothing.
        if !self.state.open {
            self.layer.set_visible(false);
        }
        effect
    }

    /// Redraw from the live session. Call this every frame the menu is open.
    pub fn refresh(&mut self, session: &Session) {
        if !self.state.open {
            self.layer.set_visible(false);
            return;
        }
        let snap = snapshot(session, &self.music_report);
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
                session.connect(&id);
                Effect::None
            }
            Action::Disconnect => {
                session.disconnect();
                self.save(session, path);
                Effect::None
            }
            Action::ChooseMusicFolder => Effect::ChooseMusicFolder,
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
        ui_style::style_label(&mut self.title, Role::Title);
        self.selected = view.selected;
        for (index, label) in self.rows.iter_mut().enumerate() {
            let Some(line) = view.lines.get(index) else {
                label.set_text("");
                label.set_visible(false);
                if let Some(panel) = self.row_panels.get_mut(index) {
                    panel.set_visible(false);
                }
                continue;
            };
            let focused = index == view.selected;
            label.set_text(line);
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
        self.detail
            .add_theme_color_override("font_color", ui_style::MIST);
    }

    fn save(&mut self, session: &Session, path: &Path) {
        match session.settings().save(path) {
            Ok(()) => self.state.save_error.clear(),
            Err(err) => self.state.save_error = format!("Could not save settings: {err}"),
        }
    }
}

fn snapshot(session: &Session, music_report: &str) -> Snapshot {
    let status = session.status();
    let settings: Settings = session.settings().clone();
    Snapshot {
        settings,
        status_line: status.line,
        dashboard: status.dashboard,
        dashboard_urls: status.dashboard_urls,
        dashboard_error: status.dashboard_error,
        music_report: music_report.to_owned(),
        devices: session
            .discovered()
            .into_iter()
            .map(|device| Device {
                id: device.id,
                name: device.name,
                rssi_dbm: device.rssi_dbm,
                power_only: !device.ftms && !device.fec,
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
