//! Ride HUD. One console on the bottom edge, drawn in code.

use std::f32::consts::TAU;

use godot::builtin::Side;
use godot::classes::control::{LayoutPreset, MouseFilter};
use godot::classes::{CanvasLayer, Control, ICanvasLayer, IControl, StyleBoxFlat};
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;
use pp_core::Telemetry;
use pp_session::{DEFAULT_FTP_W, Session};
use pp_view::console::{
    Chip, ChipKind, ConsoleInput, ConsoleModel, refresh, write_connection, write_status_line,
};
use pp_view::profile::{Chart, Column, Plot, fill_next_km, fill_skyline, fill_window};

// Keep `format`'s re-exports referenced. Callers outside this file still use those names.
#[allow(unused_imports)]
use crate::format::{
    format_cadence_rpm, format_distance_km, format_elapsed, format_elevation_m, format_grade_pct,
    format_heart_rate, format_lap, format_power_w, format_speed_kph, grade_rgb,
};
use crate::ui_style::{
    self, BAD, DUSK, DesignFrame, GOOD, MIST, RIDDEN, SNOW, Surface, WARN, Weight, design_frame,
    surface,
};

/// Full-screen HUD layer.
/// Zwift-controller chip beside the trainer name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadBadge {
    /// The setting is off. Draw nothing.
    Off,
    /// Scanning or connecting.
    Scanning,
    /// How many controllers finished the handshake.
    Count(u8),
    /// Bluetooth is off, or the link failed.
    Unavailable,
}

#[derive(GodotClass)]
#[class(base = CanvasLayer)]
pub struct Hud {
    base: Base<CanvasLayer>,
    model: ConsoleModel,
    last: ConsoleInput,
    chip: Chip,
    scratch: String,
    status: String,
    tv_label: String,
    tv_address: String,
    tv_visible: bool,
    have_connection: bool,
    dirty: bool,
    profile_dirty: bool,
    ahead_dirty: bool,
    built: bool,
    is_loop: bool,
    length_m: f64,
    marker_m: f64,
    ahead_at: f64,
    spin: f32,
    hint_base: Option<f32>,
    hint_hidden: bool,
    show_lane: bool,
    fish_text: String,
    prompts: pp_view::prompts::RidePrompts,
    chrome: RideChrome,
    card_s: f32,
    pad: PadBadge,
    /// Compared in [`Self::present`] when the course picker swaps the route.
    course_name: String,
    points: Vec<(f64, f32)>,
    /// Reused `fill_ahead` buffer. Only [`Self::present`] fills it.
    ahead: Vec<pp_session::AheadPoint>,
    ahead_samples: Vec<(f64, f32)>,
    applied: Option<Telemetry>,
    board: Option<Gd<ConsoleBoard>>,
    strip: Option<Gd<ElevationStrip>>,
}

#[godot_api]
impl ICanvasLayer for Hud {
    fn init(base: Base<CanvasLayer>) -> Self {
        let (model, last, chip, scratch) = blank_model();
        Self {
            base,
            model,
            last,
            chip,
            scratch,
            status: String::new(),
            tv_label: "TV —".to_owned(),
            tv_address: String::new(),
            tv_visible: false,
            have_connection: false,
            dirty: true,
            profile_dirty: false,
            ahead_dirty: false,
            built: false,
            is_loop: true,
            length_m: 1.0,
            marker_m: 0.0,
            ahead_at: f64::MAX,
            spin: 0.0,
            hint_base: None,
            hint_hidden: false,
            show_lane: true,
            fish_text: String::new(),
            prompts: pp_view::prompts::ride_prompts(
                pp_view::prompts::InputMode::Keyboard,
                false,
                false,
            ),
            chrome: RideChrome::default(),
            card_s: 0.0,
            pad: PadBadge::Off,
            course_name: String::new(),
            points: Vec::new(),
            ahead: Vec::new(),
            ahead_samples: Vec::new(),
            applied: None,
            board: None,
            strip: None,
        }
    }

    fn ready(&mut self) {
        if self.built {
            return;
        }
        self.built = true;
        self.build();
    }

    fn exit_tree(&mut self) {
        ui_style::release();
    }
}

#[godot_api]
impl Hud {
    /// Power digits currently on screen.
    ///
    /// This is the instantaneous wattage. The hero number draws the 3-second
    /// average once [`Self::present`] has one, so the two can differ for a
    /// few seconds after a hard change. The smoke test needs `210` within
    /// 0.4 s of the fake trainer connecting, which a 3-second window cannot do.
    #[func]
    pub fn power_text(&mut self) -> GString {
        GString::from(self.model.power_instant.as_str())
    }

    /// Speed digits currently on screen.
    #[func]
    pub fn speed_text(&mut self) -> GString {
        GString::from(self.model.speed.as_str())
    }

    /// Cadence digits, or an em dash.
    #[func]
    pub fn cadence_text(&mut self) -> GString {
        GString::from(self.model.cadence.as_str())
    }

    /// Heart-rate digits, or an em dash.
    #[func]
    pub fn heart_text(&mut self) -> GString {
        GString::from(self.model.heart.as_str())
    }

    /// Grade digits currently on screen.
    ///
    /// Negative grades are drawn with U+2212. This getter maps that back to
    /// an ASCII hyphen so GDScript `float()` can parse it.
    #[func]
    pub fn grade_text(&mut self) -> GString {
        GString::from(self.model.grade_ascii.as_str())
    }

    /// Distance digits currently on screen.
    #[func]
    pub fn distance_text(&mut self) -> GString {
        GString::from(self.model.distance.as_str())
    }

    /// Lap digits currently on screen.
    #[func]
    pub fn lap_text(&mut self) -> GString {
        GString::from(self.model.last_value.as_str())
    }

    /// Elapsed time currently on screen.
    #[func]
    pub fn elapsed_text(&mut self) -> GString {
        GString::from(self.model.elapsed.as_str())
    }

    /// Elevation digits currently on screen.
    #[func]
    pub fn elevation_text(&mut self) -> GString {
        GString::from(self.model.elevation.as_str())
    }

    /// Trainer status line.
    #[func]
    pub fn status_text(&mut self) -> GString {
        GString::from(self.status.as_str())
    }

    /// TV dashboard slot.
    #[func]
    pub fn tv_text(&mut self) -> GString {
        GString::from(self.tv_label.as_str())
    }

    /// How many samples the elevation strip is holding.
    #[func]
    pub fn profile_point_count(&self) -> i32 {
        self.strip
            .as_ref()
            .map(|strip| strip.bind().point_count())
            .unwrap_or(self.points.len() as i32)
    }
}

impl Hud {
    /// Refresh every readout from one telemetry snapshot.
    ///
    /// This keeps the console current even when [`Self::present`] is not
    /// called. Power here is always a number: a missing trainer sample is
    /// `Some` only on the session readout.
    pub fn apply_telemetry(&mut self, telemetry: &Telemetry) {
        self.applied = Some(*telemetry);
        self.marker_m = telemetry.lap_distance_m;
        self.last.power_w = Some(telemetry.power_w);
        self.last.power_3s_w = None;
        self.last.cadence_rpm = telemetry.cadence_rpm;
        self.last.heart_rate_bpm = telemetry.heart_rate_bpm;
        self.last.speed_kph = telemetry.speed_kph();
        self.last.grade_pct = telemetry.grade_pct;
        self.last.distance_m = telemetry.distance_m;
        self.last.elevation_m = telemetry.elevation_m;
        self.last.elapsed_s = telemetry.elapsed_s;
        self.last.lap = telemetry.lap;
        self.last.distance_left_m = (self.length_m - telemetry.lap_distance_m).max(0.0);
        self.last.is_loop = self.is_loop;
        self.note_hint(Some(telemetry.power_w), telemetry.elapsed_s);
        self.recompute();
        if let Some(strip) = self.strip.as_mut() {
            strip
                .bind_mut()
                .set_marker(telemetry.lap_distance_m, telemetry.grade_pct);
        }
    }

    /// Pull zone, connection, and the road ahead from the session.
    #[allow(dead_code)] // `game.rs` calls this every frame once it is wired.
    ///
    /// Call this every frame after [`Self::apply_telemetry`]. A `None` sensor
    /// on the telemetry just applied stays a dash, so a forged missing
    /// cadence is not overwritten by a live sample. `dt` advances the
    /// reconnecting spinner.
    pub fn present(&mut self, session: &Session, dt: f64) {
        if self.card_s > 0.0 {
            self.card_s = (self.card_s - dt as f32).max(0.0);
            if self.card_s == 0.0 {
                self.chrome.card_title.clear();
                self.chrome.card_line.clear();
            }
            self.sync_board();
        }
        self.note_course(session);
        let readout = session.readout();
        let telemetry = session.telemetry();
        let cadence = merge_missing(
            self.applied.map(|frame| frame.cadence_rpm),
            readout.cadence_rpm,
        );
        let heart = merge_missing(
            self.applied.map(|frame| frame.heart_rate_bpm),
            readout.heart_rate_bpm,
        );
        self.applied = None;
        self.have_connection = true;
        let connection = session.connection();
        let _ = write_connection(&mut self.chip, &connection);
        self.last.power_w = readout.power_w;
        self.last.power_3s_w = readout.power_3s_w;
        self.last.cadence_rpm = cadence;
        self.last.heart_rate_bpm = heart;
        self.last.speed_kph = telemetry.speed_kph();
        self.last.grade_pct = telemetry.grade_pct;
        self.last.distance_m = telemetry.distance_m;
        self.last.elevation_m = telemetry.elevation_m;
        self.last.elapsed_s = telemetry.elapsed_s;
        self.last.lap = telemetry.lap;
        self.last.ftp_w = session.settings().ftp_w;
        self.last.distance_left_m = session.distance_left_m();
        self.last.is_loop = self.is_loop;
        self.marker_m = telemetry.lap_distance_m;
        if (self.marker_m - self.ahead_at).abs() >= 2.5 {
            session.fill_ahead(1000.0, 1000.0 / 48.0, &mut self.ahead);
            self.ahead_samples.clear();
            for point in &self.ahead {
                self.ahead_samples.push((point.ahead_m, point.elevation_m));
            }
            self.ahead_at = self.marker_m;
            self.ahead_dirty = true;
        }
        self.spin = (self.spin + dt as f32 * TAU) % TAU;
        self.note_hint(readout.power_w, telemetry.elapsed_s);
        self.recompute();
    }

    fn note_hint(&mut self, power: Option<f32>, elapsed_s: f64) {
        if self.chip.kind != ChipKind::Simulated {
            self.hint_hidden = false;
            self.hint_base = None;
            return;
        }
        let Some(power) = power else {
            return;
        };
        // The first second is the ride spinning up. Treat that power as the
        // baseline so the hint stays until the rider actually changes it.
        if elapsed_s < 1.0 {
            self.hint_base = Some(power);
            return;
        }
        match self.hint_base {
            None => self.hint_base = Some(power),
            Some(base) if (power - base).abs() >= 1.0 => self.hint_hidden = true,
            Some(_) => {}
        }
    }

    /// Replace the lap profile. `lap_length_m` is the distance at the right edge.
    pub fn set_profile(&mut self, points: &[(f64, f32)], lap_length_m: f64) {
        self.points.clear();
        self.points.extend_from_slice(points);
        self.length_m = lap_length_m.max(1.0);
        self.profile_dirty = true;
        self.ahead_at = f64::MAX;
        if let Some(strip) = self.strip.as_mut() {
            strip.bind_mut().set_profile(points, lap_length_m);
        }
        self.sync_board();
    }

    /// Trainer status line. Pass the whole sentence.
    pub fn set_status(&mut self, text: &str) {
        if self.status != text {
            self.status.clear();
            self.status.push_str(text);
        }
        if !self.have_connection && write_status_line(&mut self.chip, text) {
            self.recompute();
        }
    }

    /// Show the simulated-rider power hint again. Call this when the pause menu closes.
    #[allow(dead_code)]
    /// Controller chip. `Off` draws nothing. `--no-hud` hides the whole layer.
    pub fn set_pad_badge(&mut self, badge: PadBadge) {
        if self.pad != badge {
            self.pad = badge;
            self.sync_board();
        }
    }

    /// Segment panel, ghost chip, powerup slot, and the finish card.
    pub fn set_chrome(&mut self, chrome: RideChrome) {
        let card_title = self.chrome.card_title.clone();
        let card_line = self.chrome.card_line.clone();
        let mut next = chrome;
        if self.card_s > 0.0 {
            next.card_title = card_title;
            next.card_line = card_line;
        }
        if self.chrome != next {
            self.chrome = next;
            self.sync_board();
        }
    }

    /// Drop the finish card. A seek should not keep the previous result up.
    pub fn clear_finish(&mut self) {
        self.card_s = 0.0;
        self.chrome.card_title.clear();
        self.chrome.card_line.clear();
        self.sync_board();
    }

    /// Two-second result after a segment finish.
    pub fn show_finish(&mut self, title: &str, line: &str) {
        self.chrome.card_title = title.to_owned();
        self.chrome.card_line = line.to_owned();
        self.card_s = 2.0;
        self.sync_board();
    }

    /// Ride chips for the device used last.
    pub fn set_prompts(&mut self, prompts: pp_view::prompts::RidePrompts) {
        if self.prompts != prompts {
            self.prompts = prompts;
            self.sync_board();
        }
    }

    /// Fish counter. Empty hides it. `arcade` off passes an empty string.
    pub fn set_fish_text(&mut self, text: &str) {
        if self.fish_text != text {
            self.fish_text = text.to_owned();
            self.sync_board();
        }
    }

    /// Lane hint beside the power hint. Stays up until the first lane change.
    pub fn set_lane_hint(&mut self, show: bool) {
        if self.show_lane != show {
            self.show_lane = show;
            self.sync_board();
        }
    }

    /// Replace the trainer chip text for a screenshot. The next `present` overwrites it.
    pub fn debug_trainer_text(&mut self, name: &str, detail: &str) {
        self.chip.name.clear();
        self.chip.name.push_str(name);
        self.chip.detail.clear();
        self.chip.detail.push_str(detail);
        self.recompute();
    }

    pub fn restore_power_hint(&mut self) {
        self.hint_hidden = false;
        self.hint_base = None;
        self.sync_board();
    }

    /// LAN dashboard address. Empty clears the slot to an em dash and hides the chip.
    pub fn set_tv_url(&mut self, url: &str) {
        let shown = if url.is_empty() {
            "TV —".to_owned()
        } else {
            format!("TV {url}")
        };
        if self.tv_label != shown {
            self.tv_label = shown;
        }
        let visible = !url.is_empty();
        if self.tv_visible != visible || self.tv_address != url {
            self.tv_visible = visible;
            self.tv_address.clear();
            self.tv_address.push_str(url);
            self.dirty = true;
            self.sync_board();
        }
    }

    fn recompute(&mut self) {
        let input = self.last;
        if refresh(&mut self.model, &mut self.scratch, &input, &self.chip) {
            self.dirty = true;
        }
        self.sync_board();
    }

    fn note_course(&mut self, session: &Session) {
        let length = session.course().length_m();
        let changed =
            session.course().name() != self.course_name || (length - self.length_m).abs() > 0.5;
        if !changed && !self.points.is_empty() {
            return;
        }
        let facts = session.course_facts();
        self.is_loop = facts.is_loop;
        self.last.is_loop = facts.is_loop;
        self.course_name.clear();
        self.course_name.push_str(&facts.name);
        let points: Vec<(f64, f32)> = session
            .course()
            .profile(20.0)
            .into_iter()
            .map(|point| (point.distance_m, point.elevation_m))
            .collect();
        self.set_profile(&points, length);
    }

    fn sync_board(&mut self) {
        if self.board.is_none() {
            return;
        }
        let dirty = self.dirty;
        let model = dirty.then(|| self.model.clone());
        let tv_visible = self.tv_visible;
        let tv_address = dirty.then(|| self.tv_address.clone());
        let spin = self.spin;
        let profile = self
            .profile_dirty
            .then(|| (self.points.clone(), self.length_m));
        let ahead = self.ahead_dirty.then(|| self.ahead_samples.clone());
        let marker = self.marker_m;
        let show_hint = !self.hint_hidden && self.chip.kind == ChipKind::Simulated;
        let board = self.board.as_mut().expect("board");
        let mut board = board.bind_mut();
        let mut redraw = false;
        if let Some(model) = model {
            board.model = model;
            board.tv_visible = tv_visible;
            if let Some(address) = tv_address {
                board.tv_address = address;
            }
            board.spin = spin;
            redraw = true;
        }
        if let Some((points, length)) = profile {
            board.points = points;
            board.length_m = length;
            board.geo = board.geo.wrapping_add(1);
            redraw = true;
        }
        if let Some(ahead) = ahead {
            board.ahead = ahead;
            board.geo = board.geo.wrapping_add(1);
            redraw = true;
        }
        if !board.laid_out || (board.marker_m - marker).abs() >= 2.5 {
            board.marker_m = marker;
            redraw = true;
        }
        if board.show_hint != show_hint {
            board.show_hint = show_hint;
            redraw = true;
        }
        if board.show_lane != self.show_lane {
            board.show_lane = self.show_lane;
            redraw = true;
        }
        if board.fish_text != self.fish_text {
            board.fish_text.clone_from(&self.fish_text);
            redraw = true;
        }
        if board.prompts != self.prompts {
            board.prompts = self.prompts.clone();
            redraw = true;
        }
        if board.chrome != self.chrome {
            board.chrome = self.chrome.clone();
            redraw = true;
        }
        if board.pad != self.pad {
            board.pad = self.pad;
            redraw = true;
        }
        if board.spin != spin {
            board.spin = spin;
            if board.model.chip.kind.spins() {
                redraw = true;
            }
        }
        if redraw {
            board.base_mut().queue_redraw();
        }
        drop(board);
        self.dirty = false;
        self.profile_dirty = false;
        self.ahead_dirty = false;
    }

    fn build(&mut self) {
        let mut board = ConsoleBoard::new_alloc();
        board.set_name("Console");
        fill_control(&mut board);
        self.base_mut().add_child(&board);
        self.board = Some(board);

        let mut strip = ElevationStrip::new_alloc();
        strip.set_name("Elevation");
        strip.set_visible(false);
        strip.set_mouse_filter(MouseFilter::IGNORE);
        self.base_mut().add_child(&strip);
        self.strip = Some(strip);
        if !self.points.is_empty() {
            self.profile_dirty = true;
        }
        self.dirty = true;
        self.sync_board();
    }
}

fn blank_model() -> (ConsoleModel, ConsoleInput, Chip, String) {
    let input = ConsoleInput {
        power_w: None,
        power_3s_w: None,
        cadence_rpm: None,
        heart_rate_bpm: None,
        speed_kph: 0.0,
        grade_pct: 0.0,
        distance_m: 0.0,
        elevation_m: 0.0,
        elapsed_s: 0.0,
        lap: 0,
        ftp_w: DEFAULT_FTP_W,
        distance_left_m: 0.0,
        is_loop: true,
    };
    let mut chip = Chip::default();
    chip.name.push_str("Simulated rider");
    let mut model = ConsoleModel::default();
    let mut scratch = String::new();
    let _ = refresh(&mut model, &mut scratch, &input, &chip);
    (model, input, chip, scratch)
}

fn merge_missing<T: Copy>(applied: Option<Option<T>>, readout: Option<T>) -> Option<T> {
    match applied {
        Some(None) => None,
        Some(Some(_)) | None => readout,
    }
}

fn fill_control(control: &mut Gd<ConsoleBoard>) {
    control.set_anchor(Side::LEFT, 0.0);
    control.set_anchor(Side::TOP, 0.0);
    control.set_anchor(Side::RIGHT, 1.0);
    control.set_anchor(Side::BOTTOM, 1.0);
    control.set_offset(Side::LEFT, 0.0);
    control.set_offset(Side::TOP, 0.0);
    control.set_offset(Side::RIGHT, 0.0);
    control.set_offset(Side::BOTTOM, 0.0);
    control.set_anchors_preset(LayoutPreset::FULL_RECT);
    control.set_mouse_filter(MouseFilter::IGNORE);
}

/// Full-viewport console. The layer positions it; this node draws.
#[derive(GodotClass)]
#[class(base = Control)]
struct ConsoleBoard {
    base: Base<Control>,
    model: ConsoleModel,
    points: Vec<(f64, f32)>,
    ahead: Vec<(f64, f32)>,
    window: Vec<(f64, f32)>,
    skyline: Chart,
    next: Chart,
    length_m: f64,
    marker_m: f64,
    laid_marker: f64,
    laid_size: Vector2,
    geo: u64,
    laid_geo: u64,
    laid_out: bool,
    tv_visible: bool,
    show_hint: bool,
    show_lane: bool,
    fish_text: String,
    prompts: pp_view::prompts::RidePrompts,
    chrome: RideChrome,
    pad: PadBadge,
    tv_address: String,
    spin: f32,
    panel: Gd<StyleBoxFlat>,
    chip_style: Gd<StyleBoxFlat>,
    seg_style: Gd<StyleBoxFlat>,
}

#[godot_api]
impl IControl for ConsoleBoard {
    fn init(base: Base<Control>) -> Self {
        Self {
            base,
            model: ConsoleModel::default(),
            points: Vec::new(),
            ahead: Vec::new(),
            window: Vec::new(),
            skyline: Chart::default(),
            next: Chart::default(),
            length_m: 1.0,
            marker_m: 0.0,
            laid_marker: f64::MAX,
            laid_size: Vector2::ZERO,
            geo: 0,
            laid_geo: u64::MAX,
            laid_out: false,
            tv_visible: false,
            show_hint: true,
            show_lane: true,
            fish_text: String::new(),
            prompts: pp_view::prompts::ride_prompts(
                pp_view::prompts::InputMode::Keyboard,
                false,
                false,
            ),
            chrome: RideChrome::default(),
            pad: PadBadge::Off,
            tv_address: String::new(),
            spin: 0.0,
            panel: surface(Surface::Console),
            chip_style: surface(Surface::Chip),
            seg_style: surface(Surface::Well),
        }
    }

    fn ready(&mut self) {
        self.base_mut().queue_redraw();
    }

    fn draw(&mut self) {
        let size = self.base().get_size();
        if size.x < 16.0 || size.y < 16.0 {
            return;
        }
        let frame = design_frame(size);
        self.ensure_layout(frame, size);
        paint_skyline(self, frame);
        paint_panel(self, frame);
        paint_numbers(self, frame);
        paint_next(self, frame);
        paint_chips(self, frame);
        paint_chrome(self, frame);
    }
}

impl ConsoleBoard {
    fn ensure_layout(&mut self, frame: DesignFrame, size: Vector2) {
        let moved = !self.laid_out
            || self.laid_geo != self.geo
            || self.laid_size != size
            || (self.marker_m - self.laid_marker).abs() >= 2.5;
        if !moved {
            return;
        }
        self.laid_out = true;
        self.laid_geo = self.geo;
        self.laid_size = size;
        self.laid_marker = self.marker_m;
        let sky = Plot {
            left: frame.x(44.0),
            base_y: frame.y(644.0),
            width: frame.len(1192.0),
            height: frame.len(30.0),
            bin_px: frame.len(4.0).max(1.0),
        };
        fill_skyline(
            &self.points,
            self.length_m,
            self.marker_m,
            sky,
            &mut self.skyline,
        );
        if self.ahead.len() >= 2 {
            fill_next_km(&self.ahead, next_plot(frame), &mut self.next);
        } else {
            fill_window(
                &self.points,
                self.length_m,
                self.marker_m,
                1000.0,
                1000.0 / 48.0,
                &mut self.window,
            );
            fill_next_km(&self.window, next_plot(frame), &mut self.next);
        }
    }
}

fn next_plot(frame: DesignFrame) -> Plot {
    Plot {
        left: frame.x(860.0),
        base_y: frame.y(722.0),
        width: frame.len(190.0),
        height: frame.len(58.0),
        bin_px: frame.len(4.0).max(1.0),
    }
}

fn paint_skyline(board: &mut ConsoleBoard, frame: DesignFrame) {
    if board.skyline.top.len() < 2 {
        return;
    }
    let ink = ui_style::with_alpha(ui_style::INK, 0.88);
    let top = packed(&board.skyline.top);
    let width = frame.len(5.0).max(1.0);
    board
        .base_mut()
        .draw_polyline_ex(&top, ink)
        .width(width)
        .antialiased(true)
        .done();
    let mut fill = PackedVector2Array::new();
    push_unique(
        &mut fill,
        Vector2::new(board.skyline.left, board.skyline.base_y),
    );
    for point in &board.skyline.top {
        push_unique(&mut fill, Vector2::new(point.x, point.y));
    }
    push_unique(
        &mut fill,
        Vector2::new(board.skyline.right, board.skyline.base_y),
    );
    if fill.len() >= 3 {
        board.base_mut().draw_colored_polygon(&fill, ink);
    }
    let ridden = packed(&board.skyline.ridden);
    if ridden.len() >= 3 {
        board.base_mut().draw_colored_polygon(&ridden, RIDDEN);
    }
    let columns = board.skyline.columns.clone();
    let column_base = board.skyline.base_y;
    paint_columns(board, &columns, column_base, 1.0);
    let snow = ui_style::with_alpha(SNOW, 0.75);
    board
        .base_mut()
        .draw_polyline_ex(&top, snow)
        .width(frame.len(1.25).max(1.0))
        .antialiased(true)
        .done();
    if board.skyline.near.len() >= 2 {
        let near = packed(&board.skyline.near);
        board
            .base_mut()
            .draw_polyline_ex(&near, SNOW)
            .width(frame.len(3.0).max(1.0))
            .antialiased(true)
            .done();
    }
    let marker = Vector2::new(board.skyline.marker.x, board.skyline.marker.y);
    let foot = Vector2::new(board.skyline.marker.x, board.skyline.base_y);
    board
        .base_mut()
        .draw_line_ex(foot, marker, ink)
        .width(frame.len(5.0).max(1.0))
        .antialiased(true)
        .done();
    board
        .base_mut()
        .draw_line_ex(foot, marker, SNOW)
        .width(frame.len(2.0).max(1.0))
        .antialiased(true)
        .done();
    let radius = frame.len(6.0);
    board.base_mut().draw_circle(marker, radius, SNOW);
    board
        .base_mut()
        .draw_arc_ex(marker, radius, 0.0, TAU, 24, ui_style::INK)
        .width(frame.len(2.5).max(1.0))
        .antialiased(true)
        .done();
}

fn paint_columns(board: &mut ConsoleBoard, columns: &[Column], base_y: f32, alpha: f32) {
    for column in columns {
        if (column.x1 - column.x0).abs() < 0.25 {
            continue;
        }
        // A quad lying on the baseline has no area and Godot refuses to triangulate it.
        if (base_y - column.y0).abs() < 0.25 && (base_y - column.y1).abs() < 0.25 {
            continue;
        }
        let mut poly = PackedVector2Array::new();
        push_unique(&mut poly, Vector2::new(column.x0, base_y));
        push_unique(&mut poly, Vector2::new(column.x0, column.y0));
        push_unique(&mut poly, Vector2::new(column.x1, column.y1));
        push_unique(&mut poly, Vector2::new(column.x1, base_y));
        if poly.len() < 3 {
            continue;
        }
        let color = ui_style::with_alpha(ui_style::RAMP[usize::from(column.ramp.min(6))], alpha);
        board.base_mut().draw_colored_polygon(&poly, color);
    }
}

fn paint_panel(board: &mut ConsoleBoard, frame: DesignFrame) {
    let canvas = board.base().get_canvas_item();
    let rect = Rect2::new(
        Vector2::new(frame.x(32.0), frame.y(644.0)),
        Vector2::new(frame.len(1216.0), frame.len(124.0)),
    );
    board.panel.set_corner_radius_all(frame.px(12.0));
    board.panel.draw(canvas, rect);
    let color = ui_style::with_alpha(SNOW, 0.14);
    let top = frame.y(660.0);
    let bottom = frame.y(752.0);
    let width = frame.len(1.0).max(1.0);
    for x in [312.5_f32, 556.5, 1063.5] {
        let x = frame.x(x);
        board
            .base_mut()
            .draw_line_ex(Vector2::new(x, top), Vector2::new(x, bottom), color)
            .width(width)
            .done();
    }
}

fn paint_numbers(board: &mut ConsoleBoard, frame: DesignFrame) {
    paint_ladder(board, frame);
    paint_hero(board, frame);
    paint_body(board, frame);
    paint_list(board, frame);
}

fn paint_hero(board: &mut ConsoleBoard, frame: DesignFrame) {
    let baseline = frame.y(720.0);
    let label_y = frame.y(748.0);
    let hero = board.model.power_hero.clone();
    let zone = board.model.zone_label.clone();
    let missing = hero == "—";
    let hero_color = if missing { DUSK } else { SNOW };
    let zone_color = if missing {
        WARN
    } else if board.model.zone_index == 0 {
        MIST
    } else {
        ui_style::zone_color(board.model.zone_index)
    };
    let hero_font = ui_style::font(Weight::Bold);
    let hero_size = frame.px(88.0);
    let x = frame.x(88.0);
    draw_text(
        board,
        &hero_font,
        Vector2::new(x, baseline),
        &hero,
        hero_size,
        hero_color,
    );
    if !missing {
        let advance = measure(&hero_font, &hero, hero_size);
        let unit_font = ui_style::font_text(Weight::Medium);
        draw_text(
            board,
            &unit_font,
            Vector2::new(x + advance + frame.len(8.0), baseline),
            "W",
            frame.px(24.0),
            MIST,
        );
    }
    let caption = ui_style::font_caption();
    draw_text(
        board,
        &caption,
        Vector2::new(x, label_y),
        &zone,
        frame.px(18.0),
        zone_color,
    );
}

fn paint_body(board: &mut ConsoleBoard, frame: DesignFrame) {
    let number = ui_style::font(Weight::Bold);
    let unit = ui_style::font_text(Weight::Medium);
    let cadence = board.model.cadence.clone();
    let heart = board.model.heart.clone();
    let speed = board.model.speed.clone();
    let grade = board.model.grade.clone();
    let fonts = (&number, &unit);
    cell(
        board,
        frame,
        fonts,
        Cell {
            x: 336.0,
            value: &cadence,
            label: "rpm",
            missing: cadence == "—",
            color: SNOW,
        },
    );
    cell(
        board,
        frame,
        fonts,
        Cell {
            x: 452.0,
            value: &heart,
            label: "bpm",
            missing: heart == "—",
            color: SNOW,
        },
    );
    cell(
        board,
        frame,
        fonts,
        Cell {
            x: 580.0,
            value: &speed,
            label: "km/h",
            missing: false,
            color: SNOW,
        },
    );
    let grade_color = ui_style::grade_color(grade_value(&board.model.grade_ascii));
    cell(
        board,
        frame,
        fonts,
        Cell {
            x: 712.0,
            value: &grade,
            label: "% grade",
            missing: false,
            color: grade_color,
        },
    );
}

fn paint_list(board: &mut ConsoleBoard, frame: DesignFrame) {
    let caption = ui_style::font_caption();
    let unit = ui_style::font_text(Weight::Medium);
    let value_font = ui_style::font(Weight::SemiBold);
    let rows = [
        ("TIME", board.model.elapsed.clone(), String::new()),
        ("DIST", board.model.distance.clone(), "km".to_owned()),
        ("ELEV", board.model.elevation.clone(), "m".to_owned()),
        (
            box_caption(&board.model.last_caption),
            board.model.last_value.clone(),
            board.model.last_unit.clone(),
        ),
    ];
    let ys = [673.0_f32, 698.0, 723.0, 748.0];
    for (index, (caption_text, value, value_unit)) in rows.iter().enumerate() {
        let y = frame.y(ys[index]);
        draw_text(
            board,
            &caption,
            Vector2::new(frame.x(1078.0), y),
            caption_text,
            frame.px(18.0),
            MIST,
        );
        draw_value_right(
            board,
            frame,
            frame.x(1228.0),
            y,
            (&value_font, &unit),
            value,
            value_unit,
        );
    }
}

fn box_caption(caption: &str) -> &'static str {
    match caption {
        "LEFT" => "LEFT",
        _ => "LAP",
    }
}

struct Cell<'a> {
    x: f32,
    value: &'a str,
    label: &'a str,
    missing: bool,
    color: Color,
}

fn cell(
    board: &mut ConsoleBoard,
    frame: DesignFrame,
    fonts: (&Gd<godot::classes::Font>, &Gd<godot::classes::Font>),
    cell: Cell<'_>,
) {
    let (number, unit) = fonts;
    let Cell {
        x,
        value,
        label,
        missing,
        color,
    } = cell;
    let value_color = if missing { DUSK } else { color };
    let label_color = if missing { DUSK } else { MIST };
    draw_text(
        board,
        number,
        Vector2::new(frame.x(x), frame.y(720.0)),
        value,
        frame.px(52.0),
        value_color,
    );
    draw_text(
        board,
        unit,
        Vector2::new(frame.x(x), frame.y(748.0)),
        label,
        frame.px(18.0),
        label_color,
    );
}

fn paint_ladder(board: &mut ConsoleBoard, frame: DesignFrame) {
    let canvas = board.base().get_canvas_item();
    let zone = board.model.zone_index;
    let radius = frame.px(2.0).max(1);
    for index in 0..7 {
        let y = frame.y(658.0) + (6 - index) as f32 * frame.len(14.0);
        let active = zone == index as u8 + 1;
        let width = frame.len(if active { 22.0 } else { 16.0 });
        let color = if active {
            ui_style::RAMP[index]
        } else {
            ui_style::with_alpha(ui_style::RAMP[index], 0.34)
        };
        let rect = Rect2::new(
            Vector2::new(frame.x(52.0), y),
            Vector2::new(width, frame.len(12.0)),
        );
        board.seg_style.set_bg_color(color);
        board.seg_style.set_corner_radius_all(radius);
        board.seg_style.set_border_width_all(0);
        board.seg_style.draw(canvas, rect);
    }
}

fn paint_next(board: &mut ConsoleBoard, frame: DesignFrame) {
    if board.next.columns.is_empty() {
        return;
    }
    let base_y = board.next.base_y;
    let columns = board.next.columns.clone();
    paint_columns(board, &columns, base_y, 0.92);
    if board.next.top.len() >= 2 {
        let top = packed(&board.next.top);
        board
            .base_mut()
            .draw_polyline_ex(&top, SNOW)
            .width(frame.len(2.0).max(1.0))
            .antialiased(true)
            .done();
    }
    let tick = ui_style::with_alpha(ui_style::INK, 0.55);
    for step in 1..4 {
        let x = frame.x(860.0) + frame.len(190.0) * step as f32 / 4.0;
        board
            .base_mut()
            .draw_line_ex(
                Vector2::new(x, frame.y(664.0)),
                Vector2::new(x, base_y),
                tick,
            )
            .width(frame.len(1.5).max(1.0))
            .done();
    }
    let left = board.next.left;
    let right = board.next.right;
    board
        .base_mut()
        .draw_line_ex(
            Vector2::new(left, base_y),
            Vector2::new(right, base_y),
            ui_style::with_alpha(SNOW, 0.3),
        )
        .width(frame.len(1.0).max(1.0))
        .done();
    let dot = Vector2::new(board.next.marker.x + frame.len(1.0), board.next.marker.y);
    let radius = frame.len(5.0);
    board.base_mut().draw_circle(dot, radius, SNOW);
    board
        .base_mut()
        .draw_arc_ex(dot, radius, 0.0, TAU, 20, ui_style::INK)
        .width(frame.len(2.0).max(1.0))
        .antialiased(true)
        .done();
    let font = ui_style::font_text(Weight::Medium);
    draw_text(
        board,
        &font,
        Vector2::new(frame.x(860.0), frame.y(748.0)),
        "next 1 km",
        frame.px(18.0),
        MIST,
    );
}

fn paint_chips(board: &mut ConsoleBoard, frame: DesignFrame) {
    let kind = board.model.chip.kind;
    let name = board.model.chip.name.clone();
    let detail = board.model.chip.detail.clone();
    let y = frame.y(32.0);
    let trainer_w = paint_trainer(board, frame, frame.x(32.0), y, kind, &name, &detail);
    let menu_w = paint_menu(
        board,
        frame,
        frame.x(1248.0),
        y,
        kind == ChipKind::Simulated,
    );
    if board.tv_visible && !board.tv_address.is_empty() {
        let address = board.tv_address.clone();
        paint_tv(
            board,
            frame,
            frame.x(1248.0) - menu_w - frame.len(8.0),
            y,
            &address,
        );
    }
    let gap = frame.len(ui_style::space::GAP as f32);
    if board.pad != PadBadge::Off {
        let pad_w = pad_width(frame, board.pad);
        let menu_left = frame.x(1248.0) - menu_w;
        let mut pad_x = frame.x(32.0) + trainer_w + gap;
        let mut pad_y = y;
        if pad_x + pad_w > menu_left - gap {
            pad_x = frame.x(32.0);
            pad_y = y + frame.len(36.0) + gap;
        }
        paint_pad(board, frame, pad_x, pad_y, board.pad);
    }
    let mut hint_x = frame.x(32.0);
    if board.show_hint {
        hint_x += paint_hint(board, frame, hint_x) + gap;
    }
    if board.show_lane {
        let lane_w = paint_lane_hint(board, frame, hint_x);
        hint_x += lane_w + gap;
    }
    if !board.prompts.powerup.is_empty() {
        let text = board.prompts.powerup.clone();
        hint_x += paint_prompt(board, frame, hint_x, frame.y(562.0), &text) + gap;
    }
    if !board.prompts.workout.is_empty() {
        let text = board.prompts.workout.clone();
        hint_x += paint_prompt(board, frame, hint_x, frame.y(562.0), &text) + gap;
    }
    if !board.fish_text.is_empty() {
        paint_fish_chip(board, frame, hint_x, &board.fish_text.clone());
    }
}

fn paint_trainer(
    board: &mut ConsoleBoard,
    frame: DesignFrame,
    x: f32,
    y: f32,
    kind: ChipKind,
    name: &str,
    detail: &str,
) -> f32 {
    let name_font = ui_style::font_text(Weight::SemiBold);
    let detail_font = ui_style::font_text(Weight::Medium);
    let name_size = frame.px(20.0);
    let detail_size = frame.px(18.0);
    let icon = frame.len(icon_design(kind));
    let mut width = frame.len(14.0) + icon + frame.len(8.0) + measure(&name_font, name, name_size);
    if !detail.is_empty() {
        width += frame.len(8.0) + measure(&detail_font, detail, detail_size);
    }
    width += frame.len(14.0);
    paint_pill(board, frame, x, y, width, frame.len(36.0));
    let mark = Vector2::new(x + frame.len(14.0), y + frame.len(18.0));
    paint_mark(board, frame, mark, kind);
    let text_x = x + frame.len(14.0) + icon + frame.len(8.0);
    let baseline = y + frame.len(18.0) + frame.len(20.0) * 0.35;
    draw_text(
        board,
        &name_font,
        Vector2::new(text_x, baseline),
        name,
        name_size,
        SNOW,
    );
    if !detail.is_empty() {
        let color = match kind {
            ChipKind::Reconnecting | ChipKind::Searching => WARN,
            ChipKind::Failed => BAD,
            _ => MIST,
        };
        let dx = measure(&name_font, name, name_size) + frame.len(8.0);
        draw_text(
            board,
            &detail_font,
            Vector2::new(text_x + dx, baseline),
            detail,
            detail_size,
            color,
        );
    }
    width
}

fn paint_menu(
    board: &mut ConsoleBoard,
    frame: DesignFrame,
    right: f32,
    y: f32,
    _keyboard: bool,
) -> f32 {
    let text = if board.prompts.menu.is_empty() {
        "[Esc] Menu".to_owned()
    } else {
        board.prompts.menu.clone()
    };
    paint_prompt(board, frame, right - prompt_width(frame, &text), y, &text)
}

fn paint_tv(board: &mut ConsoleBoard, frame: DesignFrame, right: f32, y: f32, address: &str) {
    let font = ui_style::font_text(Weight::Medium);
    let size = frame.px(18.0);
    let icon = frame.len(20.0);
    let width =
        frame.len(14.0) + icon + frame.len(8.0) + measure(&font, address, size) + frame.len(14.0);
    let x = right - width;
    paint_pill(board, frame, x, y, width, frame.len(36.0));
    let mid = Vector2::new(x + frame.len(14.0) + frame.len(10.0), y + frame.len(18.0));
    paint_tv_icon(board, frame, mid);
    draw_text(
        board,
        &font,
        Vector2::new(
            x + frame.len(14.0) + icon + frame.len(8.0),
            mid.y + frame.len(18.0) * 0.35,
        ),
        address,
        size,
        MIST,
    );
}

fn paint_hint(board: &mut ConsoleBoard, frame: DesignFrame, x: f32) -> f32 {
    let text = if board.prompts.power.is_empty() {
        "Power".to_owned()
    } else {
        board.prompts.power.clone()
    };
    paint_prompt(board, frame, x, frame.y(562.0), &text)
}

fn paint_lane_hint(board: &mut ConsoleBoard, frame: DesignFrame, x: f32) -> f32 {
    let text = if board.prompts.lane.is_empty() {
        "Lane".to_owned()
    } else {
        board.prompts.lane.clone()
    };
    paint_prompt(board, frame, x, frame.y(562.0), &text)
}

fn paint_prompt(board: &mut ConsoleBoard, frame: DesignFrame, x: f32, y: f32, text: &str) -> f32 {
    let font = ui_style::font_text(Weight::SemiBold);
    let size = frame.px(16.0);
    let width = prompt_width(frame, text);
    paint_pill(board, frame, x, y, width, frame.len(36.0));
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(14.0), y + frame.len(24.0)),
        text,
        size,
        SNOW,
    );
    width
}

fn prompt_width(frame: DesignFrame, text: &str) -> f32 {
    let font = ui_style::font_text(Weight::SemiBold);
    frame.len(28.0) + measure(&font, text, frame.px(16.0))
}

fn paint_fish_chip(board: &mut ConsoleBoard, frame: DesignFrame, x: f32, text: &str) {
    let font = ui_style::font_text(Weight::SemiBold);
    let word = measure(&font, text, frame.px(18.0));
    let width = frame.len(14.0) + frame.len(22.0) + frame.len(8.0) + word + frame.len(14.0);
    let y = frame.y(562.0);
    paint_pill(board, frame, x, y, width, frame.len(40.0));
    let mid = y + frame.len(20.0);
    paint_fish_mark(board, frame, Vector2::new(x + frame.len(22.0), mid));
    draw_text(
        board,
        &font,
        Vector2::new(
            x + frame.len(14.0) + frame.len(22.0) + frame.len(8.0),
            mid + frame.len(18.0) * 0.35,
        ),
        text,
        frame.px(18.0),
        ui_style::SUN,
    );
}

fn paint_fish_mark(board: &mut ConsoleBoard, frame: DesignFrame, center: Vector2) {
    let mut body = PackedVector2Array::new();
    body.push(center + Vector2::new(-frame.len(8.0), 0.0));
    body.push(center + Vector2::new(frame.len(2.0), -frame.len(4.0)));
    body.push(center + Vector2::new(frame.len(6.0), 0.0));
    body.push(center + Vector2::new(frame.len(2.0), frame.len(4.0)));
    board.base_mut().draw_colored_polygon(&body, ui_style::SUN);
    board.base_mut().draw_line(
        center + Vector2::new(frame.len(6.0), 0.0),
        center + Vector2::new(frame.len(10.0), -frame.len(4.0)),
        ui_style::SUN,
    );
    board.base_mut().draw_line(
        center + Vector2::new(frame.len(6.0), 0.0),
        center + Vector2::new(frame.len(10.0), frame.len(4.0)),
        ui_style::SUN,
    );
}

fn pad_width(frame: DesignFrame, badge: PadBadge) -> f32 {
    let font = ui_style::font_text(Weight::SemiBold);
    let label = pad_label(badge);
    frame.len(14.0)
        + frame.len(22.0)
        + frame.len(8.0)
        + measure(&font, label, frame.px(18.0))
        + frame.len(14.0)
}

fn pad_label(badge: PadBadge) -> &'static str {
    match badge {
        PadBadge::Off => "",
        PadBadge::Scanning => "…",
        PadBadge::Count(1) => "1",
        PadBadge::Count(_) => "2",
        PadBadge::Unavailable => "!",
    }
}

fn paint_pad(board: &mut ConsoleBoard, frame: DesignFrame, x: f32, y: f32, badge: PadBadge) {
    let font = ui_style::font_text(Weight::SemiBold);
    let label = pad_label(badge);
    let color = match badge {
        PadBadge::Count(_) => ui_style::GOOD,
        PadBadge::Unavailable => ui_style::WARN,
        _ => ui_style::DUSK,
    };
    let width = pad_width(frame, badge);
    paint_pill(board, frame, x, y, width, frame.len(36.0));
    let mid = Vector2::new(x + frame.len(14.0) + frame.len(11.0), y + frame.len(18.0));
    paint_gamepad(board, frame, mid, color);
    draw_text(
        board,
        &font,
        Vector2::new(
            x + frame.len(14.0) + frame.len(22.0) + frame.len(8.0),
            mid.y + frame.len(18.0) * 0.35,
        ),
        label,
        frame.px(18.0),
        color,
    );
}

fn paint_gamepad(board: &mut ConsoleBoard, frame: DesignFrame, center: Vector2, color: Color) {
    let body = Rect2::new(
        center - Vector2::new(frame.len(9.0), frame.len(6.0)),
        Vector2::new(frame.len(18.0), frame.len(12.0)),
    );
    board
        .base_mut()
        .draw_rect_ex(body, color)
        .filled(false)
        .width(frame.len(1.5).max(1.0))
        .done();
    let arm = frame.len(3.0);
    board.base_mut().draw_line(
        center + Vector2::new(-frame.len(4.0), 0.0),
        center + Vector2::new(-frame.len(4.0) + arm, 0.0),
        color,
    );
    board.base_mut().draw_line(
        center + Vector2::new(-frame.len(2.5), -frame.len(1.5)),
        center + Vector2::new(-frame.len(2.5), frame.len(1.5)),
        color,
    );
    board.base_mut().draw_circle(
        center + Vector2::new(frame.len(3.5), -frame.len(1.5)),
        frame.len(1.2),
        color,
    );
    board.base_mut().draw_circle(
        center + Vector2::new(frame.len(5.5), frame.len(1.2)),
        frame.len(1.2),
        color,
    );
}

fn paint_pill(
    board: &mut ConsoleBoard,
    frame: DesignFrame,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) {
    let canvas = board.base().get_canvas_item();
    board
        .chip_style
        .set_corner_radius_all((height * 0.5).round().max(1.0) as i32);
    board.chip_style.set_border_width_all(frame.px(1.0).max(1));
    board.chip_style.draw(
        canvas,
        Rect2::new(Vector2::new(x, y), Vector2::new(width, height)),
    );
}

fn paint_mark(board: &mut ConsoleBoard, frame: DesignFrame, center: Vector2, kind: ChipKind) {
    let radius = frame.len(7.0);
    match kind {
        ChipKind::Controlled => board.base_mut().draw_circle(center, radius, GOOD),
        ChipKind::ReadingOnly => {
            board
                .base_mut()
                .draw_arc_ex(center, frame.len(5.5), 0.0, TAU, 24, GOOD)
                .width(frame.len(3.0).max(1.0))
                .antialiased(true)
                .done();
        }
        ChipKind::Reconnecting | ChipKind::Searching => {
            board
                .base_mut()
                .draw_arc_ex(
                    center,
                    radius,
                    0.0,
                    TAU,
                    24,
                    ui_style::with_alpha(WARN, 0.25),
                )
                .width(frame.len(3.0).max(1.0))
                .antialiased(true)
                .done();
            let sweep = 100.0_f32.to_radians();
            let spin = board.spin;
            board
                .base_mut()
                .draw_arc_ex(center, radius, spin, spin + sweep, 16, WARN)
                .width(frame.len(3.0).max(1.0))
                .antialiased(true)
                .done();
        }
        ChipKind::Failed => board.base_mut().draw_circle(center, radius, BAD),
        ChipKind::Simulated => paint_keyboard(board, frame, center),
    }
}

fn paint_keyboard(board: &mut ConsoleBoard, frame: DesignFrame, center: Vector2) {
    let rect = Rect2::new(
        center - Vector2::new(frame.len(11.0), frame.len(7.0)),
        Vector2::new(frame.len(22.0), frame.len(14.0)),
    );
    board
        .base_mut()
        .draw_rect_ex(rect, MIST)
        .filled(false)
        .width(frame.len(1.5).max(1.0))
        .done();
    for row in 0..2 {
        for col in 0..3 {
            let key = Rect2::new(
                center
                    + Vector2::new(
                        frame.len(-7.0) + col as f32 * frame.len(5.0),
                        frame.len(-4.0) + row as f32 * frame.len(4.0),
                    ),
                Vector2::new(frame.len(3.5), frame.len(2.5)),
            );
            board.base_mut().draw_rect(key, MIST);
        }
    }
}

fn paint_tv_icon(board: &mut ConsoleBoard, frame: DesignFrame, center: Vector2) {
    let rect = Rect2::new(
        center - Vector2::new(frame.len(9.0), frame.len(6.0)),
        Vector2::new(frame.len(18.0), frame.len(12.0)),
    );
    board
        .base_mut()
        .draw_rect_ex(rect, MIST)
        .filled(false)
        .width(frame.len(1.5).max(1.0))
        .done();
    board
        .base_mut()
        .draw_line_ex(
            center + Vector2::new(0.0, frame.len(6.0)),
            center + Vector2::new(0.0, frame.len(9.0)),
            MIST,
        )
        .width(frame.len(1.5).max(1.0))
        .done();
}

fn draw_value_right(
    board: &mut ConsoleBoard,
    frame: DesignFrame,
    right: f32,
    baseline: f32,
    fonts: (&Gd<godot::classes::Font>, &Gd<godot::classes::Font>),
    value: &str,
    unit: &str,
) {
    let (value_font, unit_font) = fonts;
    let value_size = frame.px(22.0);
    let unit_size = frame.px(18.0);
    let value_w = measure(value_font, value, value_size);
    let unit_w = if unit.is_empty() {
        0.0
    } else {
        measure(unit_font, unit, unit_size)
    };
    let gap = if unit.is_empty() { 0.0 } else { frame.len(4.0) };
    let x = right - value_w - gap - unit_w;
    draw_text(
        board,
        value_font,
        Vector2::new(x, baseline),
        value,
        value_size,
        SNOW,
    );
    if !unit.is_empty() {
        draw_text(
            board,
            unit_font,
            Vector2::new(x + value_w + gap, baseline),
            unit,
            unit_size,
            MIST,
        );
    }
}

fn draw_text(
    board: &mut ConsoleBoard,
    font: &Gd<godot::classes::Font>,
    pos: Vector2,
    text: &str,
    size: i32,
    color: Color,
) {
    board
        .base_mut()
        .draw_string_ex(font, pos, text)
        .font_size(size)
        .modulate(color)
        .done();
}

fn measure(font: &Gd<godot::classes::Font>, text: &str, size: i32) -> f32 {
    font.get_string_size_ex(text).font_size(size).done().x
}

fn packed(points: &[pp_view::elevation::Pt]) -> PackedVector2Array {
    let mut out = PackedVector2Array::new();
    for point in points {
        push_unique(&mut out, Vector2::new(point.x, point.y));
    }
    out
}

fn push_unique(poly: &mut PackedVector2Array, point: Vector2) {
    if let Some(prev) = poly.get(poly.len().saturating_sub(1))
        && prev.distance_to(point) < 0.35
    {
        return;
    }
    poly.push(point);
}

fn grade_value(ascii: &str) -> f32 {
    ascii.parse().unwrap_or(0.0)
}

fn icon_design(kind: ChipKind) -> f32 {
    match kind {
        ChipKind::Simulated => 22.0,
        ChipKind::Reconnecting | ChipKind::Searching => 16.0,
        _ => 14.0,
    }
}

/// Lap elevation profile. The console draws it; this node keeps the sample count.
#[derive(GodotClass)]
#[class(base = Control)]
pub struct ElevationStrip {
    base: Base<Control>,
    points: Vec<(f64, f32)>,
    lap_length_m: f64,
    marker_m: f64,
    grade_pct: f32,
}

#[godot_api]
impl IControl for ElevationStrip {
    fn init(base: Base<Control>) -> Self {
        Self {
            base,
            points: Vec::new(),
            lap_length_m: 1.0,
            marker_m: 0.0,
            grade_pct: 0.0,
        }
    }
}

#[godot_api]
impl ElevationStrip {
    /// Sample count. Zero until [`Self::set_profile`] runs.
    #[func]
    pub fn point_count(&self) -> i32 {
        let _kept = (self.lap_length_m, self.marker_m, self.grade_pct);
        self.points.len() as i32
    }
}

impl ElevationStrip {
    /// Store the lap profile. Distances are meters from the start line.
    pub fn set_profile(&mut self, points: &[(f64, f32)], lap_length_m: f64) {
        self.points.clear();
        self.points.extend_from_slice(points);
        self.lap_length_m = lap_length_m.max(1.0);
    }

    /// Move the rider marker. The console redraws its own skyline.
    pub fn set_marker(&mut self, lap_distance_m: f64, grade_pct: f32) {
        self.marker_m = lap_distance_m;
        self.grade_pct = grade_pct;
    }
}

/// Segment, ghost, and powerup readouts for one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct RideChrome {
    /// Empty hides the segment panel.
    pub segment_name: String,
    /// Running clock, already formatted.
    pub segment_time: String,
    /// Split text, already formatted.
    pub segment_split: String,
    /// True when the split is ahead of the best, or there is no best yet.
    pub segment_ahead: bool,
    /// 0 at the start line, 1 at the finish.
    pub segment_fraction: f32,
    /// Empty hides the ghost chip.
    pub ghost: String,
    /// Empty hides the powerup slot.
    pub power_name: String,
    /// 1 feather, 2 aero, 3 burst, 0 none.
    pub power_kind: u8,
    /// The effect is running, so the ring drains and the edges tint.
    pub power_active: bool,
    /// Remaining time over the full duration, 1 full and 0 empty.
    pub power_fill: f32,
    /// Empty hides the finish card.
    pub card_title: String,
    /// Second line of the finish card.
    pub card_line: String,
    /// Empty hides the draft chip.
    pub draft: String,
    /// Empty hides the workout card.
    pub workout_title: String,
    /// Target watts, or Open.
    pub workout_target: String,
    /// 0 hidden, 1 in range, 2 out of range, 3 not judged yet.
    pub workout_range: u8,
    /// Time left in the step.
    pub workout_clock: String,
    /// Next interval. Empty hides the line.
    pub workout_next: String,
    /// `+5%` or empty.
    pub workout_bias: String,
    /// The trainer is holding an ERG target.
    pub workout_erg: bool,
    /// Cue banner. Empty hides it.
    pub workout_cue: String,
    /// Mini profile, FTP fraction at the left of each bar.
    pub workout_bars: Vec<f32>,
    /// Cursor through the plan, 0 to 1.
    pub workout_cursor: f32,
}

impl Default for RideChrome {
    fn default() -> Self {
        Self {
            segment_name: String::new(),
            segment_time: String::new(),
            segment_split: String::new(),
            segment_ahead: true,
            segment_fraction: 0.0,
            ghost: String::new(),
            power_name: String::new(),
            power_kind: 0,
            power_active: false,
            power_fill: 1.0,
            card_title: String::new(),
            card_line: String::new(),
            draft: String::new(),
            workout_title: String::new(),
            workout_target: String::new(),
            workout_range: 0,
            workout_clock: String::new(),
            workout_next: String::new(),
            workout_bias: String::new(),
            workout_erg: false,
            workout_cue: String::new(),
            workout_bars: Vec::new(),
            workout_cursor: 0.0,
        }
    }
}

fn paint_chrome(board: &mut ConsoleBoard, frame: DesignFrame) {
    if board.chrome.power_active {
        paint_power_tint(board, frame, board.chrome.power_kind);
    }
    if !board.chrome.segment_name.is_empty() {
        let name = board.chrome.segment_name.clone();
        let time = board.chrome.segment_time.clone();
        let split = board.chrome.segment_split.clone();
        let ahead = board.chrome.segment_ahead;
        let fraction = board.chrome.segment_fraction;
        paint_segment(board, frame, &name, &time, &split, ahead, fraction);
    }
    if !board.chrome.ghost.is_empty() {
        let text = board.chrome.ghost.clone();
        paint_ghost_chip(board, frame, &text);
    }
    if !board.chrome.power_name.is_empty() {
        let name = board.chrome.power_name.clone();
        let kind = board.chrome.power_kind;
        let active = board.chrome.power_active;
        let fill = board.chrome.power_fill;
        paint_power_slot(board, frame, &name, kind, active, fill);
    }
    if !board.chrome.card_title.is_empty() {
        let title = board.chrome.card_title.clone();
        let line = board.chrome.card_line.clone();
        paint_finish_card(board, frame, &title, &line);
    }
    if !board.chrome.draft.is_empty() {
        let text = board.chrome.draft.clone();
        paint_draft(board, frame, &text);
    }
    if !board.chrome.workout_title.is_empty() {
        paint_workout(board, frame);
    }
    if !board.chrome.workout_cue.is_empty() {
        let text = board.chrome.workout_cue.clone();
        paint_cue(board, frame, &text);
    }
}

fn paint_segment(
    board: &mut ConsoleBoard,
    frame: DesignFrame,
    name: &str,
    time: &str,
    split: &str,
    ahead: bool,
    fraction: f32,
) {
    let font = ui_style::font_text(Weight::SemiBold);
    let width = frame.len(420.0);
    let height = frame.len(64.0);
    let x = (board.base().get_size().x - width) * 0.5;
    let y = frame.y(78.0);
    paint_pill(board, frame, x, y, width, height);
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(16.0), y + frame.len(26.0)),
        name,
        frame.px(18.0),
        SNOW,
    );
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(250.0), y + frame.len(26.0)),
        time,
        frame.px(18.0),
        SNOW,
    );
    let split_color = if ahead { GOOD } else { BAD };
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(330.0), y + frame.len(26.0)),
        split,
        frame.px(18.0),
        split_color,
    );
    let bar_x = x + frame.len(16.0);
    let bar_y = y + frame.len(40.0);
    let bar_w = width - frame.len(32.0);
    let bar_h = frame.len(8.0);
    board.base_mut().draw_rect(
        Rect2::new(Vector2::new(bar_x, bar_y), Vector2::new(bar_w, bar_h)),
        ui_style::with_alpha(SNOW, 0.25),
    );
    board.base_mut().draw_rect(
        Rect2::new(
            Vector2::new(bar_x, bar_y),
            Vector2::new(bar_w * fraction.clamp(0.0, 1.0), bar_h),
        ),
        ui_style::SUN,
    );
}

fn paint_ghost_chip(board: &mut ConsoleBoard, frame: DesignFrame, text: &str) {
    let font = ui_style::font_text(Weight::SemiBold);
    let word = measure(&font, text, frame.px(16.0));
    let width = word + frame.len(28.0);
    let x = (board.base().get_size().x - width) * 0.5;
    let y = frame.y(28.0);
    paint_pill(board, frame, x, y, width, frame.len(32.0));
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(14.0), y + frame.len(22.0)),
        text,
        frame.px(16.0),
        Color::from_rgb(0.7, 0.86, 1.0),
    );
}

fn paint_power_slot(
    board: &mut ConsoleBoard,
    frame: DesignFrame,
    name: &str,
    kind: u8,
    active: bool,
    fill: f32,
) {
    let font = ui_style::font_text(Weight::SemiBold);
    let x = frame.x(980.0);
    let y = frame.y(500.0);
    let width = frame.len(220.0);
    paint_pill(board, frame, x, y, width, frame.len(52.0));
    let center = Vector2::new(x + frame.len(28.0), y + frame.len(26.0));
    paint_power_icon(board, frame, center, kind);
    if active {
        board.base_mut().draw_arc(
            center,
            frame.len(16.0),
            -std::f32::consts::FRAC_PI_2,
            -std::f32::consts::FRAC_PI_2 + TAU * fill.clamp(0.0, 1.0),
            24,
            ui_style::SUN,
        );
    }
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(52.0), y + frame.len(32.0)),
        name,
        frame.px(18.0),
        SNOW,
    );
}

fn paint_power_icon(board: &mut ConsoleBoard, frame: DesignFrame, center: Vector2, kind: u8) {
    let color = ui_style::SUN;
    let s = frame.len(10.0);
    match kind {
        1 => {
            board.base_mut().draw_line(
                center + Vector2::new(-s, s),
                center + Vector2::new(s * 0.2, -s),
                color,
            );
            board.base_mut().draw_line(
                center + Vector2::new(-s * 0.2, s * 0.2),
                center + Vector2::new(s * 0.8, -s * 0.4),
                color,
            );
        }
        2 => {
            let mut wing = PackedVector2Array::new();
            wing.push(center + Vector2::new(-s, 0.0));
            wing.push(center + Vector2::new(s, -s * 0.45));
            wing.push(center + Vector2::new(s * 0.2, 0.0));
            wing.push(center + Vector2::new(s, s * 0.45));
            board.base_mut().draw_colored_polygon(&wing, color);
        }
        _ => {
            let mut bolt = PackedVector2Array::new();
            bolt.push(center + Vector2::new(-s * 0.2, -s));
            bolt.push(center + Vector2::new(s * 0.55, -s));
            bolt.push(center + Vector2::new(-s * 0.1, s * 0.05));
            bolt.push(center + Vector2::new(s * 0.35, s * 0.05));
            bolt.push(center + Vector2::new(-s * 0.45, s));
            bolt.push(center + Vector2::new(-s * 0.05, -s * 0.05));
            bolt.push(center + Vector2::new(-s * 0.55, -s * 0.05));
            board.base_mut().draw_colored_polygon(&bolt, color);
        }
    }
}

fn paint_finish_card(board: &mut ConsoleBoard, frame: DesignFrame, title: &str, line: &str) {
    let font = ui_style::font_text(Weight::SemiBold);
    let width = frame.len(460.0);
    let height = frame.len(88.0);
    let x = (board.base().get_size().x - width) * 0.5;
    let y = frame.y(180.0);
    paint_pill(board, frame, x, y, width, height);
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(20.0), y + frame.len(36.0)),
        title,
        frame.px(22.0),
        SNOW,
    );
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(20.0), y + frame.len(66.0)),
        line,
        frame.px(18.0),
        if line.starts_with('+') { BAD } else { GOOD },
    );
}

fn paint_draft(board: &mut ConsoleBoard, frame: DesignFrame, text: &str) {
    let font = ui_style::font_text(Weight::SemiBold);
    let word = measure(&font, text, frame.px(16.0));
    let width = word + frame.len(28.0);
    let x = frame.x(32.0);
    let y = frame.y(72.0);
    paint_pill(board, frame, x, y, width, frame.len(32.0));
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(14.0), y + frame.len(22.0)),
        text,
        frame.px(16.0),
        ui_style::GOOD,
    );
}

fn paint_workout(board: &mut ConsoleBoard, frame: DesignFrame) {
    let font = ui_style::font_text(Weight::SemiBold);
    let chrome = board.chrome.clone();
    let x = frame.x(24.0);
    let y = frame.y(78.0);
    let width = frame.len(360.0);
    let height = frame.len(118.0);
    paint_pill(board, frame, x, y, width, height);
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(14.0), y + frame.len(26.0)),
        &chrome.workout_title,
        frame.px(16.0),
        SNOW,
    );
    let range = match chrome.workout_range {
        1 => ui_style::GOOD,
        2 => ui_style::BAD,
        _ => ui_style::WARN,
    };
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(14.0), y + frame.len(50.0)),
        &chrome.workout_target,
        frame.px(22.0),
        range,
    );
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(140.0), y + frame.len(50.0)),
        &chrome.workout_clock,
        frame.px(18.0),
        SNOW,
    );
    if chrome.workout_erg {
        draw_text(
            board,
            &font,
            Vector2::new(x + frame.len(230.0), y + frame.len(50.0)),
            "ERG",
            frame.px(16.0),
            ui_style::SUN,
        );
    }
    if !chrome.workout_bias.is_empty() {
        draw_text(
            board,
            &font,
            Vector2::new(x + frame.len(280.0), y + frame.len(26.0)),
            &chrome.workout_bias,
            frame.px(16.0),
            ui_style::SUN,
        );
    }
    if !chrome.workout_next.is_empty() {
        draw_text(
            board,
            &font,
            Vector2::new(x + frame.len(14.0), y + frame.len(72.0)),
            &chrome.workout_next,
            frame.px(16.0),
            ui_style::MIST,
        );
    }
    paint_workout_bars(
        board,
        frame,
        x + frame.len(14.0),
        y + frame.len(88.0),
        width - frame.len(28.0),
        &chrome.workout_bars,
        chrome.workout_cursor,
    );
}

fn paint_workout_bars(
    board: &mut ConsoleBoard,
    frame: DesignFrame,
    x: f32,
    y: f32,
    width: f32,
    bars: &[f32],
    cursor: f32,
) {
    if bars.is_empty() || width <= 1.0 {
        return;
    }
    let slot = width / bars.len() as f32;
    let h = frame.len(16.0);
    for (index, ftp) in bars.iter().enumerate() {
        let color = if *ftp < 0.0 {
            ui_style::MIST
        } else if *ftp < 0.6 {
            Color::from_rgb(0.45, 0.7, 0.95)
        } else if *ftp < 0.76 {
            ui_style::GOOD
        } else if *ftp < 0.91 {
            ui_style::WARN
        } else if *ftp < 1.06 {
            ui_style::SUN
        } else {
            ui_style::BAD
        };
        let bar_h = (h * ftp.clamp(0.15, 1.4) / 1.4).max(frame.len(3.0));
        board.base_mut().draw_rect(
            Rect2::new(
                Vector2::new(x + slot * index as f32, y + h - bar_h),
                Vector2::new((slot - frame.len(2.0)).max(1.0), bar_h),
            ),
            color,
        );
    }
    let mark = x + width * cursor.clamp(0.0, 1.0);
    board.base_mut().draw_line(
        Vector2::new(mark, y - frame.len(2.0)),
        Vector2::new(mark, y + h + frame.len(2.0)),
        SNOW,
    );
}

fn paint_cue(board: &mut ConsoleBoard, frame: DesignFrame, text: &str) {
    let font = ui_style::font_text(Weight::SemiBold);
    let word = measure(&font, text, frame.px(20.0));
    let width = word + frame.len(36.0);
    let x = (board.base().get_size().x - width) * 0.5;
    let y = frame.y(220.0);
    paint_pill(board, frame, x, y, width, frame.len(44.0));
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(18.0), y + frame.len(28.0)),
        text,
        frame.px(20.0),
        SNOW,
    );
}

fn paint_power_tint(board: &mut ConsoleBoard, frame: DesignFrame, kind: u8) {
    let color = match kind {
        1 => ui_style::with_alpha(GOOD, 0.22),
        2 => ui_style::with_alpha(Color::from_rgb(0.45, 0.75, 1.0), 0.22),
        _ => ui_style::with_alpha(ui_style::SUN, 0.24),
    };
    let band = frame.len(28.0);
    let size = board.base().get_size();
    board
        .base_mut()
        .draw_rect(Rect2::new(Vector2::ZERO, Vector2::new(size.x, band)), color);
    board
        .base_mut()
        .draw_rect(Rect2::new(Vector2::ZERO, Vector2::new(band, size.y)), color);
    board.base_mut().draw_rect(
        Rect2::new(Vector2::new(0.0, size.y - band), Vector2::new(size.x, band)),
        color,
    );
    board.base_mut().draw_rect(
        Rect2::new(Vector2::new(size.x - band, 0.0), Vector2::new(band, size.y)),
        color,
    );
    let line = ui_style::with_alpha(SNOW, 0.35);
    for index in 0..5 {
        let y = frame.y(160.0 + index as f32 * 28.0);
        board.base_mut().draw_line(
            Vector2::new(frame.x(40.0), y),
            Vector2::new(frame.x(160.0), y),
            line,
        );
        board.base_mut().draw_line(
            Vector2::new(frame.x(1120.0), y),
            Vector2::new(frame.x(1240.0), y),
            line,
        );
    }
}
