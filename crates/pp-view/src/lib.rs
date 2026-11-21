//! Godot-free view logic for Penguin Pass.
//!
//! Number formatting, the settings menu, the HUD strings, theme checks,
//! scatter placement, and the distant backdrop live here so they can be
//! tested without the engine. `pp-godot` draws the results and owns every
//! `Gd` handle.

pub mod backdrop;
pub mod elevation;
pub mod format;
pub mod hold;
pub mod hud;
pub mod launch;
pub mod menu;
pub mod power;
pub mod scatter;
pub mod theme;
