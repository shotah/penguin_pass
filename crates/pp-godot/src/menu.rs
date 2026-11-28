//! Settings overlay. The ride keeps ticking underneath.
//!
//! The choices live in `pp-view`. This file draws them and applies the result
//! to the session. Keyboard, controller, and a row click all move focus and
//! accept. Escape and Start open and close it.

use std::path::Path;

use godot::builtin::Side;
use godot::classes::control::{LayoutPreset, MouseFilter, SizeFlags};
use godot::classes::{
    CanvasLayer, ColorRect, HBoxContainer, Label, Node3D, PanelContainer, ScrollContainer,
    StyleBoxEmpty, VBoxContainer,
};

use crate::ui_style::{self, Role, Surface};
use godot::obj::Gd;
use godot::prelude::*;
use pp_session::{Session, Settings};
use pp_view::menu::{Action, Device, MenuState, MenuView, Snapshot};

const ROW_COUNT: usize = 26;

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
    /// Show the rides folder.
    OpenRides,
}

/// The overlay and the row that is selected.
pub struct RideMenu {
    layer: Gd<CanvasLayer>,
    title: Gd<Label>,
    rows: Vec<Gd<Label>>,
    row_panels: Vec<Gd<PanelContainer>>,
    bars: Vec<[Gd<ColorRect>; 4]>,
    scroll: Gd<ScrollContainer>,
    detail: Gd<Label>,
    footer: Gd<Label>,
    selected: usize,
    state: MenuState,
    music_report: String,
    controller_status: String,
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
        panel.set_offset(Side::LEFT, 36.0);
        panel.set_offset(Side::TOP, 28.0);
        panel.set_offset(Side::RIGHT, -36.0);
        panel.set_offset(Side::BOTTOM, -28.0);
        panel.set_mouse_filter(MouseFilter::STOP);
        let mut panel_style = ui_style::surface(Surface::Menu);
        ui_style::pad_box(
            &mut panel_style,
            ui_style::space::PANEL as f32,
            ui_style::space::PANEL as f32,
        );
        panel.add_theme_stylebox_override("panel", &panel_style);

        let mut box_ = VBoxContainer::new_alloc();
        box_.set_name("Rows");
        box_.add_theme_constant_override("separation", ui_style::space::GAP);
        box_.set_h_size_flags(SizeFlags::EXPAND_FILL);

        let title = ui_style::label("Settings", Role::Title);
        box_.add_child(&title);

        let mut scroll = ScrollContainer::new_alloc();
        scroll.set_name("RowScroll");
        scroll.set_h_size_flags(SizeFlags::EXPAND_FILL);
        scroll.set_v_size_flags(SizeFlags::EXPAND_FILL);
        scroll.set_horizontal_scroll_mode(godot::classes::scroll_container::ScrollMode::SHOW_NEVER);
        let mut list = VBoxContainer::new_alloc();
        list.set_name("RowList");
        list.add_theme_constant_override("separation", 2);
        list.set_h_size_flags(SizeFlags::EXPAND_FILL);
        list.set_mouse_filter(MouseFilter::PASS);

        let mut rows = Vec::with_capacity(ROW_COUNT);
        let mut row_panels = Vec::with_capacity(ROW_COUNT);
        let mut bars = Vec::with_capacity(ROW_COUNT);
        for index in 0..ROW_COUNT {
            let mut row_panel = PanelContainer::new_alloc();
            row_panel.set_name(&format!("RowPanel{index}"));
            row_panel.set_custom_minimum_size(Vector2::new(0.0, 36.0));
            row_panel.add_theme_stylebox_override("panel", &StyleBoxEmpty::new_gd());
            let mut row_box = HBoxContainer::new_alloc();
            row_box.add_theme_constant_override("separation", 4);
            let mut label = ui_style::label("", Role::Row);
            label.set_name(&format!("Row{index}"));
            label.set_h_size_flags(SizeFlags::EXPAND_FILL);
            row_box.add_child(&label);
            let signal: [Gd<ColorRect>; 4] = std::array::from_fn(|bar| {
                let mut mark = ColorRect::new_alloc();
                let height = 8.0 + (bar as f32) * 4.0;
                mark.set_custom_minimum_size(Vector2::new(5.0, height));
                mark.set_color(ui_style::with_alpha(ui_style::SNOW, 0.25));
                mark.set_visible(false);
                mark
            });
            for mark in &signal {
                row_box.add_child(mark);
            }
            row_panel.add_child(&row_box);
            row_panel.set_mouse_filter(MouseFilter::PASS);
            let row: Gd<godot::classes::Control> = row_panel.clone().upcast();
            ui_style::ignore_pointer_descendants(&row);
            list.add_child(&row_panel);
            bars.push(signal);
            rows.push(label);
            row_panels.push(row_panel);
        }

        scroll.add_child(&list);
        box_.add_child(&scroll);

        let mut detail = ui_style::label("", Role::Body);
        detail.set_name("Detail");
        detail.add_theme_color_override("font_color", ui_style::MIST);
        box_.add_child(&detail);

        let mut footer = ui_style::label(
            &pp_view::prompts::menu_footer(pp_view::prompts::InputMode::Keyboard),
            Role::Unit,
        );
        footer.set_name("Footer");
        box_.add_child(&footer);

        panel.add_child(&box_);
        layer.add_child(&panel);
        parent.add_child(&layer);

        Self {
            layer,
            title,
            rows,
            row_panels,
            bars,
            scroll,
            detail,
            footer,
            selected: 0,
            state: MenuState::default(),
            music_report: String::new(),
            controller_status: String::new(),
        }
    }

    /// Footer naming the buttons for the device used last.
    pub fn set_footer(&mut self, text: &str) {
        if self.footer.get_text() != text {
            self.footer.set_text(text);
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

    /// Move the highlight to the row under the pointer.
    pub fn hover(&mut self, index: usize) {
        self.state.hover(index);
    }

    /// Tell each row to report hover and a left click.
    pub fn bind_pointer(&mut self, game: &Gd<crate::game::Game>) {
        for (index, panel) in self.row_panels.iter().enumerate() {
            let mut panel = panel.clone();
            let index = Variant::from(index as i64);
            let hover = Callable::from_object_method(game, "on_menu_hover")
                .bind(std::slice::from_ref(&index));
            let click = Callable::from_object_method(game, "on_menu_click").bind(&[index]);
            let _ = panel.connect(&StringName::from("mouse_entered"), &hover);
            let _ = panel.connect(&StringName::from("gui_input"), &click);
        }
    }

    /// Status word on the Zwift controllers row.
    pub fn set_controller_status(&mut self, status: &str) {
        self.controller_status = status.to_owned();
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

    /// Show the heart-rate list.
    pub fn show_heart(&mut self, session: &mut Session) {
        self.state.open_heart();
        self.layer.set_visible(true);
        self.refresh(session);
    }

    /// Show the main page.
    pub fn open(&mut self, session: &mut Session) {
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
        let status = self.controller_status.clone();
        let report = self.music_report.clone();
        let snap = snapshot(session, &report, &status);
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
    pub fn refresh(&mut self, session: &mut Session) {
        if !self.state.open {
            self.layer.set_visible(false);
            return;
        }
        let status = self.controller_status.clone();
        let report = self.music_report.clone();
        let snap = snapshot(session, &report, &status);
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
            Action::OpenRides => Effect::OpenRides,
            Action::Strava => {
                on_strava_click(session);
                Effect::None
            }
            Action::OpenHeart => Effect::None,
            Action::ConnectHeart(id) => {
                session.connect_heart_rate(&id);
                self.save(session, path);
                Effect::None
            }
            Action::ForgetHeart => {
                session.forget_heart_rate();
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
            let level = view.bars.get(index).copied().flatten();
            if let Some(group) = self.bars.get_mut(index) {
                for (bar, mark) in group.iter_mut().enumerate() {
                    let Some(level) = level else {
                        mark.set_visible(false);
                        continue;
                    };
                    mark.set_visible(true);
                    let lit = bar < usize::from(level);
                    mark.set_color(if lit {
                        ui_style::GOOD
                    } else {
                        ui_style::with_alpha(ui_style::SNOW, 0.25)
                    });
                }
            }
        }
        self.detail.set_text(&view.detail);
        self.detail.add_theme_color_override(
            "font_color",
            if view.warn {
                ui_style::WARN
            } else {
                ui_style::MIST
            },
        );
        if let Some(panel) = self.row_panels.get(view.selected) {
            self.scroll.ensure_control_visible(panel);
        }
    }

    fn save(&mut self, session: &Session, path: &Path) {
        match session.settings().save(path) {
            Ok(()) => self.state.save_error.clear(),
            Err(err) => self.state.save_error = format!("Could not save settings: {err}"),
        }
    }
}

fn heart_line(session: &Session) -> String {
    match session.heart_rate_state() {
        pp_session::ConnectionState::Scanning => "Scanning".to_owned(),
        pp_session::ConnectionState::Connecting => "Connecting".to_owned(),
        pp_session::ConnectionState::Connected => session
            .heart_rate_name()
            .unwrap_or_else(|| "Connected".to_owned()),
        pp_session::ConnectionState::Failed(reason) => reason,
        pp_session::ConnectionState::Disconnected => "Not paired".to_owned(),
    }
}

pub(crate) fn strava_row(state: &pp_session::StravaState) -> pp_view::extras::StravaRow {
    match state {
        pp_session::StravaState::NoKeys | pp_session::StravaState::NotConnected => {
            pp_view::extras::StravaRow::Connect
        }
        pp_session::StravaState::Connecting { .. } => pp_view::extras::StravaRow::Waiting,
        pp_session::StravaState::Connected { athlete } => {
            pp_view::extras::StravaRow::Connected(athlete.clone())
        }
        pp_session::StravaState::Error(reason) => {
            pp_view::extras::StravaRow::Failed(reason.clone())
        }
    }
}

fn on_strava_click(session: &mut Session) {
    match session.strava_state() {
        pp_session::StravaState::Connecting { .. } => session.cancel_strava_connect(),
        pp_session::StravaState::Connected { .. } => session.disconnect_strava(),
        pp_session::StravaState::NoKeys
        | pp_session::StravaState::NotConnected
        | pp_session::StravaState::Error(_) => {
            if let Some(url) = session.begin_strava_connect() {
                let _ = godot::classes::Os::singleton().shell_open(&GString::from(url.as_str()));
            }
        }
    }
}

fn kit_options(session: &Session) -> Vec<pp_view::menu::RiderOption> {
    let mut kits = vec![pp_view::menu::RiderOption {
        id: "default".to_owned(),
        name: pp_rider::Kit::get("default").name.to_owned(),
    }];
    for id in session.profile().unlocked_kits {
        let kit = pp_rider::Kit::get(&id);
        if kit.id == "default" {
            continue;
        }
        kits.push(pp_view::menu::RiderOption {
            id,
            name: kit.name.to_owned(),
        });
    }
    kits
}

fn snapshot(session: &mut Session, music_report: &str, controller_status: &str) -> Snapshot {
    let status = session.status();
    let connection = session.connection();
    let settings: Settings = session.settings().clone();
    Snapshot {
        settings,
        status_line: status.line,
        dashboard: status.dashboard,
        dashboard_urls: status.dashboard_urls,
        dashboard_error: status.dashboard_error,
        music_report: music_report.to_owned(),
        phase: connection.phase,
        failure_reason: connection.failure_reason.unwrap_or_default(),
        connected_name: connection.trainer_name.unwrap_or_default(),
        controller_status: controller_status.to_owned(),
        straps: session
            .heart_rate_entries()
            .into_iter()
            .map(|sensor| Device {
                id: sensor.id,
                name: sensor.name,
                rssi_dbm: sensor.rssi_dbm,
                power_only: false,
                bars: sensor.bars,
                ftms: false,
                cycling_power: false,
                fec: false,
                remembered: sensor.remembered,
            })
            .collect(),
        heart_line: heart_line(session),
        riders: pp_rider::RiderPreset::all()
            .into_iter()
            .map(|preset| pp_view::menu::RiderOption {
                id: preset.id.to_owned(),
                name: preset.name.to_owned(),
            })
            .collect(),
        kits: kit_options(session),
        strava: strava_row(&session.strava_state()),
        devices: session
            .pairing()
            .into_iter()
            .map(|device| Device {
                id: device.id,
                name: device.name,
                rssi_dbm: device.rssi_dbm,
                power_only: !device.ftms && !device.fec,
                bars: device.bars,
                ftms: device.ftms,
                cycling_power: device.cycling_power,
                fec: device.fec,
                remembered: device.remembered,
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
