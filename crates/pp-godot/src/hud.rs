//! Ride HUD. One theme, built in code, readable on a 7-inch deck and a TV.

use godot::builtin::Side;
use godot::classes::control::{LayoutPreset, MouseFilter, SizeFlags};
use godot::classes::{
    CanvasLayer, Control, HBoxContainer, ICanvasLayer, IControl, Label, MarginContainer,
    PanelContainer, Theme, ThemeDb, VBoxContainer,
};
use godot::global::HorizontalAlignment;
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;
use pp_core::Telemetry;

use crate::format::{
    format_cadence_rpm, format_distance_km, format_elapsed, format_elevation_m, format_grade_pct,
    format_heart_rate, format_lap, format_power_w, format_speed_kph, grade_rgb,
};

const VALUE_POWER: i32 = 36;
const VALUE_PRIMARY: i32 = 20;
const VALUE_SECONDARY: i32 = 16;
const CAPTION: i32 = 10;
const UNIT: i32 = 10;

/// Full-screen HUD layer.
#[derive(GodotClass)]
#[class(base = CanvasLayer)]
pub struct Hud {
    base: Base<CanvasLayer>,
    power: Option<Gd<Label>>,
    speed: Option<Gd<Label>>,
    cadence: Option<Gd<Label>>,
    heart: Option<Gd<Label>>,
    grade: Option<Gd<Label>>,
    distance: Option<Gd<Label>>,
    lap: Option<Gd<Label>>,
    elapsed: Option<Gd<Label>>,
    elevation: Option<Gd<Label>>,
    status: Option<Gd<Label>>,
    tv_url: Option<Gd<Label>>,
    strip: Option<Gd<ElevationStrip>>,
    built: bool,
}

#[godot_api]
impl ICanvasLayer for Hud {
    fn init(base: Base<CanvasLayer>) -> Self {
        Self {
            base,
            power: None,
            speed: None,
            cadence: None,
            heart: None,
            grade: None,
            distance: None,
            lap: None,
            elapsed: None,
            elevation: None,
            status: None,
            tv_url: None,
            strip: None,
            built: false,
        }
    }

    fn ready(&mut self) {
        if self.built {
            return;
        }
        self.built = true;
        self.build();
    }
}

#[godot_api]
impl Hud {
    /// Power digits currently on screen.
    #[func]
    pub fn power_text(&mut self) -> GString {
        label_text(&mut self.power)
    }

    /// Speed digits currently on screen.
    #[func]
    pub fn speed_text(&mut self) -> GString {
        label_text(&mut self.speed)
    }

    /// Cadence digits, or an em dash.
    #[func]
    pub fn cadence_text(&mut self) -> GString {
        label_text(&mut self.cadence)
    }

    /// Heart-rate digits, or an em dash.
    #[func]
    pub fn heart_text(&mut self) -> GString {
        label_text(&mut self.heart)
    }

    /// Grade digits currently on screen.
    #[func]
    pub fn grade_text(&mut self) -> GString {
        label_text(&mut self.grade)
    }

    /// Distance digits currently on screen.
    #[func]
    pub fn distance_text(&mut self) -> GString {
        label_text(&mut self.distance)
    }

    /// Lap digits currently on screen.
    #[func]
    pub fn lap_text(&mut self) -> GString {
        label_text(&mut self.lap)
    }

    /// Elapsed time currently on screen.
    #[func]
    pub fn elapsed_text(&mut self) -> GString {
        label_text(&mut self.elapsed)
    }

    /// Elevation digits currently on screen.
    #[func]
    pub fn elevation_text(&mut self) -> GString {
        label_text(&mut self.elevation)
    }

    /// Trainer status line.
    #[func]
    pub fn status_text(&mut self) -> GString {
        label_text(&mut self.status)
    }

    /// TV dashboard slot.
    #[func]
    pub fn tv_text(&mut self) -> GString {
        label_text(&mut self.tv_url)
    }

    /// How many samples the elevation strip is holding.
    #[func]
    pub fn profile_point_count(&self) -> i32 {
        self.strip
            .as_ref()
            .map(|strip| strip.bind().point_count())
            .unwrap_or(0)
    }
}

impl Hud {
    /// Refresh every readout from one telemetry snapshot.
    pub fn apply_telemetry(&mut self, telemetry: &Telemetry) {
        set_label(&mut self.power, &format_power_w(telemetry.power_w));
        set_label(&mut self.speed, &format_speed_kph(telemetry.speed_kph()));
        set_label(
            &mut self.cadence,
            &format_cadence_rpm(telemetry.cadence_rpm),
        );
        set_label(
            &mut self.heart,
            &format_heart_rate(telemetry.heart_rate_bpm),
        );
        let grade = format_grade_pct(telemetry.grade_pct);
        set_label(&mut self.grade, &grade);
        let (red, green, blue) = grade_rgb(telemetry.grade_pct);
        if let Some(label) = self.grade.as_mut() {
            label.add_theme_color_override("font_color", Color::from_rgb(red, green, blue));
        }
        set_label(
            &mut self.distance,
            &format_distance_km(telemetry.distance_m),
        );
        set_label(&mut self.lap, &format_lap(telemetry.lap));
        set_label(&mut self.elapsed, &format_elapsed(telemetry.elapsed_s));
        set_label(
            &mut self.elevation,
            &format_elevation_m(telemetry.elevation_m),
        );
        if let Some(strip) = self.strip.as_mut() {
            strip
                .bind_mut()
                .set_marker(telemetry.lap_distance_m, telemetry.grade_pct);
        }
    }

    /// Replace the lap profile. `lap_length_m` is the distance at the right edge.
    pub fn set_profile(&mut self, points: &[(f64, f32)], lap_length_m: f64) {
        if let Some(strip) = self.strip.as_mut() {
            strip.bind_mut().set_profile(points, lap_length_m);
        }
    }

    /// Trainer status line. Pass the whole sentence.
    pub fn set_status(&mut self, text: &str) {
        set_label(&mut self.status, text);
    }

    /// LAN dashboard address. Empty clears the slot to an em dash.
    pub fn set_tv_url(&mut self, url: &str) {
        let shown = if url.is_empty() {
            "TV —".to_string()
        } else {
            format!("TV {url}")
        };
        set_label(&mut self.tv_url, &shown);
    }

    fn build(&mut self) {
        let theme = ride_theme();
        let mut margin = MarginContainer::new_alloc();
        margin.set_name("Margin");
        fill_rect(&mut margin);
        margin.add_theme_constant_override("margin_left", 16);
        margin.add_theme_constant_override("margin_top", 10);
        margin.add_theme_constant_override("margin_right", 16);
        margin.add_theme_constant_override("margin_bottom", 10);
        margin.set_theme(&theme);
        margin.set_mouse_filter(MouseFilter::IGNORE);

        let mut column = VBoxContainer::new_alloc();
        column.set_name("Column");
        column.set_h_size_flags(SizeFlags::EXPAND_FILL);
        column.set_v_size_flags(SizeFlags::EXPAND_FILL);
        column.add_theme_constant_override("separation", 8);
        column.set_mouse_filter(MouseFilter::IGNORE);

        let mut top = panel("Top");
        let mut top_box = VBoxContainer::new_alloc();
        top_box.add_theme_constant_override("separation", 6);
        top_box.set_mouse_filter(MouseFilter::IGNORE);

        let mut primary = HBoxContainer::new_alloc();
        primary.set_name("Primary");
        primary.add_theme_constant_override("separation", 12);
        primary.set_alignment(godot::classes::box_container::AlignmentMode::CENTER);
        let speed = stat_column(&mut primary, "SPEED", "km/h", VALUE_PRIMARY, false);
        let power = stat_column(&mut primary, "POWER", "W", VALUE_POWER, true);
        let cadence = stat_column(&mut primary, "CADENCE", "rpm", VALUE_PRIMARY, false);
        let heart = stat_column(&mut primary, "HEART", "bpm", VALUE_PRIMARY, false);
        top_box.add_child(&primary);

        let mut secondary = HBoxContainer::new_alloc();
        secondary.set_name("Secondary");
        secondary.add_theme_constant_override("separation", 8);
        secondary.set_alignment(godot::classes::box_container::AlignmentMode::CENTER);
        let grade = stat_column(&mut secondary, "GRADE", "%", VALUE_SECONDARY, false);
        let distance = stat_column(&mut secondary, "DISTANCE", "km", VALUE_SECONDARY, false);
        let lap = stat_column(&mut secondary, "LAP", "", VALUE_SECONDARY, false);
        let elapsed = stat_column(&mut secondary, "TIME", "", VALUE_SECONDARY, false);
        let elevation = stat_column(&mut secondary, "ELEVATION", "m", VALUE_SECONDARY, false);
        top_box.add_child(&secondary);
        top.add_child(&top_box);
        column.add_child(&top);

        let mut spacer = godot::classes::Control::new_alloc();
        spacer.set_name("Spacer");
        spacer.set_v_size_flags(SizeFlags::EXPAND_FILL);
        spacer.set_mouse_filter(MouseFilter::IGNORE);
        column.add_child(&spacer);

        let mut bottom = panel("Bottom");
        let mut bottom_box = VBoxContainer::new_alloc();
        bottom_box.add_theme_constant_override("separation", 6);
        bottom_box.set_mouse_filter(MouseFilter::IGNORE);
        let mut strip = ElevationStrip::new_alloc();
        strip.set_name("Elevation");
        strip.set_custom_minimum_size(Vector2::new(0.0, 48.0));
        strip.set_h_size_flags(SizeFlags::EXPAND_FILL);
        strip.set_mouse_filter(MouseFilter::IGNORE);
        bottom_box.add_child(&strip);

        let mut status_row = HBoxContainer::new_alloc();
        status_row.set_name("StatusRow");
        status_row.add_theme_constant_override("separation", 16);
        let mut status = Label::new_alloc();
        status.set_name("Status");
        status.set_text(" ");
        status.set_h_size_flags(SizeFlags::EXPAND_FILL);
        status.add_theme_font_size_override("font_size", 13);
        status.add_theme_color_override("font_color", Color::from_rgb(0.82, 0.86, 0.90));
        let mut tv = Label::new_alloc();
        tv.set_name("TvUrl");
        tv.set_text("TV —");
        tv.set_horizontal_alignment(HorizontalAlignment::RIGHT);
        tv.add_theme_font_size_override("font_size", 13);
        tv.add_theme_color_override("font_color", Color::from_rgb(0.82, 0.86, 0.90));
        status_row.add_child(&status);
        status_row.add_child(&tv);
        bottom_box.add_child(&status_row);
        bottom.add_child(&bottom_box);
        column.add_child(&bottom);

        margin.add_child(&column);
        self.base_mut().add_child(&margin);

        self.power = Some(power);
        self.speed = Some(speed);
        self.cadence = Some(cadence);
        self.heart = Some(heart);
        self.grade = Some(grade);
        self.distance = Some(distance);
        self.lap = Some(lap);
        self.elapsed = Some(elapsed);
        self.elevation = Some(elevation);
        self.status = Some(status);
        self.tv_url = Some(tv);
        self.strip = Some(strip);
    }
}

/// Lap elevation profile with a marker at the rider.
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

    fn draw(&mut self) {
        let size = self.base().get_size();
        let points = self.points.clone();
        let lap = self.lap_length_m;
        let marker = self.marker_m;
        let grade = self.grade_pct;
        paint(self, size, &points, lap, marker, grade);
    }
}

#[godot_api]
impl ElevationStrip {
    /// Sample count. Zero until [`Self::set_profile`] runs.
    #[func]
    pub fn point_count(&self) -> i32 {
        self.points.len() as i32
    }
}

impl ElevationStrip {
    /// Store the lap profile. Distances are meters from the start line.
    pub fn set_profile(&mut self, points: &[(f64, f32)], lap_length_m: f64) {
        self.points = points.to_vec();
        self.lap_length_m = lap_length_m.max(1.0);
        self.base_mut().queue_redraw();
    }

    /// Move the rider marker and recolor the upcoming grade.
    pub fn set_marker(&mut self, lap_distance_m: f64, grade_pct: f32) {
        self.marker_m = lap_distance_m;
        self.grade_pct = grade_pct;
        self.base_mut().queue_redraw();
    }
}

fn paint(
    strip: &mut ElevationStrip,
    size: Vector2,
    points: &[(f64, f32)],
    lap: f64,
    marker: f64,
    grade: f32,
) {
    if size.x < 4.0 || size.y < 4.0 || points.len() < 2 || lap <= 0.0 {
        return;
    }
    let Some(layout) = pp_view::elevation::layout(size.x, size.y, points, lap, marker) else {
        return;
    };
    let mut poly = PackedVector2Array::new();
    let mut line = PackedVector2Array::new();
    for point in &layout.fill {
        poly.push(Vector2::new(point.x, point.y));
    }
    for point in &layout.line {
        line.push(Vector2::new(point.x, point.y));
    }
    strip
        .base_mut()
        .draw_colored_polygon(&poly, Color::from_rgba(0.45, 0.72, 0.55, 0.55));
    strip
        .base_mut()
        .draw_polyline_ex(&line, Color::from_rgb(0.92, 0.95, 0.96))
        .width(2.0)
        .antialiased(true)
        .done();

    let (red, green, blue) = grade_rgb(grade);
    let mut ahead = PackedVector2Array::new();
    for point in &layout.ahead {
        ahead.push(Vector2::new(point.x, point.y));
    }
    if ahead.len() >= 2 {
        strip
            .base_mut()
            .draw_polyline_ex(&ahead, Color::from_rgb(red, green, blue))
            .width(4.0)
            .antialiased(true)
            .done();
    }

    let marker_x = layout.marker_top.x;
    strip
        .base_mut()
        .draw_line_ex(
            Vector2::new(layout.marker_top.x, layout.marker_top.y),
            Vector2::new(layout.marker_bottom.x, layout.marker_bottom.y),
            Color::from_rgb(1.0, 0.86, 0.35),
        )
        .width(2.0)
        .antialiased(true)
        .done();
    let mut tip = PackedVector2Array::new();
    let tip_y = layout.marker_top.y;
    tip.push(Vector2::new(marker_x, tip_y));
    tip.push(Vector2::new(marker_x - 6.0, tip_y + 10.0));
    tip.push(Vector2::new(marker_x + 6.0, tip_y + 10.0));
    strip
        .base_mut()
        .draw_colored_polygon(&tip, Color::from_rgb(1.0, 0.86, 0.35));
}

fn ride_theme() -> Gd<Theme> {
    let mut theme = Theme::new_gd();
    let fallback = ThemeDb::singleton().get_fallback_font();
    if let Some(font) = fallback {
        theme.set_default_font(&font);
    }
    theme.set_default_font_size(20);
    let mut panel = godot::classes::StyleBoxFlat::new_gd();
    panel.set_bg_color(Color::from_rgba(0.04, 0.06, 0.09, 0.58));
    panel.set_corner_radius_all(8);
    panel.set_content_margin_all(6.0);
    panel.set_border_width_all(1);
    panel.set_border_color(Color::from_rgba(1.0, 1.0, 1.0, 0.10));
    theme.set_stylebox("panel", "PanelContainer", &panel);
    theme.set_color("font_color", "Label", Color::from_rgb(0.96, 0.97, 0.98));
    theme.set_font_size("font_size", "Label", 20);
    theme.set_constant("separation", "VBoxContainer", 4);
    theme.set_constant("separation", "HBoxContainer", 12);
    theme
}

fn panel(name: &str) -> Gd<PanelContainer> {
    let mut panel = PanelContainer::new_alloc();
    panel.set_name(name);
    panel.set_mouse_filter(MouseFilter::IGNORE);
    panel
}

fn stat_column(
    row: &mut Gd<HBoxContainer>,
    caption: &str,
    unit: &str,
    value_px: i32,
    emphasize: bool,
) -> Gd<Label> {
    let mut column = VBoxContainer::new_alloc();
    column.set_h_size_flags(SizeFlags::EXPAND_FILL);
    column.add_theme_constant_override("separation", 0);
    column.set_alignment(godot::classes::box_container::AlignmentMode::CENTER);
    column.set_mouse_filter(MouseFilter::IGNORE);

    let caption_color = Color::from_rgb(0.68, 0.75, 0.82);
    let cap = make_label(caption, CAPTION, caption_color);
    column.add_child(&cap);

    let value_color = if emphasize {
        Color::from_rgb(1.0, 0.93, 0.82)
    } else {
        Color::from_rgb(0.97, 0.98, 0.99)
    };
    let value = make_label("—", value_px, value_color);
    column.add_child(&value);

    if !unit.is_empty() {
        let unit_label = make_label(unit, UNIT, caption_color);
        column.add_child(&unit_label);
    }
    row.add_child(&column);
    value
}

fn make_label(text: &str, size: i32, color: Color) -> Gd<Label> {
    let mut label = Label::new_alloc();
    label.set_text(text);
    label.set_horizontal_alignment(HorizontalAlignment::CENTER);
    label.add_theme_font_size_override("font_size", size);
    label.add_theme_color_override("font_color", color);
    label.set_h_size_flags(SizeFlags::EXPAND_FILL);
    label.set_mouse_filter(MouseFilter::IGNORE);
    label
}

fn fill_rect(control: &mut Gd<MarginContainer>) {
    control.set_anchor(Side::LEFT, 0.0);
    control.set_anchor(Side::TOP, 0.0);
    control.set_anchor(Side::RIGHT, 1.0);
    control.set_anchor(Side::BOTTOM, 1.0);
    control.set_offset(Side::LEFT, 0.0);
    control.set_offset(Side::TOP, 0.0);
    control.set_offset(Side::RIGHT, 0.0);
    control.set_offset(Side::BOTTOM, 0.0);
    control.set_anchors_preset(LayoutPreset::FULL_RECT);
}

fn set_label(slot: &mut Option<Gd<Label>>, text: &str) {
    if let Some(label) = slot.as_mut() {
        label.set_text(text);
    }
}

fn label_text(slot: &mut Option<Gd<Label>>) -> GString {
    match slot.as_mut() {
        Some(label) => label.get_text(),
        None => GString::new(),
    }
}
