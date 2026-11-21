//! Materials and box meshes shared by the rider and the roadside furniture.

use godot::classes::{BoxMesh, Material, Mesh, MeshInstance3D, StandardMaterial3D};
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
