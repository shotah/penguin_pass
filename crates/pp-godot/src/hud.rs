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
    Chip, ChipKind, ConsoleInput, ConsoleModel, power_hint, refresh, write_connection,
    write_status_line,
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
    tv_address: String,
    spin: f32,
    hint: String,
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
            tv_address: String::new(),
            spin: 0.0,
            hint: power_hint(),
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
    paint_trainer(board, frame, frame.x(32.0), y, kind, &name, &detail);
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
    if board.show_hint {
        paint_hint(board, frame);
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
) {
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
}

fn paint_menu(
    board: &mut ConsoleBoard,
    frame: DesignFrame,
    right: f32,
    y: f32,
    keyboard: bool,
) -> f32 {
    let font = ui_style::font_text(Weight::Medium);
    let size = frame.px(18.0);
    let icon = if keyboard {
        frame.len(36.0)
    } else {
        frame.len(22.0)
    };
    let width =
        frame.len(14.0) + icon + frame.len(8.0) + measure(&font, "Menu", size) + frame.len(14.0);
    let x = right - width;
    paint_pill(board, frame, x, y, width, frame.len(36.0));
    let mid = y + frame.len(18.0);
    if keyboard {
        paint_keycap(board, frame, Vector2::new(x + frame.len(14.0), mid), "Esc");
    } else {
        paint_menu_glyph(
            board,
            frame,
            Vector2::new(x + frame.len(14.0) + frame.len(11.0), mid),
        );
    }
    let baseline = mid + frame.len(18.0) * 0.35;
    draw_text(
        board,
        &font,
        Vector2::new(x + frame.len(14.0) + icon + frame.len(8.0), baseline),
        "Menu",
        size,
        MIST,
    );
    width
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

fn paint_hint(board: &mut ConsoleBoard, frame: DesignFrame) {
    let font = ui_style::font_text(Weight::SemiBold);
    let minor = ui_style::font_text(Weight::Medium);
    let hint = board.hint.clone();
    let power_w = measure(&font, "Power", frame.px(20.0));
    let step_w = measure(&minor, &hint, frame.px(18.0));
    let width = frame.len(14.0)
        + frame.len(56.0)
        + frame.len(8.0)
        + power_w
        + frame.len(8.0)
        + step_w
        + frame.len(14.0);
    let x = frame.x(32.0);
    let y = frame.y(562.0);
    paint_pill(board, frame, x, y, width, frame.len(40.0));
    let mid = y + frame.len(20.0);
    paint_arrow(board, frame, Vector2::new(x + frame.len(22.0), mid), true);
    paint_arrow(board, frame, Vector2::new(x + frame.len(48.0), mid), false);
    let baseline = mid + frame.len(20.0) * 0.35;
    let text_x = x + frame.len(14.0) + frame.len(56.0) + frame.len(8.0);
    draw_text(
        board,
        &font,
        Vector2::new(text_x, baseline),
        "Power",
        frame.px(20.0),
        SNOW,
    );
    draw_text(
        board,
        &minor,
        Vector2::new(text_x + power_w + frame.len(8.0), baseline),
        &hint,
        frame.px(18.0),
        MIST,
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

fn paint_keycap(board: &mut ConsoleBoard, frame: DesignFrame, center: Vector2, text: &str) {
    let width = frame.len(36.0);
    let height = frame.len(26.0);
    let rect = Rect2::new(
        center - Vector2::new(0.0, height * 0.5),
        Vector2::new(width, height),
    );
    board
        .base_mut()
        .draw_rect_ex(rect, MIST)
        .filled(false)
        .width(frame.len(1.5).max(1.0))
        .done();
    let font = ui_style::font_text(Weight::Medium);
    let size = frame.px(14.0);
    let advance = measure(&font, text, size);
    draw_text(
        board,
        &font,
        Vector2::new(
            rect.position.x + (width - advance) * 0.5,
            center.y + frame.len(14.0) * 0.35,
        ),
        text,
        size,
        MIST,
    );
}

fn paint_menu_glyph(board: &mut ConsoleBoard, frame: DesignFrame, center: Vector2) {
    let color = MIST;
    let width = frame.len(1.6).max(1.0);
    for step in [-4.0_f32, 0.0, 4.0] {
        let y = center.y + frame.len(step);
        board
            .base_mut()
            .draw_line_ex(
                Vector2::new(center.x - frame.len(7.0), y),
                Vector2::new(center.x + frame.len(7.0), y),
                color,
            )
            .width(width)
            .antialiased(true)
            .done();
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

fn paint_arrow(board: &mut ConsoleBoard, frame: DesignFrame, center: Vector2, up: bool) {
    let rect = Rect2::new(
        center - Vector2::new(frame.len(9.0), frame.len(9.0)),
        Vector2::new(frame.len(18.0), frame.len(18.0)),
    );
    board
        .base_mut()
        .draw_rect_ex(rect, SNOW)
        .filled(false)
        .width(frame.len(1.5).max(1.0))
        .done();
    let mut poly = PackedVector2Array::new();
    let tip = if up { -frame.len(4.0) } else { frame.len(4.0) };
    let base = if up { frame.len(3.0) } else { -frame.len(3.0) };
    poly.push(center + Vector2::new(0.0, tip));
    poly.push(center + Vector2::new(-frame.len(4.0), base));
    poly.push(center + Vector2::new(frame.len(4.0), base));
    board.base_mut().draw_colored_polygon(&poly, SNOW);
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
