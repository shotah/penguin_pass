//! World look loaded from `res://themes/alpine.json`.
//!
//! Parsing and the checks live in `pp-view`. This module only reads the file
//! and turns RGB triples into Godot colours.

use godot::classes::FileAccess;
use godot::classes::file_access::ModeFlags;
use godot::prelude::*;

pub use pp_view::theme::{ROAD_KEEP_OUT_M, ScatterLayer, Theme};

/// Read `res://themes/alpine.json`. On failure, log and use the built-in copy.
pub fn load_or_builtin() -> (Theme, bool) {
    let Some(file) = FileAccess::open("res://themes/alpine.json", ModeFlags::READ) else {
        godot_error!("Penguin Pass: themes/alpine.json missing, using built-in theme");
        return (Theme::builtin(), true);
    };
    let text = file.get_as_text().to_string();
    match Theme::parse(&text) {
        Ok(theme) => (theme, false),
        Err(error) => {
            godot_error!("Penguin Pass: theme rejected ({error}), using built-in theme");
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
