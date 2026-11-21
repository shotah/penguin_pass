//! Scatter, guard rails, posts, and the start arch.
//!
//! Scatter is planned per chunk from the theme seed. Two runs with the same
//! course and theme produce the same instances. Nothing is placed inside
//! [`ROAD_KEEP_OUT_M`] of the centreline.

use godot::classes::base_material_3d::Flags;
use godot::classes::mesh::PrimitiveType;
use godot::classes::multi_mesh::TransformFormat;
use godot::classes::{ArrayMesh, MultiMesh, MultiMeshInstance3D, StandardMaterial3D, SurfaceTool};
use godot::obj::Gd;
use godot::prelude::*;
use pp_course::Course;

use crate::assets::{Catalog, ScatterProp};
use crate::mesh_util::{add_box, standard_material};
use crate::theme::{ROAD_KEEP_OUT_M, ScatterLayer, Theme};
use crate::world::terrain::Field;

/// One placed prop. Coordinates are meters.
#[derive(Clone, Copy)]
pub struct Instance {
    /// World position.
    pub position: Vector3,
    /// Radians around Y, then a small pitch if the layer asks for it.
    pub yaw: f32,
    /// Uniform scale.
    pub scale: f32,
}

/// Counts and a checksum of every accepted instance.
pub struct ScatterReport {
    /// Instances actually placed.
    pub count: i32,
    /// Order-independent mix of positions.
    pub checksum: i64,
    /// Instances that landed inside the road keep-out. Must stay zero.
    pub violations: i32,
}

/// Forest, furniture, and the banner.
pub fn build(
    parent: &mut Gd<Node3D>,
    course: &Course,
    field: &Field,
    theme: &Theme,
    catalog: &Catalog,
) -> ScatterReport {
    let report = scatter_world(parent, field, theme, catalog);
    add_furniture(parent, course, field);
    add_arch(parent, course);
    add_posts(parent, course);
    report
}

/// Plan every chunk again and checksum it. Used to prove the seed is stable.
pub fn recompute_checksum(field: &Field, theme: &Theme) -> (i32, i64, i32) {
    let mut count = 0;
    let mut checksum = 0_i64;
    let mut violations = 0;
    for layer in &theme.scatter {
        visit_chunks(field, layer.max_road_m, |cx, cz| {
            for instance in plan_chunk(field, theme, layer, cx, cz) {
                count += 1;
                checksum = checksum.wrapping_add(instance_hash(layer, &instance));
                if field.road(instance.position.x, instance.position.z).0 < ROAD_KEEP_OUT_M {
                    violations += 1;
                }
            }
        });
    }
    (count, checksum, violations)
}

fn scatter_world(
    parent: &mut Gd<Node3D>,
    field: &Field,
    theme: &Theme,
    catalog: &Catalog,
) -> ScatterReport {
    let meshes = placeholder_meshes();
    let mut count = 0;
    let mut checksum = 0_i64;
    let mut violations = 0;
    let mut seen_unknown = false;
    for layer in &theme.scatter {
        let library = catalog.props_for(layer);
        let fallback = meshes.get(layer.kind.as_str()).map(|mesh| ScatterProp {
            mesh: mesh.clone().upcast(),
            lift: 0.0,
        });
        if library.is_empty() && fallback.is_none() {
            if !seen_unknown {
                godot_error!(
                    "Penguin Pass: no placeholder mesh for scatter kind '{}'",
                    layer.kind
                );
                seen_unknown = true;
            }
            continue;
        }
        visit_chunks(field, layer.max_road_m, |cx, cz| {
            let planned = plan_chunk(field, theme, layer, cx, cz);
            if planned.is_empty() {
                return;
            }
            for instance in &planned {
                count += 1;
                checksum = checksum.wrapping_add(instance_hash(layer, instance));
                if field.road(instance.position.x, instance.position.z).0 < ROAD_KEEP_OUT_M {
                    violations += 1;
                }
            }
            if library.is_empty() {
                if let Some(prop) = &fallback {
                    spawn_chunk(parent, prop, layer, cx, cz, 0, &planned);
                }
                return;
            }
            let mut groups: Vec<Vec<Instance>> = vec![Vec::new(); library.len()];
            for instance in planned {
                let index = variant_index(&instance, library.len());
                groups[index].push(instance);
            }
            for (index, group) in groups.iter().enumerate() {
                if group.is_empty() {
                    continue;
                }
                spawn_chunk(parent, &library[index], layer, cx, cz, index as i32, group);
            }
        });
    }
    ScatterReport {
        count,
        checksum,
        violations,
    }
}

fn visit_chunks(field: &Field, max_road_m: f32, mut visit: impl FnMut(i32, i32)) {
    let min_c = (-1_200.0_f32 / CHUNK_M) as i32;
    let max_c = (1_200.0_f32 / CHUNK_M) as i32;
    let limit = max_road_m + CHUNK_M;
    for cx in min_c..=max_c {
        for cz in min_c..=max_c {
            let x = (cx as f32 + 0.5) * CHUNK_M;
            let z = (cz as f32 + 0.5) * CHUNK_M;
            if field.road(x, z).0 > limit {
                continue;
            }
            visit(cx, cz);
        }
    }
}

const CHUNK_M: f32 = 128.0;

/// Instances for one layer in one chunk. Depends only on that chunk's coordinates.
pub fn plan_chunk(
    field: &Field,
    theme: &Theme,
    layer: &ScatterLayer,
    cx: i32,
    cz: i32,
) -> Vec<Instance> {
    pp_view::scatter::plan_chunk(theme.seed, layer, cx, cz, |x, z| pp_view::scatter::Ground {
        road_m: field.road(x, z).0,
        height: field.height(x, z),
        slope: slope_at(field, x, z),
    })
    .into_iter()
    .map(|placed| Instance {
        position: Vector3::new(placed.x, placed.y, placed.z),
        yaw: placed.yaw,
        scale: placed.scale,
    })
    .collect()
}

fn variant_index(instance: &Instance, count: usize) -> usize {
    pp_view::scatter::variant_index(instance.position.x, instance.position.z, count)
}

fn spawn_chunk(
    parent: &mut Gd<Node3D>,
    prop: &ScatterProp,
    layer: &ScatterLayer,
    cx: i32,
    cz: i32,
    variant: i32,
    planned: &[Instance],
) {
    let mut multi = MultiMesh::new_gd();
    multi.set_transform_format(TransformFormat::TRANSFORM_3D);
    multi.set_mesh(&prop.mesh);
    multi.set_instance_count(planned.len() as i32);
    for (index, instance) in planned.iter().enumerate() {
        let mut basis = Basis::IDENTITY.rotated(Vector3::UP, instance.yaw);
        if layer.tilt_to_slope {
            basis = basis.rotated(Vector3::RIGHT, (instance.yaw * 0.15).clamp(-0.2, 0.2));
        }
        basis = basis.scaled(Vector3::new(instance.scale, instance.scale, instance.scale));
        let position = instance.position + Vector3::UP * prop.lift * instance.scale;
        multi.set_instance_transform(index as i32, Transform3D::new(basis, position));
    }
    let mut node = MultiMeshInstance3D::new_alloc();
    node.set_name(&format!("{}{cx}_{cz}_{variant}", layer.kind));
    node.set_multimesh(&multi);
    node.set_visibility_range_end(layer.visibility_m);
    node.set_cast_shadows_setting(godot::classes::geometry_instance_3d::ShadowCastingSetting::OFF);
    parent.add_child(&node);
}

fn add_furniture(parent: &mut Gd<Node3D>, course: &Course, field: &Field) {
    let mesh = rail_mesh();
    let mut spots = Vec::new();
    let length = course.length_m();
    let mut distance = 0.0;
    while distance < length {
        let here = course.tangent_at(distance);
        let next = course.tangent_at(distance + 8.0);
        let turn = here[0] * next[2] - here[2] * next[0];
        let side = if turn >= 0.0 { 1.0 } else { -1.0 };
        let at = course.position_at(distance);
        let right = Vector3::new(-here[2], 0.0, here[0]);
        let outward = right * side * 4.4;
        let sample_x = at[0] + outward.x;
        let sample_z = at[2] + outward.z;
        let drop = at[1] - field.height(sample_x, sample_z);
        if turn.abs() > 0.035 || drop > 1.1 {
            let yaw = here[2].atan2(here[0]);
            spots.push(Instance {
                position: Vector3::new(at[0], at[1] + 0.08, at[2]) + outward,
                yaw,
                scale: 1.0,
            });
        }
        distance += 8.0;
    }
    if spots.is_empty() {
        return;
    }
    let mut multi = MultiMesh::new_gd();
    multi.set_transform_format(TransformFormat::TRANSFORM_3D);
    multi.set_mesh(&mesh);
    multi.set_instance_count(spots.len() as i32);
    for (index, spot) in spots.iter().enumerate() {
        let basis = Basis::IDENTITY
            .rotated(Vector3::UP, spot.yaw)
            .scaled(Vector3::ONE);
        multi.set_instance_transform(index as i32, Transform3D::new(basis, spot.position));
    }
    let mut node = MultiMeshInstance3D::new_alloc();
    node.set_name("GuardRails");
    node.set_multimesh(&multi);
    parent.add_child(&node);
}

fn add_arch(parent: &mut Gd<Node3D>, course: &Course) {
    // Far enough ahead of the line that the chase camera sees it on the way in.
    let at = course.position_at(18.0);
    let tangent = course.tangent_at(18.0);
    let mut arch = Node3D::new_alloc();
    arch.set_name("StartArch");
    let origin = Vector3::new(at[0], at[1] + 0.08, at[2]);
    let forward = Vector3::new(tangent[0], 0.0, tangent[2]);
    let aim = if forward.length_squared() < 1.0e-6 {
        origin + Vector3::FORWARD
    } else {
        origin + forward.normalized()
    };
    arch.look_at_from_position(origin, aim);
    let post = standard_material(Color::from_rgb(0.95, 0.95, 0.93), 0.45, 0.0);
    let band = standard_material(Color::from_rgb(0.90, 0.28, 0.16), 0.5, 0.0);
    add_box(
        &mut arch,
        "LeftPost",
        Vector3::new(0.22, 4.6, 0.22),
        Vector3::new(-4.0, 2.3, 0.0),
        &post,
    );
    add_box(
        &mut arch,
        "RightPost",
        Vector3::new(0.22, 4.6, 0.22),
        Vector3::new(4.0, 2.3, 0.0),
        &post,
    );
    add_box(
        &mut arch,
        "Beam",
        Vector3::new(8.6, 0.28, 0.28),
        Vector3::new(0.0, 4.7, 0.0),
        &post,
    );
    add_box(
        &mut arch,
        "Band",
        Vector3::new(6.2, 0.7, 0.08),
        Vector3::new(0.0, 4.15, -0.16),
        &band,
    );
    parent.add_child(&arch);
}

fn add_posts(parent: &mut Gd<Node3D>, course: &Course) {
    let mut posts = Node3D::new_alloc();
    posts.set_name("Posts");
    let marker = standard_material(Color::from_rgb(0.95, 0.84, 0.35), 0.55, 0.0);
    let reflector = standard_material(Color::from_rgb(0.9, 0.9, 0.85), 0.3, 0.1);
    let length = course.length_m();
    let mut distance = 20.0;
    let mut index = 0;
    while distance < length - 10.0 {
        let at = course.position_at(distance);
        let tangent = course.tangent_at(distance);
        let right = Vector3::new(-tangent[2], 0.0, tangent[0]).normalized();
        let base = Vector3::new(at[0], at[1] + 0.08, at[2]);
        let marker_here = (distance % 500.0) < 20.0;
        let material = if marker_here { &marker } else { &reflector };
        let height = if marker_here { 1.5 } else { 0.7 };
        add_box(
            &mut posts,
            &format!("Post{index}"),
            Vector3::new(0.12, height, 0.12),
            base + right * 4.6 + Vector3::new(0.0, height * 0.5, 0.0),
            material,
        );
        distance += if marker_here { 480.0 } else { 24.0 };
        index += 1;
    }
    parent.add_child(&posts);
}

fn placeholder_meshes() -> HashMapKind {
    let mut meshes = HashMapKind::default();
    meshes.insert("tree", tree_mesh());
    meshes.insert("bush", bush_mesh());
    meshes.insert("grass", tuft_mesh(Color::from_rgb(0.35, 0.58, 0.22)));
    meshes.insert("flower", tuft_mesh(Color::from_rgb(0.86, 0.72, 0.28)));
    meshes.insert("rock", rock_mesh());
    meshes
}

struct HashMapKind {
    pairs: Vec<(&'static str, Gd<ArrayMesh>)>,
}

impl HashMapKind {
    fn default() -> Self {
        Self { pairs: Vec::new() }
    }

    fn insert(&mut self, kind: &'static str, mesh: Gd<ArrayMesh>) {
        self.pairs.push((kind, mesh));
    }

    fn get(&self, kind: &str) -> Option<&Gd<ArrayMesh>> {
        self.pairs
            .iter()
            .find(|(name, _)| *name == kind)
            .map(|(_, mesh)| mesh)
    }
}

fn tree_mesh() -> Gd<ArrayMesh> {
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    cone(
        &mut tool,
        1.3,
        5.2,
        1.35,
        Color::from_rgb(0.08, 0.32, 0.14),
        8,
    );
    cone(
        &mut tool,
        0.7,
        3.4,
        1.15,
        Color::from_rgb(0.12, 0.40, 0.16),
        8,
    );
    cylinder(
        &mut tool,
        0.0,
        1.5,
        0.18,
        Color::from_rgb(0.36, 0.22, 0.12),
        6,
    );
    finish_mesh(tool)
}

fn bush_mesh() -> Gd<ArrayMesh> {
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    cone(
        &mut tool,
        0.15,
        1.3,
        0.9,
        Color::from_rgb(0.16, 0.42, 0.18),
        7,
    );
    finish_mesh(tool)
}

fn tuft_mesh(color: Color) -> Gd<ArrayMesh> {
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    cone(&mut tool, 0.0, 0.55, 0.22, color, 4);
    finish_mesh(tool)
}

fn rock_mesh() -> Gd<ArrayMesh> {
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    cone(
        &mut tool,
        0.0,
        0.7,
        0.55,
        Color::from_rgb(0.45, 0.44, 0.42),
        5,
    );
    finish_mesh(tool)
}

fn finish_mesh(mut tool: Gd<SurfaceTool>) -> Gd<ArrayMesh> {
    let mut mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut material = StandardMaterial3D::new_gd();
    material.set_flag(Flags::ALBEDO_FROM_VERTEX_COLOR, true);
    material.set_roughness(0.9);
    mesh.surface_set_material(0, &material);
    mesh
}

fn rail_mesh() -> Gd<ArrayMesh> {
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    box_tri(
        &mut tool,
        Vector3::new(0.0, 0.45, 0.0),
        Vector3::new(0.1, 0.9, 0.1),
        Color::from_rgb(0.55, 0.56, 0.58),
    );
    box_tri(
        &mut tool,
        Vector3::new(0.0, 0.85, 4.0),
        Vector3::new(0.08, 0.08, 8.0),
        Color::from_rgb(0.72, 0.74, 0.76),
    );
    finish_mesh(tool)
}

fn cone(tool: &mut Gd<SurfaceTool>, y0: f32, y1: f32, radius: f32, color: Color, sides: i32) {
    let tip = Vector3::new(0.0, y1, 0.0);
    for i in 0..sides {
        let a0 = i as f32 / sides as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / sides as f32 * std::f32::consts::TAU;
        let p0 = Vector3::new(a0.cos() * radius, y0, a0.sin() * radius);
        let p1 = Vector3::new(a1.cos() * radius, y0, a1.sin() * radius);
        let normal = ((p0 + p1) * 0.5 + Vector3::UP).normalized();
        for vertex in [p0, p1, tip] {
            tool.set_normal(normal);
            tool.set_color(color);
            tool.add_vertex(vertex);
        }
    }
}

fn cylinder(tool: &mut Gd<SurfaceTool>, y0: f32, y1: f32, radius: f32, color: Color, sides: i32) {
    for i in 0..sides {
        let a0 = i as f32 / sides as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / sides as f32 * std::f32::consts::TAU;
        let b0 = Vector3::new(a0.cos() * radius, y0, a0.sin() * radius);
        let b1 = Vector3::new(a1.cos() * radius, y0, a1.sin() * radius);
        let t0 = Vector3::new(a0.cos() * radius, y1, a0.sin() * radius);
        let t1 = Vector3::new(a1.cos() * radius, y1, a1.sin() * radius);
        let normal = Vector3::new(a0.cos(), 0.0, a0.sin());
        for vertex in [b0, b1, t1, b0, t1, t0] {
            tool.set_normal(normal);
            tool.set_color(color);
            tool.add_vertex(vertex);
        }
    }
}

fn box_tri(tool: &mut Gd<SurfaceTool>, center: Vector3, size: Vector3, color: Color) {
    let hx = size.x * 0.5;
    let hy = size.y * 0.5;
    let hz = size.z * 0.5;
    let corners = [
        Vector3::new(-hx, -hy, -hz),
        Vector3::new(hx, -hy, -hz),
        Vector3::new(hx, hy, -hz),
        Vector3::new(-hx, hy, -hz),
        Vector3::new(-hx, -hy, hz),
        Vector3::new(hx, -hy, hz),
        Vector3::new(hx, hy, hz),
        Vector3::new(-hx, hy, hz),
    ];
    let faces: [[usize; 4]; 6] = [
        [0, 1, 2, 3],
        [5, 4, 7, 6],
        [4, 0, 3, 7],
        [1, 5, 6, 2],
        [3, 2, 6, 7],
        [4, 5, 1, 0],
    ];
    for face in faces {
        let a = center + corners[face[0]];
        let b = center + corners[face[1]];
        let c = center + corners[face[2]];
        let d = center + corners[face[3]];
        let normal = (b - a).cross(d - a).normalized();
        for vertex in [a, b, c, a, c, d] {
            tool.set_normal(normal);
            tool.set_color(color);
            tool.add_vertex(vertex);
        }
    }
}

fn slope_at(field: &Field, x: f32, z: f32) -> f32 {
    let step = 2.0;
    let dx = field.height(x + step, z) - field.height(x - step, z);
    let dz = field.height(x, z + step) - field.height(x, z - step);
    ((dx * dx + dz * dz).sqrt() / (2.0 * step)).min(2.0)
}

fn instance_hash(layer: &ScatterLayer, instance: &Instance) -> i64 {
    pp_view::scatter::instance_hash(
        &layer.kind,
        instance.position.x,
        instance.position.y,
        instance.position.z,
    )
}
