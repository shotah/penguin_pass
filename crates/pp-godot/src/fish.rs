//! Low-poly fish on the road. Built in code: body, tail, fins, and an eye.

use godot::classes::mesh::PrimitiveType;
use godot::classes::{
    ArrayMesh, Material, MeshInstance3D, Node3D, StandardMaterial3D, SurfaceTool,
};
use godot::obj::Gd;
use godot::prelude::*;

use crate::road::flat_right;

/// Body mesh scale for light-yellow coin fish (~0.9 m long).
const COIN_SCALE: f32 = 0.48;
/// Body mesh scale for gold-orange boost fish (~1.4 m long).
const BOOST_SCALE: f32 = 0.74;
const EYE_WHITE_RADIUS: f32 = 0.09;
const EYE_PUPIL_RADIUS: f32 = 0.04;

/// One fish the session says is still ahead.
pub struct FishSpot {
    /// Stable index from the field.
    pub index: u32,
    /// Ride distance, same scale as the rider's total distance.
    pub distance_m: f64,
    /// Lane slot. `0` is the left asphalt edge. Fish stay on the paved slots.
    pub slot: u8,
    /// Larger, gold, and glowing.
    pub golden: bool,
}

struct Live {
    index: u32,
    node: Gd<Node3D>,
    bob: f32,
}

struct Pop {
    node: Gd<Node3D>,
    age: f32,
}

/// The fish currently on the course, plus the ones popping.
pub struct FishSchool {
    root: Gd<Node3D>,
    live: Vec<Live>,
    pops: Vec<Pop>,
    coin: Gd<StandardMaterial3D>,
    boost: Gd<StandardMaterial3D>,
    white: Gd<StandardMaterial3D>,
    pupil: Gd<StandardMaterial3D>,
    body_coin: Gd<ArrayMesh>,
    body_boost: Gd<ArrayMesh>,
    eye_white: Gd<ArrayMesh>,
    eye_pupil: Gd<ArrayMesh>,
}

impl FishSchool {
    /// Parent the school under `parent`.
    pub fn new(parent: &mut Gd<Node3D>) -> Self {
        let mut root = Node3D::new_alloc();
        root.set_name("Fish");
        parent.add_child(&root);
        let coin = flat_color(Color::from_rgb(1.0, 0.95, 0.55));
        let boost = glow_color(Color::from_rgb(1.0, 0.56, 0.06));
        let white = flat_color(Color::from_rgb(0.98, 0.98, 1.0));
        let pupil = flat_color(Color::from_rgb(0.05, 0.05, 0.08));
        Self {
            root,
            live: Vec::new(),
            pops: Vec::new(),
            coin,
            boost,
            white,
            pupil,
            body_coin: fish_mesh(COIN_SCALE),
            body_boost: fish_mesh(BOOST_SCALE),
            eye_white: disc(EYE_WHITE_RADIUS),
            eye_pupil: disc(EYE_PUPIL_RADIUS),
        }
    }

    /// Move the visible fish and play collects. `volume` is 0 to 1.
    pub fn sync(
        &mut self,
        spots: &[FishSpot],
        course: &pp_course::Course,
        lane: pp_view::lane::LaneSpec,
        dt: f32,
        volume: f32,
        combo: u32,
    ) {
        let dt = dt.max(0.0);
        let mut keep = Vec::new();
        for spot in spots {
            if let Some(pos) = self.live.iter().position(|live| live.index == spot.index) {
                keep.push(self.live.swap_remove(pos));
            } else {
                keep.push(self.spawn(spot));
            }
        }
        let gone = std::mem::replace(&mut self.live, keep);
        for old in gone {
            self.pops.push(Pop {
                node: old.node,
                age: 0.0,
            });
        }
        for live in &mut self.live {
            let Some(spot) = spots.iter().find(|spot| spot.index == live.index) else {
                continue;
            };
            live.bob = (live.bob + dt * 2.2).rem_euclid(std::f32::consts::TAU);
            place_fish(&mut live.node, course, lane, spot, live.bob);
        }
        self.pops.retain_mut(|pop| {
            pop.age += dt;
            let t = (pop.age / 0.7).clamp(0.0, 1.0);
            let scale = 1.0 + t * 1.1;
            pop.node.set_scale(Vector3::ONE * scale);
            pop.node.translate(Vector3::UP * dt * 1.4);
            if pop.age >= 0.7 {
                pop.node.queue_free();
                false
            } else {
                true
            }
        });
        let _ = (combo, volume);
    }

    fn spawn(&mut self, spot: &FishSpot) -> Live {
        let mut node = Node3D::new_alloc();
        node.set_name(&format!("Fish{}", spot.index));
        let scale = if spot.golden { BOOST_SCALE } else { COIN_SCALE };
        let mesh = if spot.golden {
            self.body_boost.clone()
        } else {
            self.body_coin.clone()
        };
        let material = if spot.golden {
            self.boost.clone()
        } else {
            self.coin.clone()
        };
        let mut body = MeshInstance3D::new_alloc();
        body.set_name("Body");
        body.set_mesh(&mesh);
        body.set_surface_override_material(0, &material.clone().upcast::<Material>());
        node.add_child(&body);
        let eye_scale = Vector3::ONE * scale;
        let mut eye = MeshInstance3D::new_alloc();
        eye.set_name("Eye");
        eye.set_mesh(&self.eye_white);
        eye.set_scale(eye_scale);
        eye.set_position(Vector3::new(0.50, 0.05, -0.10) * scale);
        eye.set_surface_override_material(0, &self.white.clone().upcast::<Material>());
        node.add_child(&eye);
        let mut pupil = MeshInstance3D::new_alloc();
        pupil.set_name("Pupil");
        pupil.set_mesh(&self.eye_pupil);
        pupil.set_scale(eye_scale);
        pupil.set_position(Vector3::new(0.54, 0.05, -0.125) * scale);
        pupil.set_surface_override_material(0, &self.pupil.clone().upcast::<Material>());
        node.add_child(&pupil);
        self.root.add_child(&node);
        Live {
            index: spot.index,
            node,
            bob: spot.index as f32,
        }
    }
}

fn place_fish(
    node: &mut Gd<Node3D>,
    course: &pp_course::Course,
    lane: pp_view::lane::LaneSpec,
    spot: &FishSpot,
    bob: f32,
) {
    let distance = spot.distance_m;
    let position = course.position_at(distance);
    let tangent = course.tangent_at(distance);
    let tangent_v = Vector3::new(tangent[0], tangent[1], tangent[2]);
    let side = flat_right(tangent_v);
    let offset = lane.offset_for_slot(spot.slot);
    let hover = 0.95 + bob.sin() * 0.08;
    let origin = Vector3::new(position[0], position[1] + hover, position[2]) + side * offset;
    // The side faces the rider. Local -Z points back down the road, so the
    // profile (nose at +X, forked tail at -X) is the silhouette.
    let toward_rider = unit_or(-tangent_v, Vector3::FORWARD);
    let up = if toward_rider.dot(Vector3::UP).abs() > 0.92 {
        Vector3::RIGHT
    } else {
        Vector3::UP
    };
    node.look_at_from_position_ex(origin, origin + toward_rider)
        .up(up)
        .done();
    node.rotate_object_local(Vector3::FORWARD, bob.sin() * 0.10);
    node.rotate_object_local(Vector3::UP, bob.cos() * 0.05);
}

fn glow_color(color: Color) -> Gd<StandardMaterial3D> {
    let mut material = flat_color(color);
    material.set_shading_mode(godot::classes::base_material_3d::ShadingMode::PER_PIXEL);
    material.set_emission(color);
    material.set_emission_energy_multiplier(3.0);
    material
}

fn flat_color(color: Color) -> Gd<StandardMaterial3D> {
    let mut material = StandardMaterial3D::new_gd();
    material.set_albedo(color);
    material.set_shading_mode(godot::classes::base_material_3d::ShadingMode::UNSHADED);
    material.set_cull_mode(godot::classes::base_material_3d::CullMode::DISABLED);
    material
}

/// Side-on fish. Local +X is the nose, local -X the forked tail, local -Z the eye.
///
/// The body is convex. The fork, the dorsal fin, and the pectoral fin are
/// separate triangles so a fan does not turn the tail into a star.
fn fish_mesh(scale: f32) -> Gd<ArrayMesh> {
    let body: [(f32, f32); 8] = [
        (0.92, 0.0),
        (0.62, -0.16),
        (0.15, -0.24),
        (-0.35, -0.12),
        (-0.52, 0.0),
        (-0.38, 0.14),
        (0.05, 0.22),
        (0.55, 0.16),
    ];
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    fill_prism(&mut tool, &body, 0.08 * scale, scale);
    let p = |x: f32, y: f32| Vector3::new(x * scale, y * scale, -0.09 * scale);
    // Dorsal fin.
    tri(&mut tool, p(0.02, 0.18), p(0.08, 0.42), p(-0.18, 0.14));
    // Forked tail, two blades with a gap between the tips.
    tri(&mut tool, p(-0.48, 0.06), p(-0.95, 0.36), p(-0.58, 0.0));
    tri(&mut tool, p(-0.48, -0.04), p(-0.58, 0.0), p(-0.92, -0.32));
    // Pectoral fin.
    tri(&mut tool, p(0.12, -0.06), p(-0.08, -0.42), p(-0.22, 0.0));
    tool.commit().unwrap_or_else(ArrayMesh::new_gd)
}

fn fill_prism(tool: &mut Gd<SurfaceTool>, outline: &[(f32, f32)], half: f32, scale: f32) {
    let n = outline.len();
    let (mut cx, mut cy) = (0.0, 0.0);
    for (x, y) in outline {
        cx += *x;
        cy += *y;
    }
    cx = cx / n as f32 * scale;
    cy = cy / n as f32 * scale;
    let near = Vector3::new(cx, cy, -half);
    let far = Vector3::new(cx, cy, half);
    for index in 0..n {
        let (x0, y0) = outline[index];
        let (x1, y1) = outline[(index + 1) % n];
        let a = Vector3::new(x0 * scale, y0 * scale, -half);
        let b = Vector3::new(x1 * scale, y1 * scale, -half);
        let c = Vector3::new(x0 * scale, y0 * scale, half);
        let d = Vector3::new(x1 * scale, y1 * scale, half);
        tri(tool, near, a, b);
        tri(tool, far, d, c);
        tri(tool, a, c, b);
        tri(tool, b, c, d);
    }
}

/// A flat disc in the XY plane, facing the rider.
fn disc(radius: f32) -> Gd<ArrayMesh> {
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    let steps = 12;
    for step in 0..steps {
        let a0 = step as f32 / steps as f32 * std::f32::consts::TAU;
        let a1 = (step + 1) as f32 / steps as f32 * std::f32::consts::TAU;
        let p0 = Vector3::new(a0.cos() * radius, a0.sin() * radius, 0.0);
        let p1 = Vector3::new(a1.cos() * radius, a1.sin() * radius, 0.0);
        tri(&mut tool, Vector3::ZERO, p1, p0);
    }
    tool.commit().unwrap_or_else(ArrayMesh::new_gd)
}

fn tri(tool: &mut Gd<SurfaceTool>, a: Vector3, b: Vector3, c: Vector3) {
    let normal = (b - a).cross(c - a);
    let normal = if normal.length_squared() < 1.0e-8 {
        Vector3::UP
    } else {
        normal.normalized()
    };
    for point in [a, b, c] {
        tool.set_normal(normal);
        tool.add_vertex(point);
    }
}

fn unit_or(vector: Vector3, fallback: Vector3) -> Vector3 {
    if vector.length_squared() < 1.0e-8 {
        fallback
    } else {
        vector.normalized()
    }
}
