//! Procedural road bike and rider.
//!
//! Kept apart from the rest of the scene so a licensed model can replace
//! [`Rider::assemble`] later. Wheels spin with distance. Cranks and legs
//! follow cadence and hold still when cadence is missing or zero.

use godot::classes::{INode3D, Node3D};
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;

use crate::mesh_util::{add_box, add_cylinder, add_sphere, add_tube, standard_material};

const WHEEL_RADIUS: f32 = 0.34;
const CRANK_LENGTH: f32 = 0.17;

/// Cyclist stand-in. Origin sits on the road. The mesh faces local -Z.
#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Rider {
    base: Base<Node3D>,
    wheels: Vec<Gd<Node3D>>,
    crank: Option<Gd<Node3D>>,
    thighs: Vec<Gd<Node3D>>,
    shins: Vec<Gd<Node3D>>,
    wheel_angle: f32,
    crank_angle: f32,
    built: bool,
}

#[godot_api]
impl INode3D for Rider {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            wheels: Vec::new(),
            crank: None,
            thighs: Vec::new(),
            shins: Vec::new(),
            wheel_angle: 0.0,
            crank_angle: 0.0,
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
    /// World position of the rider origin.
    #[func]
    pub fn rider_origin(&self) -> Vector3 {
        self.base().get_global_position()
    }

    /// Wheel rotation, radians, from distance traveled.
    #[func]
    pub fn wheel_angle(&self) -> f64 {
        f64::from(self.wheel_angle)
    }

    /// Crank rotation, radians. Stationary when cadence is 0 or missing.
    #[func]
    pub fn crank_angle(&self) -> f64 {
        f64::from(self.crank_angle)
    }
}

impl Rider {
    /// Place the bike, spin the wheels, and pose the legs.
    ///
    /// `lean` rolls the frame into the turn. Positive rolls to the rider's right.
    pub fn place(
        &mut self,
        position: Vector3,
        tangent: Vector3,
        distance_m: f64,
        cadence_rpm: Option<f32>,
        dt: f64,
        lean: f32,
    ) {
        let forward = unit_or(tangent, Vector3::FORWARD);
        let up = if forward.dot(Vector3::UP).abs() > 0.95 {
            Vector3::RIGHT
        } else {
            Vector3::UP
        };
        self.base_mut()
            .look_at_from_position_ex(position, position + forward)
            .up(up)
            .done();
        self.base_mut().rotate_object_local(Vector3::FORWARD, lean);
        self.wheel_angle = (distance_m / f64::from(WHEEL_RADIUS)) as f32;
        if dt > 0.0
            && let Some(rpm) = cadence_rpm.filter(|rpm| *rpm > 0.5)
        {
            self.crank_angle += rpm * dt as f32 * std::f32::consts::TAU / 60.0;
        }
        let spin = self.wheel_angle;
        for wheel in &mut self.wheels {
            wheel.set_rotation(Vector3::new(spin, 0.0, 0.0));
        }
        if let Some(crank) = self.crank.as_mut() {
            crank.set_rotation(Vector3::new(self.crank_angle, 0.0, 0.0));
        }
        self.pose_legs();
    }

    fn pose_legs(&mut self) {
        let bb = Vector3::new(0.0, 0.29, -0.02);
        let angle = self.crank_angle;
        for (index, side) in [-1.0_f32, 1.0].into_iter().enumerate() {
            let phase = if side < 0.0 {
                angle
            } else {
                angle + std::f32::consts::PI
            };
            let pedal = bb
                + Vector3::new(
                    side * 0.09,
                    phase.sin() * CRANK_LENGTH,
                    -phase.cos() * CRANK_LENGTH,
                );
            let hip = Vector3::new(side * 0.09, 0.96, 0.08);
            let knee = (hip + pedal) * 0.5 + Vector3::new(side * 0.05, 0.06, 0.04);
            if let Some(thigh) = self.thighs.get_mut(index) {
                span(thigh, hip, knee);
            }
            if let Some(shin) = self.shins.get_mut(index) {
                span(shin, knee, pedal);
            }
        }
    }

    fn assemble(&mut self) {
        let frame = standard_material(Color::from_rgb(0.10, 0.16, 0.28), 0.28, 0.65);
        let tire = standard_material(Color::from_rgb(0.08, 0.08, 0.09), 0.9, 0.0);
        let rim = standard_material(Color::from_rgb(0.75, 0.76, 0.78), 0.25, 0.8);
        let jersey = standard_material(Color::from_rgb(0.93, 0.24, 0.16), 0.65, 0.0);
        let dark = standard_material(Color::from_rgb(0.07, 0.07, 0.08), 0.7, 0.0);
        let helmet = standard_material(Color::from_rgb(0.95, 0.95, 0.97), 0.3, 0.05);
        let skin = standard_material(Color::from_rgb(0.78, 0.62, 0.48), 0.65, 0.0);
        let saddle_mat = standard_material(Color::from_rgb(0.12, 0.10, 0.10), 0.55, 0.0);

        let rear = Vector3::new(0.0, WHEEL_RADIUS, 0.40);
        let front = Vector3::new(0.0, WHEEL_RADIUS, -0.70);
        let bb = Vector3::new(0.0, 0.29, -0.02);
        let seat = Vector3::new(0.0, 0.98, 0.12);
        let head_tube = Vector3::new(0.0, 0.72, -0.38);
        let bar = Vector3::new(0.0, 0.90, -0.46);

        self.add_wheel("RearWheel", rear, &tire, &rim);
        self.add_wheel("FrontWheel", front, &tire, &rim);

        let mut root = self.to_gd().upcast::<Node3D>();
        add_tube(&mut root, "Chainstay", rear, bb, 0.03, &frame);
        add_tube(&mut root, "SeatStay", rear, seat, 0.022, &frame);
        add_tube(&mut root, "SeatTube", bb, seat, 0.034, &frame);
        add_tube(&mut root, "DownTube", bb, head_tube, 0.036, &frame);
        add_tube(&mut root, "TopTube", seat, head_tube, 0.028, &frame);
        add_tube(&mut root, "Fork", head_tube, front, 0.026, &frame);
        add_tube(&mut root, "Stem", head_tube, bar, 0.026, &frame);
        add_box(
            &mut root,
            "Hoods",
            Vector3::new(0.42, 0.03, 0.04),
            bar,
            &dark,
        );
        add_box(
            &mut root,
            "DropsL",
            Vector3::new(0.03, 0.10, 0.08),
            bar + Vector3::new(-0.20, -0.06, 0.02),
            &dark,
        );
        add_box(
            &mut root,
            "DropsR",
            Vector3::new(0.03, 0.10, 0.08),
            bar + Vector3::new(0.20, -0.06, 0.02),
            &dark,
        );
        add_box(
            &mut root,
            "Saddle",
            Vector3::new(0.12, 0.03, 0.28),
            seat + Vector3::new(0.0, 0.04, -0.02),
            &saddle_mat,
        );

        let mut crank = Node3D::new_alloc();
        crank.set_name("Crank");
        crank.set_position(bb);
        add_box(
            &mut crank,
            "Arm",
            Vector3::new(0.02, 0.03, CRANK_LENGTH * 2.0),
            Vector3::ZERO,
            &frame,
        );
        add_box(
            &mut crank,
            "PedalL",
            Vector3::new(0.08, 0.02, 0.05),
            Vector3::new(-0.08, 0.0, -CRANK_LENGTH),
            &dark,
        );
        add_box(
            &mut crank,
            "PedalR",
            Vector3::new(0.08, 0.02, 0.05),
            Vector3::new(0.08, 0.0, CRANK_LENGTH),
            &dark,
        );
        root.add_child(&crank);
        self.crank = Some(crank);

        let hip = Vector3::new(0.0, 0.98, 0.14);
        let shoulder = Vector3::new(0.0, 1.24, 0.02);
        let head = Vector3::new(0.0, 1.48, 0.06);
        add_box(
            &mut root,
            "Shorts",
            Vector3::new(0.28, 0.16, 0.20),
            hip,
            &dark,
        );
        add_tube(&mut root, "Torso", hip, shoulder, 0.18, &jersey);
        add_sphere(&mut root, "Helmet", 0.12, head, &helmet);
        add_sphere(
            &mut root,
            "Visor",
            0.04,
            head + Vector3::new(0.0, -0.02, -0.10),
            &dark,
        );
        add_tube(
            &mut root,
            "ArmL",
            shoulder + Vector3::new(-0.12, -0.02, 0.0),
            bar + Vector3::new(-0.16, 0.0, 0.0),
            0.05,
            &skin,
        );
        add_tube(
            &mut root,
            "ArmR",
            shoulder + Vector3::new(0.12, -0.02, 0.0),
            bar + Vector3::new(0.16, 0.0, 0.0),
            0.05,
            &skin,
        );
        add_box(
            &mut root,
            "HandL",
            Vector3::new(0.06, 0.04, 0.08),
            bar + Vector3::new(-0.16, 0.0, 0.0),
            &skin,
        );
        add_box(
            &mut root,
            "HandR",
            Vector3::new(0.06, 0.04, 0.08),
            bar + Vector3::new(0.16, 0.0, 0.0),
            &skin,
        );
        self.thighs.push(bone(&mut root, "ThighL", &dark));
        self.thighs.push(bone(&mut root, "ThighR", &dark));
        self.shins.push(bone(&mut root, "ShinL", &skin));
        self.shins.push(bone(&mut root, "ShinR", &skin));
        self.pose_legs();
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
        let mut tire_mesh = add_cylinder(
            &mut hub,
            "Tire",
            WHEEL_RADIUS,
            0.028,
            18,
            Vector3::ZERO,
            tire,
        );
        tire_mesh.set_rotation(Vector3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2));
        let mut rim_mesh = add_cylinder(&mut hub, "Rim", 0.22, 0.02, 12, Vector3::ZERO, rim);
        rim_mesh.set_rotation(Vector3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2));
        for spoke in 0..6 {
            let mut bar = add_box(
                &mut hub,
                &format!("Spoke{spoke}"),
                Vector3::new(0.012, 0.012, WHEEL_RADIUS * 1.7),
                Vector3::ZERO,
                rim,
            );
            bar.set_rotation(Vector3::new(
                spoke as f32 * std::f32::consts::TAU / 6.0,
                0.0,
                0.0,
            ));
        }
        self.base_mut().add_child(&hub);
        self.wheels.push(hub);
    }
}

fn bone(
    parent: &mut Gd<Node3D>,
    name: &str,
    material: &Gd<godot::classes::StandardMaterial3D>,
) -> Gd<Node3D> {
    let mut node = Node3D::new_alloc();
    node.set_name(name);
    let mut shape = godot::classes::BoxMesh::new_gd();
    shape.set_size(Vector3::new(0.07, 0.07, 1.0));
    let mut mesh = godot::classes::MeshInstance3D::new_alloc();
    mesh.set_name("Mesh");
    mesh.set_mesh(&shape);
    mesh.set_surface_override_material(0, material);
    node.add_child(&mesh);
    parent.add_child(&node);
    node
}

fn span(node: &mut Gd<Node3D>, from: Vector3, to: Vector3) {
    let delta = to - from;
    let length = delta.length().max(0.05);
    let mid = (from + to) * 0.5;
    let up = if delta.normalized().dot(Vector3::UP).abs() > 0.92 {
        Vector3::RIGHT
    } else {
        Vector3::UP
    };
    node.look_at_from_position_ex(mid, to).up(up).done();
    node.set_scale(Vector3::new(1.0, 1.0, length));
}

fn unit_or(vector: Vector3, fallback: Vector3) -> Vector3 {
    if vector.length_squared() < 1.0e-8 {
        fallback
    } else {
        vector.normalized()
    }
}
