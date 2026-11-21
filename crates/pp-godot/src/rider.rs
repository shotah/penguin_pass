//! Road bike and rider.
//!
//! The meshes and the pose come from `pp-rider`, which knows nothing about
//! Godot. This file turns its surfaces into one `ArrayMesh` per part once,
//! in `ready`, and sets one transform per part in [`Rider::place`]. Wheels
//! spin with distance. Cranks and legs follow cadence and hold still when
//! cadence is missing or zero.

use godot::classes::geometry_instance_3d::ShadowCastingSetting;
use godot::classes::mesh::{ArrayType, PrimitiveType};
use godot::classes::{ArrayMesh, INode3D, MeshInstance3D, Node3D, StandardMaterial3D};
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;
use pp_rider::{Part, PartId, PoseInput, RideAnimator, RideSample, RiderModel, RiderStyle};

use crate::mesh_util::{add_cylinder, add_sphere, add_tube, standard_material};

/// Where the model's road contact sits in the node, in meters.
///
/// `Game` places this node 0.06 m above the course line and the road ribbon
/// is drawn 0.08 m above it, so the tyres stand 0.02 m above the node origin.
const MODEL_ORIGIN: Vector3 = Vector3::new(0.0, 0.02, 0.0);

/// A camera closer to the rider's eyes than this does not draw the head or
/// the torso, in meters. At rest on the bike the handlebar camera is 0.3 to
/// 0.5 m from them, depending on the posture; the chase camera is 3.5 m
/// away or more.
const CLOSE_CAMERA_M: f32 = 0.75;

/// The parts a camera at the rider's head would look out through.
const HIDDEN_FROM_CLOSE_CAMERA: [PartId; 2] = [PartId::Head, PartId::Torso];

// `mesh_util` still has three helpers that only the placeholder rider
// called. That file belongs to the scenery. Naming them here keeps the
// crate free of dead-code errors until they are deleted there; then delete
// this block and the three imports.
const _: () = {
    let _ = add_cylinder;
    let _ = add_sphere;
    let _ = add_tube;
};

/// What the game told the rider about the ride, for the next [`Rider::place`].
#[derive(Clone, Copy)]
struct RideState {
    /// Power over FTP. `None` when the FTP is not a usable number.
    power_ratio: Option<f32>,
    grade_pct: f32,
    speed_mps: f32,
}

/// The cyclist and the bike. Origin sits on the road. The mesh faces local -Z.
#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Rider {
    base: Base<Node3D>,
    model: Option<RiderModel>,
    /// One node per [`PartId`], in `PartId::ALL` order.
    parts: Vec<Gd<MeshInstance3D>>,
    animator: RideAnimator,
    ride: Option<RideState>,
    cadence_override: Option<f32>,
    input: PoseInput,
    /// Eye position and facing in this node's space.
    eye: Vector3,
    eye_forward: Vector3,
    hidden_from_camera: bool,
    built: bool,
}

#[godot_api]
impl INode3D for Rider {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            model: None,
            parts: Vec::new(),
            animator: RideAnimator::new(),
            ride: None,
            cadence_override: None,
            input: PoseInput::default(),
            eye: Vector3::new(0.0, 1.5, -0.4),
            eye_forward: Vector3::FORWARD,
            hidden_from_camera: false,
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

    /// Runs after `Game::process` has moved the camera for this frame.
    fn process(&mut self, _delta: f64) {
        self.hide_from_a_close_camera();
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
        self.animator.wheel_angle_rad()
    }

    /// Crank rotation, radians. Stationary when cadence is 0 or missing.
    #[func]
    pub fn crank_angle(&self) -> f64 {
        self.animator.crank_angle_rad()
    }

    /// Tell the rider about the ride, for the next [`Rider::place`].
    ///
    /// `power_w` over `ftp_w` sets how hard the upper body works. `grade_pct`
    /// (percent, positive uphill) and `speed_mps` decide when the rider
    /// stands on the pedals and when they tuck. Call it every frame, just
    /// before `place`. When it is not called, `place` reads the grade from
    /// the tangent, estimates the speed from the distance, and guesses the
    /// effort from the cadence and the grade.
    #[func]
    pub fn set_ride_state(&mut self, power_w: f32, ftp_w: f32, grade_pct: f32, speed_mps: f32) {
        let power_ratio = (ftp_w.is_finite() && ftp_w >= 1.0 && power_w.is_finite())
            .then(|| power_w.max(0.0) / ftp_w);
        self.ride = Some(RideState {
            power_ratio,
            grade_pct,
            speed_mps,
        });
    }

    /// World position of the rider's eyes, for a first-person camera.
    #[func]
    pub fn eye_position(&self) -> Vector3 {
        self.base().get_global_transform() * self.eye
    }

    /// Unit vector the rider's head faces, in world space.
    #[func]
    pub fn eye_forward(&self) -> Vector3 {
        unit_or(
            self.base().get_global_transform().basis * self.eye_forward,
            Vector3::FORWARD,
        )
    }

    /// Out-of-saddle blend this frame. 0 is seated, 1 is standing.
    #[func]
    pub fn stand_blend(&self) -> f64 {
        f64::from(self.input.stand)
    }

    /// Tuck blend this frame. 0 is on the hoods, 1 is in the drops.
    #[func]
    pub fn tuck_blend(&self) -> f64 {
        f64::from(self.input.tuck)
    }

    /// Steering angle this frame, in radians. Positive is to the rider's right.
    #[func]
    pub fn steer_angle(&self) -> f64 {
        f64::from(self.input.steer_rad)
    }

    /// How hard the rider is shown working this frame, 0 to 1.
    #[func]
    pub fn effort(&self) -> f64 {
        f64::from(self.input.effort)
    }

    /// Use `rpm` in place of the cadence `place` is given, so a picture can
    /// show a slow grind while the simulated rider spins. Negative clears it.
    #[func]
    pub fn debug_override_cadence(&mut self, rpm: f64) {
        self.cadence_override = (rpm >= 0.0).then_some(rpm as f32);
    }
}

impl Rider {
    /// Place the bike, spin the wheels, and pose the rider.
    ///
    /// `lean` rolls the frame into the turn. Positive rolls to the rider's
    /// right, and the bars turn a little the same way. A `dt` of zero is a
    /// cut: nothing turns and the posture jumps to where the ride puts it.
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

        let ride = self.ride.take();
        let sample = RideSample {
            distance_m,
            cadence_rpm: self.cadence_override.or(cadence_rpm),
            lean_rad: lean,
            grade_pct: ride.map_or_else(|| grade_pct(forward), |ride| ride.grade_pct),
            speed_mps: ride.map(|ride| ride.speed_mps),
            power_ratio: ride.and_then(|ride| ride.power_ratio),
        };
        self.input = self.animator.step(&sample, dt);
        self.pose_parts();
    }

    /// Give every part its transform for the current pose input.
    fn pose_parts(&mut self) {
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let pose = model.pose(&self.input);
        for (node, transform) in self.parts.iter_mut().zip(&pose.transforms) {
            let [x, y, z, origin] = transform.to_cols().map(vector);
            node.set_transform(Transform3D::new(
                Basis::from_cols(x, y, z),
                origin + MODEL_ORIGIN,
            ));
        }
        self.eye = vector(pose.anchors.eye) + MODEL_ORIGIN;
        self.eye_forward = vector(pose.anchors.eye_forward);
    }

    /// Draw only the shadow of the head and the torso while the camera is
    /// at the rider's head.
    ///
    /// The handlebar camera sits in the rider's chest. Seated, it looks out
    /// between the arms. A tucked rider lowers the collar and the helmet into
    /// its view, and a camera at the eyes is inside the head.
    fn hide_from_a_close_camera(&mut self) {
        let camera = self
            .base()
            .get_viewport()
            .and_then(|viewport| viewport.get_camera_3d());
        let Some(camera) = camera else {
            return;
        };
        let eye = self.base().get_global_transform() * self.eye;
        let hidden = camera.get_global_position().distance_to(eye) < CLOSE_CAMERA_M;
        if hidden == self.hidden_from_camera {
            return;
        }
        self.hidden_from_camera = hidden;
        let setting = if hidden {
            ShadowCastingSetting::SHADOWS_ONLY
        } else {
            ShadowCastingSetting::ON
        };
        for part in HIDDEN_FROM_CLOSE_CAMERA {
            if let Some(node) = self.parts.get_mut(part.index()) {
                node.set_cast_shadows_setting(setting);
            }
        }
    }

    /// Build one mesh node per part. Surfaces that share a look share a material.
    fn assemble(&mut self) {
        let model = RiderModel::build(&RiderStyle::default());
        let mut materials: Vec<(pp_rider::Material, Gd<StandardMaterial3D>)> = Vec::new();
        for part in model.parts() {
            let mut node = MeshInstance3D::new_alloc();
            node.set_name(part.name());
            node.set_mesh(&part_mesh(part, &mut materials));
            node.set_cast_shadows_setting(ShadowCastingSetting::ON);
            self.base_mut().add_child(&node);
            self.parts.push(node);
        }
        self.model = Some(model);
        self.pose_parts();
    }
}

/// One `ArrayMesh` surface per `pp-rider` surface, wound the way Godot draws.
fn part_mesh(
    part: &Part,
    materials: &mut Vec<(pp_rider::Material, Gd<StandardMaterial3D>)>,
) -> Gd<ArrayMesh> {
    let mut mesh = ArrayMesh::new_gd();
    for (index, surface) in part.surfaces().iter().enumerate() {
        let positions: PackedVector3Array = surface.positions.iter().copied().map(vector).collect();
        let normals: PackedVector3Array = surface.normals.iter().copied().map(vector).collect();
        let indices: PackedInt32Array = surface
            .indices_clockwise()
            .into_iter()
            .map(|index| index as i32)
            .collect();
        let mut arrays = VarArray::new();
        arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
        arrays.set(ArrayType::VERTEX.ord() as usize, &positions.to_variant());
        arrays.set(ArrayType::NORMAL.ord() as usize, &normals.to_variant());
        arrays.set(ArrayType::INDEX.ord() as usize, &indices.to_variant());
        mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);

        let wanted = surface.material;
        let material = match materials.iter().find(|(known, _)| *known == wanted) {
            Some((_, material)) => material.clone(),
            None => {
                // `base_color` is sRGB, which is what `Color::from_rgb` takes.
                let [r, g, b] = wanted.base_color;
                let material =
                    standard_material(Color::from_rgb(r, g, b), wanted.roughness, wanted.metallic);
                materials.push((wanted, material.clone()));
                material
            }
        };
        mesh.surface_set_material(index as i32, &material);
    }
    mesh
}

/// Road grade along `forward`, in percent. Positive is uphill.
fn grade_pct(forward: Vector3) -> f32 {
    let run = Vector2::new(forward.x, forward.z).length();
    if run < 1.0e-4 {
        0.0
    } else {
        forward.y / run * 100.0
    }
}

fn vector(v: [f32; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}

fn unit_or(vector: Vector3, fallback: Vector3) -> Vector3 {
    if vector.length_squared() < 1.0e-8 {
        fallback
    } else {
        vector.normalized()
    }
}
