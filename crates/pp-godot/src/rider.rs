//! Low-poly rider and bike. Primitives only: two wheels, a frame, a body.

use godot::classes::{INode3D, Node3D};
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;

use crate::mesh_util::{add_box, add_cylinder, add_sphere, add_tube, standard_material};

const WHEEL_RADIUS: f32 = 0.34;

/// Cyclist stand-in. Origin sits on the road, facing the parent's -Z once aimed.
#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Rider {
    base: Base<Node3D>,
    wheels: Vec<Gd<Node3D>>,
    built: bool,
}

#[godot_api]
impl INode3D for Rider {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            wheels: Vec::new(),
            built: false,
        }
    }

    fn ready(&mut self) {
        if self.built {
            return;
        }
        self.built = true;
        self.assemble();
    }
}

#[godot_api]
impl Rider {
    /// World position of the rider origin. Headless tests compare this to the course.
    #[func]
    pub fn rider_origin(&self) -> Vector3 {
        self.base().get_global_position()
    }
}

impl Rider {
    /// Place the bike on `position`, nose along `tangent`, and spin the wheels from `distance_m`.
    pub fn place(&mut self, position: Vector3, tangent: Vector3, distance_m: f64) {
        let forward = unit_or(tangent, Vector3::FORWARD);
        let up = if forward.dot(Vector3::UP).abs() > 0.95 {
            Vector3::RIGHT
        } else {
            Vector3::UP
        };
        // The mesh faces -Z. `look_at` aims -Z at the target, so the target is one step along travel.
        self.base_mut()
            .look_at_from_position_ex(position, position + forward)
            .up(up)
            .done();
        let spin = (distance_m / f64::from(WHEEL_RADIUS)) as f32;
        for wheel in &mut self.wheels {
            wheel.set_rotation(Vector3::new(spin, 0.0, 0.0));
        }
    }

    fn assemble(&mut self) {
        let frame = standard_material(Color::from_rgb(0.07, 0.08, 0.10), 0.32, 0.75);
        let tire = standard_material(Color::from_rgb(0.04, 0.04, 0.045), 0.85, 0.0);
        let rim = standard_material(Color::from_rgb(0.62, 0.64, 0.66), 0.28, 0.85);
        let jersey = standard_material(Color::from_rgb(0.90, 0.34, 0.16), 0.7, 0.0);
        let dark = standard_material(Color::from_rgb(0.08, 0.08, 0.09), 0.75, 0.0);
        let helmet = standard_material(Color::from_rgb(0.93, 0.94, 0.95), 0.35, 0.05);
        let skin = standard_material(Color::from_rgb(0.76, 0.62, 0.50), 0.7, 0.0);
        let saddle_mat = standard_material(Color::from_rgb(0.12, 0.12, 0.13), 0.6, 0.0);

        let rear = Vector3::new(0.0, WHEEL_RADIUS, 0.42);
        let front = Vector3::new(0.0, WHEEL_RADIUS, -0.62);
        let bb = Vector3::new(0.0, 0.30, 0.02);
        let seat = Vector3::new(0.0, 1.00, 0.18);
        let head_tube = Vector3::new(0.0, 0.78, -0.42);
        let bar = Vector3::new(0.0, 0.96, -0.50);

        self.add_wheel("RearWheel", rear, &tire, &rim);
        self.add_wheel("FrontWheel", front, &tire, &rim);

        let mut base = self.to_gd().upcast::<Node3D>();
        add_tube(&mut base, "Chainstay", rear, bb, 0.035, &frame);
        add_tube(&mut base, "SeatStay", rear, seat, 0.028, &frame);
        add_tube(&mut base, "SeatTube", bb, seat, 0.04, &frame);
        add_tube(&mut base, "DownTube", bb, head_tube, 0.042, &frame);
        add_tube(&mut base, "TopTube", seat, head_tube, 0.034, &frame);
        add_tube(&mut base, "Fork", head_tube, front, 0.032, &frame);
        add_tube(&mut base, "Stem", head_tube, bar, 0.03, &frame);
        add_box(
            &mut base,
            "Handlebar",
            Vector3::new(0.46, 0.028, 0.028),
            bar,
            &frame,
        );
        add_box(
            &mut base,
            "Saddle",
            Vector3::new(0.12, 0.035, 0.26),
            seat + Vector3::new(0.0, 0.04, 0.0),
            &saddle_mat,
        );
        // Crank and two pedals, frozen in a readable pose.
        add_box(
            &mut base,
            "Crank",
            Vector3::new(0.02, 0.18, 0.04),
            bb + Vector3::new(0.08, 0.0, 0.0),
            &frame,
        );
        add_box(
            &mut base,
            "Pedal",
            Vector3::new(0.08, 0.02, 0.05),
            bb + Vector3::new(0.14, -0.09, 0.02),
            &dark,
        );

        let hip = Vector3::new(0.0, 1.02, 0.06);
        let shoulder = Vector3::new(0.0, 1.28, -0.16);
        let head = Vector3::new(0.0, 1.46, -0.24);
        add_box(
            &mut base,
            "Shorts",
            Vector3::new(0.30, 0.22, 0.22),
            hip,
            &dark,
        );
        add_tube(&mut base, "Torso", hip, shoulder, 0.22, &jersey);
        add_box(
            &mut base,
            "Jersey",
            Vector3::new(0.28, 0.16, 0.16),
            (hip + shoulder) * 0.5 + Vector3::new(0.0, 0.02, -0.02),
            &jersey,
        );
        add_sphere(&mut base, "Helmet", 0.13, head, &helmet);
        add_tube(
            &mut base,
            "ArmL",
            shoulder + Vector3::new(-0.12, 0.0, 0.0),
            bar + Vector3::new(-0.16, 0.0, 0.0),
            0.06,
            &skin,
        );
        add_tube(
            &mut base,
            "ArmR",
            shoulder + Vector3::new(0.12, 0.0, 0.0),
            bar + Vector3::new(0.16, 0.0, 0.0),
            0.06,
            &skin,
        );
        add_tube(
            &mut base,
            "ThighL",
            hip + Vector3::new(-0.08, -0.05, 0.0),
            Vector3::new(-0.09, 0.48, -0.16),
            0.09,
            &dark,
        );
        add_tube(
            &mut base,
            "ShinL",
            Vector3::new(-0.09, 0.48, -0.16),
            bb + Vector3::new(-0.08, -0.08, 0.06),
            0.06,
            &skin,
        );
        add_tube(
            &mut base,
            "ThighR",
            hip + Vector3::new(0.08, -0.05, 0.0),
            Vector3::new(0.09, 0.55, 0.16),
            0.09,
            &dark,
        );
        add_tube(
            &mut base,
            "ShinR",
            Vector3::new(0.09, 0.55, 0.16),
            bb + Vector3::new(0.10, 0.08, -0.02),
            0.06,
            &skin,
        );
    }

    fn add_wheel(
        &mut self,
        name: &str,
        at: Vector3,
        tire: &Gd<godot::classes::StandardMaterial3D>,
        rim: &Gd<godot::classes::StandardMaterial3D>,
    ) {
        let mut hub = Node3D::new_alloc();
        hub.set_name(name);
        hub.set_position(at);
        // CylinderMesh is Y-up. Turn it so the axle is X, then spin the hub around X.
        let mut tire_mesh = add_cylinder(
            &mut hub,
            "Tire",
            WHEEL_RADIUS,
            0.028,
            20,
            Vector3::ZERO,
            tire,
        );
        tire_mesh.set_rotation(Vector3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2));
        let mut rim_mesh = add_cylinder(&mut hub, "Rim", 0.20, 0.03, 16, Vector3::ZERO, rim);
        rim_mesh.set_rotation(Vector3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2));
        self.base_mut().add_child(&hub);
        self.wheels.push(hub);
    }
}

fn unit_or(vector: Vector3, fallback: Vector3) -> Vector3 {
    if vector.length_squared() < 1.0e-8 {
        fallback
    } else {
        vector.normalized()
    }
}
