//! Primitive meshes shared by the rider and the placeholder scenery.

use godot::classes::{
    BoxMesh, CylinderMesh, Material, Mesh, MeshInstance3D, SphereMesh, StandardMaterial3D,
};
use godot::obj::Gd;
use godot::prelude::*;

/// Opaque PBR material.
pub fn standard_material(albedo: Color, roughness: f32, metallic: f32) -> Gd<StandardMaterial3D> {
    let mut material = StandardMaterial3D::new_gd();
    material.set_albedo(albedo);
    material.set_roughness(roughness);
    material.set_metallic(metallic);
    material
}

/// Box centered on `position`. Local axes match the parent.
pub fn add_box(
    parent: &mut Gd<Node3D>,
    name: &str,
    size: Vector3,
    position: Vector3,
    material: &Gd<StandardMaterial3D>,
) -> Gd<MeshInstance3D> {
    let mut mesh = BoxMesh::new_gd();
    mesh.set_size(size);
    add_instance(parent, name, mesh.upcast::<Mesh>(), position, material)
}

/// Y-axis cylinder centered on `position`.
pub fn add_cylinder(
    parent: &mut Gd<Node3D>,
    name: &str,
    radius: f32,
    height: f32,
    segments: i32,
    position: Vector3,
    material: &Gd<StandardMaterial3D>,
) -> Gd<MeshInstance3D> {
    let mut mesh = CylinderMesh::new_gd();
    mesh.set_top_radius(radius);
    mesh.set_bottom_radius(radius);
    mesh.set_height(height);
    mesh.set_radial_segments(segments);
    add_instance(parent, name, mesh.upcast::<Mesh>(), position, material)
}

/// Sphere centered on `position`.
pub fn add_sphere(
    parent: &mut Gd<Node3D>,
    name: &str,
    radius: f32,
    position: Vector3,
    material: &Gd<StandardMaterial3D>,
) -> Gd<MeshInstance3D> {
    let mut mesh = SphereMesh::new_gd();
    mesh.set_radius(radius);
    mesh.set_height(radius * 2.0);
    mesh.set_radial_segments(12);
    mesh.set_rings(8);
    add_instance(parent, name, mesh.upcast::<Mesh>(), position, material)
}

/// Box stretched from `start` to `end`. The mesh's local Z axis runs along the segment.
pub fn add_tube(
    parent: &mut Gd<Node3D>,
    name: &str,
    start: Vector3,
    end: Vector3,
    thickness: f32,
    material: &Gd<StandardMaterial3D>,
) -> Gd<MeshInstance3D> {
    let delta = end - start;
    let length = delta.length().max(0.001);
    let mid = (start + end) * 0.5;
    let mut mesh = BoxMesh::new_gd();
    mesh.set_size(Vector3::new(thickness, thickness, length));
    let mut instance = add_instance(parent, name, mesh.upcast::<Mesh>(), mid, material);
    let up = if delta.normalized().dot(Vector3::UP).abs() > 0.95 {
        Vector3::RIGHT
    } else {
        Vector3::UP
    };
    instance.look_at_from_position_ex(mid, end).up(up).done();
    instance
}

fn add_instance(
    parent: &mut Gd<Node3D>,
    name: &str,
    mesh: Gd<Mesh>,
    position: Vector3,
    material: &Gd<StandardMaterial3D>,
) -> Gd<MeshInstance3D> {
    let mut instance = MeshInstance3D::new_alloc();
    instance.set_name(name);
    instance.set_mesh(&mesh);
    instance.set_position(position);
    instance.set_surface_override_material(0, &material.clone().upcast::<Material>());
    parent.add_child(&instance);
    instance
}
