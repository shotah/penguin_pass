//! Chase camera and a first-person handlebar view.
//!
//! Smoothing is an exponential decay on `delta`, so the lag stays the same at
//! any frame rate and does not oscillate.

use godot::classes::{Camera3D, INode3D, Node3D};
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;

const CHASE_LAMBDA: f64 = 7.5;
const LOOK_LAMBDA: f64 = 9.0;

/// Camera that follows a rider transform. Not parented to the bike, so the
/// smoother is the only motion on top of the rider.
#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct CameraRig {
    base: Base<Node3D>,
    camera: Option<Gd<Camera3D>>,
    smoothed_pos: Vector3,
    smoothed_look: Vector3,
    initialized: bool,
    first_person: bool,
}

#[godot_api]
impl INode3D for CameraRig {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            camera: None,
            smoothed_pos: Vector3::ZERO,
            smoothed_look: Vector3::FORWARD,
            initialized: false,
            first_person: false,
        }
    }

    fn ready(&mut self) {
        if self.camera.is_some() {
            return;
        }
        let mut camera = Camera3D::new_alloc();
        camera.set_name("Camera");
        camera.set_current(true);
        camera.set_fov(58.0);
        camera.set_near(0.08);
        camera.set_far(8_000.0);
        self.base_mut().add_child(&camera);
        self.camera = Some(camera);
    }
}

impl CameraRig {
    /// Move toward the rig described by `rider_global`.
    ///
    /// `dt` of zero snaps. Switching camera mode snaps, so the lens does not
    /// travel through the rider.
    pub fn follow(&mut self, rider_global: Transform3D, dt: f64, first_person: bool) {
        if first_person != self.first_person {
            self.first_person = first_person;
            self.initialized = false;
        }
        let (desired_pos, desired_look, fov) = if first_person {
            // Just above the hoods, looking down the road, so the bars and
            // front wheel sit in the lower frame.
            (
                point(rider_global, Vector3::new(0.0, 1.22, -0.18)),
                point(rider_global, Vector3::new(0.0, 0.55, -6.0)),
                72.0,
            )
        } else {
            // Close and a little low. A high, distant camera turned the 6 m
            // road into a black wedge that filled the frame.
            (
                point(rider_global, Vector3::new(0.7, 1.48, 3.05)),
                point(rider_global, Vector3::new(0.0, 1.05, -8.0)),
                50.0,
            )
        };
        let snap = !self.initialized || dt <= 0.0;
        if snap {
            self.smoothed_pos = desired_pos;
            self.smoothed_look = desired_look;
            self.initialized = true;
        } else {
            let pos_blend = 1.0 - (-dt * CHASE_LAMBDA).exp();
            let look_blend = 1.0 - (-dt * LOOK_LAMBDA).exp();
            self.smoothed_pos = self.smoothed_pos.lerp(desired_pos, pos_blend as f32);
            self.smoothed_look = self.smoothed_look.lerp(desired_look, look_blend as f32);
        }
        let pos = self.smoothed_pos;
        let look = self.smoothed_look;
        if let Some(camera) = self.camera.as_mut() {
            camera.set_fov(fov);
            camera.look_at_from_position(pos, look);
        }
    }
}

fn point(transform: Transform3D, local: Vector3) -> Vector3 {
    transform.origin + transform.basis * local
}
