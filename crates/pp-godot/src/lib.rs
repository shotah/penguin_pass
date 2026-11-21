//! GDExtension entry point for Penguin Pass.
//!
//! The scene tree, meshes, and HUD are built in Rust. `main.tscn` is a single
//! [`game::Game`] node. [`PenguinPass`] stays registered so the headless smoke
//! test can prove the library loaded.

mod assets;
mod banners;
mod camera;
mod fake_trainer;
mod fish;
mod format;
mod game;
mod hud;
mod input_map;
mod launch;
mod menu;
mod mesh_util;
mod music;
mod rider;
mod road;
mod screens;
mod sim;
mod theme;
pub mod ui_style;
mod world;

use godot::classes::{INode, Node};
use godot::prelude::*;
use pp_core::RiderProfile;

struct PenguinPassExtension;

#[gdextension]
unsafe impl ExtensionLibrary for PenguinPassExtension {}

/// Load probe. The smoke test instantiates this and reads [`Self::version`].
#[derive(GodotClass)]
#[class(base = Node)]
struct PenguinPass {
    base: Base<Node>,
}

#[godot_api]
impl INode for PenguinPass {
    fn init(base: Base<Node>) -> Self {
        Self { base }
    }
}

#[godot_api]
impl PenguinPass {
    /// Crate version of this extension.
    #[func]
    fn version(&self) -> GString {
        // Keep pp-core linked even if a future build trims the ride types.
        let _total_mass_kg = RiderProfile::default().total_mass_kg();
        GString::from(env!("CARGO_PKG_VERSION"))
    }
}
