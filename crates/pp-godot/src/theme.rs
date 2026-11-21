//! World look loaded from `res://themes/<name>.json`.
//!
//! Parsing and the checks live in `pp-view`. This module only reads the file
//! and turns RGB triples into Godot colours.

use godot::classes::FileAccess;
use godot::classes::file_access::ModeFlags;
use godot::prelude::*;

pub use pp_view::theme::{ROAD_KEEP_OUT_M, ScatterLayer, Theme};

/// Environment variable that replaces the theme name for this process, for
/// looking at a theme before the course picker can choose it.
pub const THEME_OVERRIDE_ENV: &str = "PENGUIN_PASS_THEME";

/// Read `res://themes/alpine.json`. On failure, log and use the built-in copy.
pub fn load_or_builtin() -> (Theme, bool) {
    load_named("alpine")
}

/// Read `res://themes/<name>.json`. The flag is `true` when the built-in
/// alpine theme was used instead: the name is not a plain file stem, the
/// file is missing, or the theme was rejected.
///
/// `PENGUIN_PASS_THEME`, when set, replaces `name`.
pub fn load_named(name: &str) -> (Theme, bool) {
    let forced = godot::classes::Os::singleton()
        .get_environment(THEME_OVERRIDE_ENV)
        .to_string();
    let name = if forced.is_empty() { name } else { &forced };
    if !pp_view::theme::is_theme_name(name) {
        godot_error!("Penguin Pass: '{name}' is not a theme name, using built-in theme");
        return (Theme::builtin(), true);
    }
    let path = format!("res://themes/{name}.json");
    let Some(file) = FileAccess::open(&path, ModeFlags::READ) else {
        godot_error!("Penguin Pass: themes/{name}.json missing, using built-in theme");
        return (Theme::builtin(), true);
    };
    let text = file.get_as_text().to_string();
    match Theme::parse(&text) {
        Ok(theme) => (theme, false),
        Err(error) => {
            godot_error!("Penguin Pass: theme '{name}' rejected ({error}), using built-in theme");
            (Theme::builtin(), true)
        }
    }
}

/// Parse and validate theme JSON. Exposed for the smoke test.
pub fn parse(text: &str) -> Result<Theme, String> {
    Theme::parse(text)
}

/// RGB triple as a Godot colour.
pub fn rgb(color: [f32; 3]) -> Color {
    Color::from_rgb(color[0], color[1], color[2])
}
