//! Landmarks the rider can aim at, plus one peak that stays on the horizon.
//!
//! The places come from [`pp_world::place_landmarks`]. Box stand-ins are
//! disabled until a real CC0 model replaces them.
#![allow(dead_code)]

use godot::classes::mesh::PrimitiveType;
use godot::classes::{ArrayMesh, MeshInstance3D, Shader, ShaderMaterial, SurfaceTool};
use godot::obj::Gd;
use godot::prelude::*;
use pp_course::Course;
use pp_world::{LandmarkRequest, PlacedLandmark, SiteKind, horizon_anchor, place_landmarks};

use crate::mesh_util::{add_box, standard_material};
use crate::theme::Theme;
use crate::world::terrain::Field;

const WATER_SHADER: &str = r#"shader_type spatial;
render_mode cull_disabled, blend_mix, depth_draw_opaque, specular_schlick_ggx;
uniform vec3 tint : source_color = vec3(0.55, 0.82, 0.92);
void fragment() {
    float band = fract(UV.y * 3.0 - TIME * 0.35);
    float foam = smoothstep(0.55, 0.95, band);
    ALBEDO = mix(tint, vec3(0.92, 0.96, 1.0), foam);
    ALPHA = mix(0.72, 0.95, foam);
    ROUGHNESS = 0.08;
}
"#;

const BLADE_SHADER: &str = r#"shader_type spatial;
render_mode cull_disabled, diffuse_burley;
void vertex() {
    float angle = TIME * 1.6;
    float c = cos(angle);
    float s = sin(angle);
    vec3 p = VERTEX;
    VERTEX = vec3(p.x * c - p.y * s, p.x * s + p.y * c, p.z);
}
void fragment() {
    ALBEDO = vec3(0.45, 0.48, 0.52);
    ROUGHNESS = 0.45;
}
"#;

const SMOKE_SHADER: &str = r#"shader_type spatial;
render_mode cull_disabled, blend_mix, depth_draw_opaque, shadows_disabled;
void vertex() {
    VERTEX.x += sin(TIME * 0.5 + VERTEX.y * 0.15) * VERTEX.y * 0.08;
    VERTEX.z += cos(TIME * 0.4 + VERTEX.y * 0.12) * VERTEX.y * 0.06;
}
void fragment() {
    float fade = clamp(1.0 - UV.y, 0.0, 1.0);
    ALBEDO = vec3(0.78, 0.78, 0.80);
    ALPHA = 0.28 * fade;
}
"#;

struct Kind {
    id: &'static str,
    footprint_m: f32,
    road_m: f32,
    site: SiteKind,
    rarity: f32,
    spans_road: bool,
}

fn kinds_for(theme: &str) -> Vec<Kind> {
    if theme == "farmland" {
        return vec![
            Kind {
                id: "barn",
                footprint_m: 16.0,
                road_m: 26.0,
                site: SiteKind::Roadside,
                rarity: 1.0,
                spans_road: false,
            },
            Kind {
                id: "silo_row",
                footprint_m: 18.0,
                road_m: 38.0,
                site: SiteKind::Straight,
                rarity: 1.0,
                spans_road: false,
            },
            Kind {
                id: "farmstead",
                footprint_m: 24.0,
                road_m: 44.0,
                site: SiteKind::Straight,
                rarity: 1.0,
                spans_road: false,
            },
            Kind {
                id: "pasture",
                footprint_m: 18.0,
                road_m: 28.0,
                site: SiteKind::Roadside,
                rarity: 1.0,
                spans_road: false,
            },
            Kind {
                id: "windmill",
                footprint_m: 10.0,
                road_m: 20.0,
                site: SiteKind::Roadside,
                rarity: 1.0,
                spans_road: false,
            },
            Kind {
                id: "hay_yard",
                footprint_m: 14.0,
                road_m: 18.0,
                site: SiteKind::Roadside,
                rarity: 1.0,
                spans_road: false,
            },
        ];
    }
    if theme == "desert" {
        return vec![
            Kind {
                id: "ruins",
                footprint_m: 22.0,
                road_m: 70.0,
                site: SiteKind::Vista,
                rarity: 1.0,
                spans_road: false,
            },
            Kind {
                id: "cactus",
                footprint_m: 10.0,
                road_m: 15.0,
                site: SiteKind::Roadside,
                rarity: 1.0,
                spans_road: false,
            },
            Kind {
                id: "cactus_b",
                footprint_m: 10.0,
                road_m: 18.0,
                site: SiteKind::Roadside,
                rarity: 1.0,
                spans_road: false,
            },
            Kind {
                id: "wreck",
                footprint_m: 8.0,
                road_m: 14.0,
                site: SiteKind::Roadside,
                rarity: 1.0,
                spans_road: false,
            },
            Kind {
                id: "tower",
                footprint_m: 14.0,
                road_m: 22.0,
                site: SiteKind::Crest,
                rarity: 1.0,
                spans_road: false,
            },
            Kind {
                id: "arch",
                footprint_m: 8.0,
                road_m: 8.0,
                site: SiteKind::Straight,
                rarity: 1.0,
                spans_road: true,
            },
        ];
    }
    vec![
        Kind {
            id: "village",
            footprint_m: 42.0,
            road_m: 64.0,
            site: SiteKind::Straight,
            rarity: 1.0,
            spans_road: false,
        },
        Kind {
            id: "mill",
            footprint_m: 12.0,
            road_m: 36.0,
            site: SiteKind::Valley,
            rarity: 1.0,
            spans_road: false,
        },
        Kind {
            id: "meadow",
            footprint_m: 15.0,
            road_m: 24.0,
            site: SiteKind::Roadside,
            rarity: 1.0,
            spans_road: false,
        },
        Kind {
            id: "herd",
            footprint_m: 15.0,
            road_m: 24.0,
            site: SiteKind::Roadside,
            rarity: 1.0,
            spans_road: false,
        },
        Kind {
            id: "camp",
            footprint_m: 10.0,
            road_m: 14.0,
            site: SiteKind::Roadside,
            rarity: 1.0,
            spans_road: false,
        },
        Kind {
            id: "sign",
            footprint_m: 4.0,
            road_m: 13.0,
            site: SiteKind::Roadside,
            rarity: 1.0,
            spans_road: false,
        },
        Kind {
            id: "tower",
            footprint_m: 16.0,
            road_m: 22.0,
            site: SiteKind::Crest,
            rarity: 1.0,
            spans_road: false,
        },
        Kind {
            id: "bridge",
            footprint_m: 8.0,
            road_m: 8.0,
            site: SiteKind::Straight,
            rarity: 1.0,
            spans_road: true,
        },
    ]
}

/// Choose sites and level pads into `field` before the ground mesh is built.
pub fn prepare(course: &Course, field: &mut Field, theme: &Theme) -> Vec<PlacedLandmark> {
    let requests: Vec<LandmarkRequest> = kinds_for(&theme.name)
        .into_iter()
        .map(|kind| LandmarkRequest {
            id: kind.id.to_owned(),
            footprint_m: kind.footprint_m,
            road_m: kind.road_m,
            site: kind.site,
            rarity: kind.rarity,
            spans_road: kind.spans_road,
        })
        .collect();
    let mut placed = place_landmarks(course, theme.seed, &requests, field);
    for site in &mut placed {
        if site.id == "waterfall" {
            site.pad = None;
        }
    }
    let pads: Vec<(f32, f32, f32, f32)> = placed.iter().filter_map(|site| site.pad).collect();
    field.set_pads(pads);
    placed
}

/// Build the landmark meshes. Pads are already in `field`.
pub fn spawn(
    parent: &mut Gd<Node3D>,
    _course: &Course,
    _field: &Field,
    _theme: &Theme,
    placed: &[PlacedLandmark],
) {
    let mut group = Node3D::new_alloc();
    group.set_name("Landmarks");
    for (index, site) in placed.iter().enumerate() {
        spawn_one(&mut group, site, index, _theme);
        godot_print!(
            "PENGUIN_PASS_LANDMARK id={} dist={:.0} x={:.1} z={:.1} yaw={:.2}",
            site.id,
            site.distance_m,
            site.x,
            site.z,
            site.yaw
        );
    }
    parent.add_child(&group);
}

fn spawn_one(parent: &mut Gd<Node3D>, site: &PlacedLandmark, index: usize, _theme: &Theme) {
    let mut root = Node3D::new_alloc();
    root.set_name(&format!("Landmark_{}_{index}", site.id));
    root.set_position(Vector3::new(site.x, site.ground_y, site.z));
    root.set_rotation(Vector3::new(0.0, site.yaw, 0.0));
    root.set_meta("reveal_m", &site.reveal_m.to_variant());
    match site.id.as_str() {
        "village" => build_village_models(&mut root),
        "mill" => build_mill(&mut root),
        "statue" => build_statue(&mut root),
        "bridge" | "arch" => build_bridge(&mut root),
        "meadow" | "herd" | "animals" => build_animals(&mut root),
        "camp" => build_camp(&mut root),
        "sign" => prop(
            &mut root,
            "Sign",
            &format!("{NATURE}sign.glb"),
            Vector3::ZERO,
            0.2,
            12.0,
        ),
        "tower" => build_castle(&mut root),
        "waterfall" => build_waterfall_models(&mut root),
        "cactus" | "cactus_b" => build_cacti(&mut root),
        "ruins" | "temple" => build_temple_models(&mut root),
        "ruin" => build_ruin(&mut root),
        "wreck" => prop(&mut root, "Cart", MED_CART, Vector3::ZERO, 1.2, 3.6),
        "barn" => build_barn(&mut root),
        "silo_row" => build_silo_row(&mut root),
        "farmstead" => build_farmstead(&mut root),
        "pasture" => build_pasture(&mut root),
        "windmill" => build_windmill(&mut root),
        "hay_yard" => build_hay_yard(&mut root),
        _ => {}
    }
    parent.add_child(&root);
}

const MED: &str = "res://assets/landmarks/quaternius-medieval/models/";
const ANIMAL: &str = "res://assets/landmarks/quaternius-animals/models/";
const NATURE: &str = "res://assets/landmarks/kenney-nature/models/";
const CASTLE: &str = "res://assets/landmarks/kenney-castle/models/";
const MED_MILL: &str = "res://assets/landmarks/quaternius-medieval/models/mill.gltf";
const MED_CART: &str = "res://assets/landmarks/quaternius-medieval/models/cart.gltf";
const CASTLE_TOWER: &str = "res://assets/landmarks/kenney-castle/models/tower-square.glb";
const STATUE: &str = "res://assets/landmarks/polyhaven-gothic-statue/gothic_statue.gltf";

fn slope_at(field: &Field, x: f32, z: f32) -> f32 {
    let step = 6.0;
    let dx = field.height(x + step, z) - field.height(x - step, z);
    let dz = field.height(x, z + step) - field.height(x, z - step);
    (dx * dx + dz * dz).sqrt() / (2.0 * step)
}

fn prop(parent: &mut Gd<Node3D>, name: &str, path: &str, position: Vector3, yaw: f32, scale: f32) {
    let Some(mesh) = load_model(path) else {
        godot_error!("Penguin Pass: landmark model missing {path}");
        return;
    };
    let mut node = MeshInstance3D::new_alloc();
    node.set_name(name);
    node.set_mesh(&mesh);
    node.set_position(position);
    node.set_rotation(Vector3::new(0.0, yaw, 0.0));
    node.set_scale(Vector3::new(scale, scale, scale));
    parent.add_child(&node);
}

fn fence_panel_step(scale: f32) -> f32 {
    scale * 0.76
}

fn build_fence_run(
    root: &mut Gd<Node3D>,
    prefix: &str,
    start: Vector3,
    dir_x: f32,
    dir_z: f32,
    count: usize,
    scale: f32,
) {
    let step = fence_panel_step(scale);
    let len = (dir_x * dir_x + dir_z * dir_z).sqrt();
    let (dx, dz) = if len > f32::EPSILON {
        (dir_x / len, dir_z / len)
    } else {
        (1.0, 0.0)
    };
    let yaw = (-dz).atan2(dx);
    let path = format!("{MED}fence.gltf");
    for i in 0..count {
        let along = step * (i as f32 + 0.5);
        let pos = Vector3::new(start.x + dx * along, start.y, start.z + dz * along);
        prop(root, &format!("{prefix}{i}"), &path, pos, yaw, scale);
    }
}

fn build_fenced_pen(
    root: &mut Gd<Node3D>,
    prefix: &str,
    cols: usize,
    rows: usize,
    scale: f32,
    gate_col: Option<usize>,
) {
    let step = fence_panel_step(scale);
    let w = cols as f32 * step;
    let d = rows as f32 * step;
    let half_w = w * 0.5;
    let half_d = d * 0.5;

    let south = Vector3::new(-half_w, 0.0, half_d);
    if let Some(gate) = gate_col {
        if gate > 0 {
            build_fence_run(root, &format!("{prefix}S"), south, 1.0, 0.0, gate, scale);
        }
        if gate + 1 < cols {
            let skip = step * (gate as f32 + 1.0);
            build_fence_run(
                root,
                &format!("{prefix}Sg"),
                Vector3::new(south.x + skip, south.y, south.z),
                1.0,
                0.0,
                cols - gate - 1,
                scale,
            );
        }
    } else {
        build_fence_run(root, &format!("{prefix}S"), south, 1.0, 0.0, cols, scale);
    }
    build_fence_run(
        root,
        &format!("{prefix}N"),
        Vector3::new(-half_w, 0.0, -half_d),
        1.0,
        0.0,
        cols,
        scale,
    );
    build_fence_run(
        root,
        &format!("{prefix}W"),
        Vector3::new(-half_w, 0.0, -half_d),
        0.0,
        1.0,
        rows,
        scale,
    );
    build_fence_run(
        root,
        &format!("{prefix}E"),
        Vector3::new(half_w, 0.0, -half_d),
        0.0,
        1.0,
        rows,
        scale,
    );
}

fn load_model(path: &str) -> Option<Gd<godot::classes::Mesh>> {
    let mut document = godot::classes::GltfDocument::new_gd();
    let state = godot::classes::GltfState::new_gd();
    if document.append_from_file(path, &state) != godot::global::Error::OK {
        return None;
    }
    let scene = document.generate_scene(&state)?;
    let mesh = find_first_mesh(&scene);
    scene.free();
    mesh
}

fn find_first_mesh(node: &Gd<Node>) -> Option<Gd<godot::classes::Mesh>> {
    if let Ok(mesh_node) = node.clone().try_cast::<MeshInstance3D>()
        && let Some(mesh) = mesh_node.get_mesh()
    {
        return Some(mesh);
    }
    for child in node.get_children().iter_shared() {
        if let Some(mesh) = find_first_mesh(&child) {
            return Some(mesh);
        }
    }
    None
}

fn build_village_models(root: &mut Gd<Node3D>) {
    // Twelve houses along the lane, then the bell tower as the tall point.
    let houses = [
        "house_1",
        "house_2",
        "house_3",
        "house_4",
        "inn",
        "stable",
        "blacksmith",
        "sawmill",
    ];
    for index in 0..12 {
        let name = houses[index % houses.len()];
        let side = if index % 2 == 0 { -1.0 } else { 1.0 };
        let z = (index / 2) as f32 * 7.2 - 16.0;
        let yaw = if side < 0.0 { 1.35 } else { -1.35 } + (index as f32 * 0.08);
        let scale = 1.75 + (index % 4) as f32 * 0.12;
        prop(
            root,
            &format!("{name}{index}"),
            &format!("{MED}{name}.gltf"),
            Vector3::new(side * 8.0, 0.0, z),
            yaw,
            scale,
        );
    }
    prop(
        root,
        "Church",
        &format!("{MED}bell_tower.gltf"),
        Vector3::new(0.0, 0.0, 28.0),
        0.15,
        3.1,
    );
    prop(
        root,
        "Well",
        &format!("{MED}well.gltf"),
        Vector3::new(0.0, 0.0, 4.0),
        0.4,
        2.4,
    );
    const VILLAGE_FENCE: f32 = 3.8;
    build_fence_run(
        root,
        "FenceL",
        Vector3::new(-3.2, 0.0, -18.0),
        0.0,
        1.0,
        12,
        VILLAGE_FENCE,
    );
    build_fence_run(
        root,
        "FenceR",
        Vector3::new(3.2, 0.0, -18.0),
        0.0,
        1.0,
        12,
        VILLAGE_FENCE,
    );
    for i in 0..12 {
        let z = -18.0 + (i as f32 + 0.5) * fence_panel_step(VILLAGE_FENCE);
        if i % 2 == 0 {
            prop(
                root,
                &format!("Hay{i}"),
                &format!("{MED}hay.gltf"),
                Vector3::new(-5.5, 0.0, z + 1.5),
                0.3,
                10.0,
            );
            prop(
                root,
                &format!("Barrel{i}"),
                &format!("{MED}barrel.gltf"),
                Vector3::new(5.2, 0.0, z),
                0.8,
                8.0,
            );
        }
    }
}

fn build_statue(root: &mut Gd<Node3D>) {
    let scale = 6.0;
    prop(
        root,
        "Plinth",
        &format!("{NATURE}statue_column.glb"),
        Vector3::ZERO,
        0.2,
        2.2,
    );
    // Mesh origin sits about 1.21 m above the feet. Scale makes a figure ~8 m tall.
    prop(
        root,
        "Figure",
        STATUE,
        Vector3::new(0.0, 1.21 * scale, 0.0),
        0.4,
        scale,
    );
}

fn build_bridge(root: &mut Gd<Node3D>) {
    prop(
        root,
        "Gate",
        &format!("{CASTLE}gate.glb"),
        Vector3::ZERO,
        0.0,
        5.5,
    );
    for (index, x) in [-6.0_f32, 6.0].iter().enumerate() {
        prop(
            root,
            &format!("Pier{index}"),
            CASTLE_TOWER,
            Vector3::new(*x, 0.0, 0.0),
            0.0,
            2.4,
        );
    }
}

fn build_animals(root: &mut Gd<Node3D>) {
    const PEN_SCALE: f32 = 3.8;
    build_fenced_pen(root, "Pen", 7, 5, PEN_SCALE, Some(3));

    prop(
        root,
        "CowA",
        &format!("{ANIMAL}cow.gltf"),
        Vector3::new(-5.2, 0.0, -2.5),
        0.5,
        0.27,
    );
    prop(
        root,
        "CowB",
        &format!("{ANIMAL}cow.gltf"),
        Vector3::new(4.6, 0.0, 2.2),
        2.1,
        0.26,
    );
    prop(
        root,
        "Sheep0",
        &format!("{ANIMAL}sheep.gltf"),
        Vector3::new(-1.5, 0.0, 3.0),
        0.4,
        0.21,
    );
    prop(
        root,
        "Sheep1",
        &format!("{ANIMAL}sheep.gltf"),
        Vector3::new(1.2, 0.0, 3.8),
        1.1,
        0.20,
    );
    prop(
        root,
        "Sheep2",
        &format!("{ANIMAL}sheep.gltf"),
        Vector3::new(-0.4, 0.0, 1.4),
        0.7,
        0.22,
    );
    prop(
        root,
        "Hay",
        &format!("{MED}hay.gltf"),
        Vector3::new(-7.5, 0.0, 4.5),
        0.3,
        5.0,
    );
    prop(
        root,
        "Trough",
        &format!("{MED}well.gltf"),
        Vector3::new(7.2, 0.0, -4.2),
        1.2,
        1.4,
    );
}

fn build_mill(root: &mut Gd<Node3D>) {
    // The Quaternius mill's hub points toward +X/+Z. Yaw 2.24 turns that
    // face toward the road (local -Z). The sails are not in the mesh.
    let scale = 2.5;
    prop(root, "Mill", MED_MILL, Vector3::ZERO, 2.24, scale);
    let hub = Vector3::new(-0.05, 3.05, -2.14) * scale;
    let mut sails = sail_mesh();
    sails.set_name("Sails");
    sails.set_position(hub);
    root.add_child(&sails);
}

const SAIL_SHADER: &str = r#"shader_type spatial;
render_mode cull_disabled, diffuse_burley;
uniform vec3 tint = vec3(0.45, 0.24, 0.12);
void vertex() {
    float a = TIME * 0.4;
    float c = cos(a);
    float s = sin(a);
    vec2 p = VERTEX.xy;
    VERTEX.x = c * p.x - s * p.y;
    VERTEX.y = s * p.x + c * p.y;
}
void fragment() {
    ALBEDO = tint;
    ROUGHNESS = 0.85;
}
"#;

fn sail_mesh() -> Gd<MeshInstance3D> {
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    for blade in 0..4 {
        let angle = blade as f32 * std::f32::consts::FRAC_PI_2;
        let dir = Vector3::new(angle.cos(), angle.sin(), 0.0);
        let side = Vector3::new(-angle.sin(), angle.cos(), 0.0) * 0.45;
        let tip = dir * 7.6;
        let mid = dir * 3.6;
        push_tri(&mut tool, side * 0.3, tip + side, tip - side);
        push_tri(&mut tool, side * 0.3, tip - side, -side * 0.3);
        push_tri(
            &mut tool,
            mid + side * 1.3,
            mid + dir * 0.8,
            mid - side * 0.2,
        );
    }
    let mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut shader = Shader::new_gd();
    shader.set_code(SAIL_SHADER);
    let mut material = ShaderMaterial::new_gd();
    material.set_shader(&shader);
    let mut node = MeshInstance3D::new_alloc();
    node.set_mesh(&mesh);
    node.set_surface_override_material(0, &material);
    node
}

fn build_camp(root: &mut Gd<Node3D>) {
    prop(
        root,
        "TentA",
        &format!("{NATURE}tent_detailedOpen.glb"),
        Vector3::new(-3.2, 0.0, 0.6),
        0.5,
        4.4,
    );
    prop(
        root,
        "TentB",
        &format!("{NATURE}tent_smallOpen.glb"),
        Vector3::new(3.4, 0.0, -0.4),
        -0.7,
        4.0,
    );
    prop(
        root,
        "Fire",
        &format!("{NATURE}campfire_logs.glb"),
        Vector3::ZERO,
        0.2,
        2.8,
    );
    prop(
        root,
        "Logs",
        &format!("{NATURE}log_stack.glb"),
        Vector3::new(2.4, 0.0, 1.6),
        0.8,
        2.0,
    );
    let mut light = godot::classes::OmniLight3D::new_alloc();
    light.set_name("FireLight");
    light.set_position(Vector3::new(0.0, 1.2, 0.0));
    light.set_param(godot::classes::light_3d::Param::ENERGY, 2.4);
    light.set_param(godot::classes::light_3d::Param::RANGE, 14.0);
    light.set_color(Color::from_rgb(1.0, 0.62, 0.28));
    root.add_child(&light);
}

fn build_cacti(root: &mut Gd<Node3D>) {
    for (i, (x, z)) in [
        (-3.0, 0.0),
        (1.5, 2.0),
        (4.0, -1.0),
        (-1.0, 4.0),
        (3.0, 5.0),
    ]
    .iter()
    .enumerate()
    {
        let model = if i % 2 == 0 {
            "cactus_tall.glb"
        } else {
            "cactus_short.glb"
        };
        prop(
            root,
            &format!("Cactus{i}"),
            &format!("{NATURE}{model}"),
            Vector3::new(*x, 0.0, *z),
            i as f32 * 0.4,
            5.2,
        );
    }
}

fn build_ruin(root: &mut Gd<Node3D>) {
    for (index, (x, z, tilt)) in [
        (-6.0, 0.0, 0.0),
        (-2.0, 2.0, 0.9),
        (3.0, -1.0, 0.2),
        (6.5, 2.5, 1.4),
    ]
    .iter()
    .enumerate()
    {
        let mut column = MeshInstance3D::new_alloc();
        if let Some(mesh) = load_model(&format!("{NATURE}statue_column.glb")) {
            column.set_mesh(&mesh);
        }
        column.set_name(&format!("Column{index}"));
        column.set_position(Vector3::new(*x, 0.0, *z));
        column.set_rotation(Vector3::new(*tilt, index as f32 * 0.4, 0.0));
        column.set_scale(Vector3::new(3.2, 3.2, 3.2));
        root.add_child(&column);
    }
    prop(
        root,
        "Fallen",
        &format!("{CASTLE}wall-narrow.glb"),
        Vector3::new(1.0, 0.4, 4.0),
        0.6,
        2.4,
    );
}

fn build_castle(root: &mut Gd<Node3D>) {
    prop(
        root,
        "Keep",
        CASTLE_TOWER,
        Vector3::new(0.0, 0.0, 2.0),
        0.2,
        9.0,
    );
    prop(
        root,
        "FlankL",
        CASTLE_TOWER,
        Vector3::new(-8.0, 0.0, -2.0),
        0.4,
        4.2,
    );
    prop(
        root,
        "FlankR",
        &format!("{CASTLE}tower-base.glb"),
        Vector3::new(7.0, 0.0, -1.0),
        1.1,
        3.6,
    );
    prop(
        root,
        "Wall",
        &format!("{CASTLE}wall-narrow.glb"),
        Vector3::new(0.0, 0.0, -4.0),
        0.1,
        3.4,
    );
    prop(
        root,
        "Gate",
        &format!("{CASTLE}gate.glb"),
        Vector3::new(-2.0, 0.0, -5.0),
        0.3,
        2.8,
    );
    prop(
        root,
        "Rubble",
        &format!("{CASTLE}rocks-large.glb"),
        Vector3::new(5.0, 0.0, 3.0),
        0.8,
        2.2,
    );
}

fn build_temple_models(root: &mut Gd<Node3D>) {
    for row in 0..2 {
        for index in 0..5 {
            let fallen = row == 1 && index == 3;
            prop(
                root,
                &format!("Column{row}{index}"),
                &format!("{NATURE}statue_column.glb"),
                Vector3::new(
                    -10.0 + index as f32 * 5.0,
                    if fallen { 0.4 } else { 0.0 },
                    row as f32 * 8.0,
                ),
                if fallen { 1.4 } else { 0.1 * index as f32 },
                if fallen { 4.2 } else { 5.5 },
            );
        }
    }
    prop(
        root,
        "Arch",
        &format!("{CASTLE}gate.glb"),
        Vector3::new(0.0, 0.0, -4.0),
        0.0,
        4.5,
    );
    prop(
        root,
        "WallL",
        &format!("{CASTLE}wall-narrow.glb"),
        Vector3::new(-8.0, 0.0, 12.0),
        0.2,
        3.2,
    );
    prop(
        root,
        "WallR",
        &format!("{CASTLE}wall-narrow.glb"),
        Vector3::new(8.0, 0.0, 12.0),
        -0.3,
        2.6,
    );
    prop(
        root,
        "Steps",
        &format!("{MED}path_straight.gltf"),
        Vector3::new(0.0, 0.0, -8.0),
        0.0,
        3.0,
    );
}

fn build_waterfall_models(root: &mut Gd<Node3D>) {
    prop(
        root,
        "CliffA",
        &format!("{NATURE}cliff_waterfall_rock.glb"),
        Vector3::new(0.0, 0.0, 1.0),
        0.0,
        8.0,
    );
    prop(
        root,
        "CliffB",
        &format!("{NATURE}cliff_waterfallTop_rock.glb"),
        Vector3::new(0.2, 6.5, 1.4),
        0.1,
        7.0,
    );
    prop(
        root,
        "CliffC",
        &format!("{NATURE}cliff_large_rock.glb"),
        Vector3::new(-3.5, 2.0, 0.4),
        0.5,
        6.0,
    );
    prop(
        root,
        "CliffD",
        &format!("{NATURE}rock_largeA.glb"),
        Vector3::new(3.2, 0.0, 0.2),
        0.8,
        5.5,
    );
    for (index, offset) in [0.0_f32, 0.35, -0.28].iter().enumerate() {
        let mut sheet = quad(2.4, 14.0, water_material(Color::from_rgb(0.75, 0.92, 1.0)));
        sheet.set_name(&format!("Falls{index}"));
        sheet.set_position(Vector3::new(*offset, 7.0, 2.4));
        root.add_child(&sheet);
    }
    let mut foam = quad(6.0, 2.2, water_material(Color::from_rgb(0.92, 0.96, 1.0)));
    foam.set_name("Foam");
    foam.set_position(Vector3::new(0.0, 0.8, 3.6));
    root.add_child(&foam);
    let mut pool = quad(10.0, 7.0, water_material(Color::from_rgb(0.22, 0.48, 0.62)));
    pool.set_name("Pool");
    pool.set_rotation(Vector3::new(-std::f32::consts::FRAC_PI_2, 0.0, 0.0));
    pool.set_position(Vector3::new(0.0, 0.12, 6.0));
    root.add_child(&pool);
    let mut mist = quad(4.0, 3.0, smoke_material());
    mist.set_name("Mist");
    mist.set_position(Vector3::new(0.2, 1.6, 4.2));
    root.add_child(&mist);
    prop(
        root,
        "Culvert",
        &format!("{NATURE}bridge_wood.glb"),
        Vector3::new(0.0, 0.2, -6.0),
        1.57,
        4.0,
    );
}

fn build_waterfall(root: &mut Gd<Node3D>) {
    let rock = standard_material(Color::from_rgb(0.45, 0.44, 0.42), 0.9, 0.0);
    add_box(
        root,
        "CliffL",
        Vector3::new(8.0, 22.0, 6.0),
        Vector3::new(-7.0, 11.0, 0.0),
        &rock,
    );
    add_box(
        root,
        "CliffR",
        Vector3::new(8.0, 18.0, 6.0),
        Vector3::new(7.0, 9.0, 0.0),
        &rock,
    );
    add_box(
        root,
        "Ledge",
        Vector3::new(18.0, 3.0, 7.0),
        Vector3::new(0.0, 20.0, -1.0),
        &rock,
    );
    let mut sheet = quad(4.2, 18.0, water_material(Color::from_rgb(0.75, 0.92, 1.0)));
    sheet.set_name("Water");
    sheet.set_position(Vector3::new(0.0, 10.0, 1.2));
    root.add_child(&sheet);
    let mut mist = standard_material(Color::from_rgba(0.9, 0.95, 1.0, 0.45), 1.0, 0.0);
    mist.set_transparency(godot::classes::base_material_3d::Transparency::ALPHA);
    add_box(
        root,
        "Mist",
        Vector3::new(7.0, 3.0, 5.0),
        Vector3::new(0.0, 1.6, 2.0),
        &mist,
    );
}

fn build_village(root: &mut Gd<Node3D>) {
    let wall = standard_material(Color::from_rgb(0.72, 0.55, 0.42), 0.85, 0.0);
    let roof = standard_material(Color::from_rgb(0.45, 0.18, 0.14), 0.7, 0.0);
    let church = standard_material(Color::from_rgb(0.85, 0.84, 0.80), 0.8, 0.0);
    let spots = [
        (-10.0, 4.0),
        (-2.0, 8.0),
        (8.0, 3.0),
        (14.0, 9.0),
        (-8.0, 12.0),
    ];
    for (index, (x, z)) in spots.iter().enumerate() {
        add_box(
            root,
            &format!("House{index}"),
            Vector3::new(6.0, 4.0, 5.0),
            Vector3::new(*x, 2.0, *z),
            &wall,
        );
        add_box(
            root,
            &format!("Roof{index}"),
            Vector3::new(7.0, 2.2, 6.0),
            Vector3::new(*x, 4.8, *z),
            &roof,
        );
    }
    add_box(
        root,
        "Church",
        Vector3::new(7.0, 8.0, 10.0),
        Vector3::new(2.0, 4.0, 16.0),
        &church,
    );
    add_box(
        root,
        "Tower",
        Vector3::new(3.2, 14.0, 3.2),
        Vector3::new(2.0, 11.0, 20.0),
        &church,
    );
    add_box(
        root,
        "Spire",
        Vector3::new(0.6, 6.0, 0.6),
        Vector3::new(2.0, 20.0, 20.0),
        &roof,
    );
}

fn build_cross(root: &mut Gd<Node3D>) {
    let stone = standard_material(Color::from_rgb(0.55, 0.54, 0.52), 0.85, 0.0);
    let wood = standard_material(Color::from_rgb(0.35, 0.24, 0.16), 0.8, 0.0);
    let flag = standard_material(Color::from_rgb(0.75, 0.15, 0.18), 0.6, 0.0);
    add_box(
        root,
        "Base",
        Vector3::new(4.0, 1.2, 3.0),
        Vector3::new(0.0, 0.6, 0.0),
        &stone,
    );
    add_box(
        root,
        "Post",
        Vector3::new(0.7, 14.0, 0.7),
        Vector3::new(0.0, 8.0, 0.0),
        &wood,
    );
    add_box(
        root,
        "Arm",
        Vector3::new(5.2, 0.55, 0.55),
        Vector3::new(0.0, 12.6, 0.0),
        &wood,
    );
    add_box(
        root,
        "Flag",
        Vector3::new(1.8, 1.1, 0.08),
        Vector3::new(1.2, 8.6, 0.0),
        &flag,
    );
    add_box(
        root,
        "Bench",
        Vector3::new(2.4, 0.5, 0.6),
        Vector3::new(2.6, 0.55, 1.5),
        &wood,
    );
}

fn build_turbines(root: &mut Gd<Node3D>) {
    let mast = standard_material(Color::from_rgb(0.82, 0.84, 0.86), 0.4, 0.2);
    for (index, x) in [-16.0_f32, 0.0, 18.0].iter().enumerate() {
        add_box(
            root,
            &format!("Mast{index}"),
            Vector3::new(1.1, 28.0, 1.1),
            Vector3::new(*x, 14.0, 0.0),
            &mast,
        );
        add_box(
            root,
            &format!("Nacelle{index}"),
            Vector3::new(1.4, 1.2, 3.2),
            Vector3::new(*x, 28.4, 0.6),
            &mast,
        );
        let mut blades = blade_mesh();
        blades.set_name(&format!("Blades{index}"));
        blades.set_position(Vector3::new(*x, 28.4, 2.2));
        root.add_child(&blades);
    }
}

fn build_lake(root: &mut Gd<Node3D>) {
    let mut water = quad(
        48.0,
        32.0,
        water_material(Color::from_rgb(0.25, 0.55, 0.72)),
    );
    water.set_name("Lake");
    water.set_rotation(Vector3::new(-std::f32::consts::FRAC_PI_2, 0.0, 0.0));
    water.set_position(Vector3::new(0.0, 0.15, 6.0));
    root.add_child(&water);
    let reed = standard_material(Color::from_rgb(0.35, 0.48, 0.22), 0.9, 0.0);
    for index in 0..5 {
        let x = -10.0 + index as f32 * 5.0;
        add_box(
            root,
            &format!("Reed{index}"),
            Vector3::new(0.3, 1.6, 0.3),
            Vector3::new(x, 0.8, -2.0),
            &reed,
        );
    }
}

fn build_pylons(root: &mut Gd<Node3D>) {
    let steel = standard_material(Color::from_rgb(0.55, 0.58, 0.62), 0.45, 0.35);
    for (index, z) in [0.0_f32, 28.0, 60.0].iter().enumerate() {
        let height = 16.0 - index as f32 * 1.5;
        add_box(
            root,
            &format!("Pylon{index}"),
            Vector3::new(1.4, height, 1.4),
            Vector3::new(0.0, height * 0.5, *z),
            &steel,
        );
        add_box(
            root,
            &format!("Arm{index}"),
            Vector3::new(6.0, 0.35, 0.35),
            Vector3::new(0.0, height - 1.0, *z),
            &steel,
        );
    }
    add_box(
        root,
        "Cable",
        Vector3::new(0.15, 0.15, 62.0),
        Vector3::new(0.0, 12.0, 30.0),
        &steel,
    );
}

fn build_herd(root: &mut Gd<Node3D>, _desert: bool) {
    let wool = standard_material(Color::from_rgb(0.9, 0.9, 0.86), 0.95, 0.0);
    let face = standard_material(Color::from_rgb(0.25, 0.2, 0.16), 0.8, 0.0);
    for index in 0..7 {
        let x = (index % 4) as f32 * 3.2 - 4.0;
        let z = (index / 4) as f32 * 3.5;
        add_box(
            root,
            &format!("Body{index}"),
            Vector3::new(1.5, 1.0, 2.1),
            Vector3::new(x, 0.9, z),
            &wool,
        );
        add_box(
            root,
            &format!("Head{index}"),
            Vector3::new(0.55, 0.55, 0.6),
            Vector3::new(x, 1.3, z + 1.1),
            &face,
        );
    }
}

fn build_arch(root: &mut Gd<Node3D>, desert: bool) {
    let color = if desert {
        Color::from_rgb(0.72, 0.48, 0.32)
    } else {
        Color::from_rgb(0.62, 0.62, 0.60)
    };
    let stone = standard_material(color, 0.9, 0.0);
    add_box(
        root,
        "Left",
        Vector3::new(1.4, 6.5, 1.6),
        Vector3::new(-6.4, 3.25, 0.0),
        &stone,
    );
    add_box(
        root,
        "Right",
        Vector3::new(1.4, 6.5, 1.6),
        Vector3::new(6.4, 3.25, 0.0),
        &stone,
    );
    add_box(
        root,
        "Beam",
        Vector3::new(14.5, 1.3, 2.2),
        Vector3::new(0.0, 6.8, 0.0),
        &stone,
    );
}

fn build_temple(root: &mut Gd<Node3D>) {
    let sand = standard_material(Color::from_rgb(0.78, 0.62, 0.40), 0.9, 0.0);
    add_box(
        root,
        "Step1",
        Vector3::new(22.0, 2.0, 22.0),
        Vector3::new(0.0, 1.0, 8.0),
        &sand,
    );
    add_box(
        root,
        "Step2",
        Vector3::new(14.0, 2.2, 14.0),
        Vector3::new(0.0, 3.0, 8.0),
        &sand,
    );
    add_box(
        root,
        "Cella",
        Vector3::new(8.0, 6.0, 8.0),
        Vector3::new(0.0, 7.0, 8.0),
        &sand,
    );
    for (index, x) in [-6.0_f32, 6.0].iter().enumerate() {
        add_box(
            root,
            &format!("Column{index}"),
            Vector3::new(1.2, 7.0, 1.2),
            Vector3::new(*x, 5.5, 0.0),
            &sand,
        );
    }
}

fn build_oasis(root: &mut Gd<Node3D>) {
    let mut water = quad(
        16.0,
        12.0,
        water_material(Color::from_rgb(0.20, 0.55, 0.62)),
    );
    water.set_name("Pool");
    water.set_rotation(Vector3::new(-std::f32::consts::FRAC_PI_2, 0.0, 0.0));
    water.set_position(Vector3::new(0.0, 0.1, 4.0));
    root.add_child(&water);
    let trunk = standard_material(Color::from_rgb(0.45, 0.30, 0.16), 0.85, 0.0);
    let frond = standard_material(Color::from_rgb(0.20, 0.55, 0.18), 0.8, 0.0);
    for (index, (x, z)) in [(-6.0, 2.0), (5.0, 1.0), (-2.0, 8.0), (3.0, 9.0), (7.0, 6.0)]
        .iter()
        .enumerate()
    {
        add_box(
            root,
            &format!("Palm{index}"),
            Vector3::new(0.5, 7.0, 0.5),
            Vector3::new(*x, 3.5, *z),
            &trunk,
        );
        add_box(
            root,
            &format!("Frond{index}"),
            Vector3::new(3.5, 1.2, 3.5),
            Vector3::new(*x, 7.4, *z),
            &frond,
        );
    }
}

fn build_dish(root: &mut Gd<Node3D>) {
    let metal = standard_material(Color::from_rgb(0.75, 0.78, 0.82), 0.35, 0.55);
    let hut = standard_material(Color::from_rgb(0.55, 0.50, 0.42), 0.9, 0.0);
    add_box(
        root,
        "Pole",
        Vector3::new(0.6, 10.0, 0.6),
        Vector3::new(0.0, 5.0, 0.0),
        &metal,
    );
    add_box(
        root,
        "Dish",
        Vector3::new(8.0, 0.4, 8.0),
        Vector3::new(0.0, 10.5, 1.5),
        &metal,
    );
    add_box(
        root,
        "Hut",
        Vector3::new(4.0, 2.6, 3.5),
        Vector3::new(5.0, 1.3, -2.0),
        &hut,
    );
}

fn build_wreck(root: &mut Gd<Node3D>) {
    let rust = standard_material(Color::from_rgb(0.45, 0.28, 0.18), 0.75, 0.25);
    let dark = standard_material(Color::from_rgb(0.15, 0.15, 0.16), 0.6, 0.1);
    add_box(
        root,
        "Cab",
        Vector3::new(2.2, 2.2, 2.4),
        Vector3::new(0.4, 1.6, 1.2),
        &rust,
    );
    add_box(
        root,
        "Bed",
        Vector3::new(2.4, 1.2, 4.2),
        Vector3::new(-0.2, 1.1, -2.0),
        &rust,
    );
    add_box(
        root,
        "Wheel",
        Vector3::new(0.5, 1.1, 1.1),
        Vector3::new(1.3, 0.6, -1.0),
        &dark,
    );
}

fn build_cactus(root: &mut Gd<Node3D>) {
    let green = standard_material(Color::from_rgb(0.25, 0.55, 0.22), 0.8, 0.0);
    for (index, (x, z, h)) in [
        (-4.0, 0.0, 4.5),
        (0.0, 3.0, 6.0),
        (4.5, 1.0, 3.5),
        (2.0, 6.0, 5.0),
        (-2.0, 7.0, 3.0),
    ]
    .iter()
    .enumerate()
    {
        add_box(
            root,
            &format!("Cactus{index}"),
            Vector3::new(0.8, *h, 0.8),
            Vector3::new(*x, h * 0.5, *z),
            &green,
        );
        add_box(
            root,
            &format!("Arm{index}"),
            Vector3::new(1.6, 0.45, 0.45),
            Vector3::new(x + 0.8, h * 0.65, *z),
            &green,
        );
    }
}

fn build_barn(root: &mut Gd<Node3D>) {
    let red = standard_material(Color::from_rgb(0.55, 0.14, 0.11), 0.82, 0.0);
    let trim = standard_material(Color::from_rgb(0.92, 0.91, 0.88), 0.65, 0.0);
    let roof = standard_material(Color::from_rgb(0.38, 0.36, 0.34), 0.75, 0.0);
    let silo = standard_material(Color::from_rgb(0.62, 0.64, 0.66), 0.35, 0.45);
    add_box(
        root,
        "Walls",
        Vector3::new(12.0, 5.5, 8.0),
        Vector3::new(-2.0, 2.75, 0.0),
        &red,
    );
    add_box(
        root,
        "RoofLo",
        Vector3::new(13.0, 1.2, 9.0),
        Vector3::new(-2.0, 6.2, 0.0),
        &roof,
    );
    add_box(
        root,
        "RoofHi",
        Vector3::new(9.0, 1.4, 7.0),
        Vector3::new(-2.0, 7.0, 0.0),
        &roof,
    );
    add_box(
        root,
        "DoorL",
        Vector3::new(0.35, 3.6, 0.18),
        Vector3::new(4.0, 1.8, 4.1),
        &trim,
    );
    add_box(
        root,
        "DoorR",
        Vector3::new(0.35, 3.6, 0.18),
        Vector3::new(3.2, 1.8, 4.1),
        &trim,
    );
    add_box(
        root,
        "Loft",
        Vector3::new(1.4, 1.0, 0.12),
        Vector3::new(-2.0, 4.6, 4.1),
        &trim,
    );
    add_box(
        root,
        "Silo",
        Vector3::new(3.2, 14.0, 3.2),
        Vector3::new(8.0, 7.0, -1.0),
        &silo,
    );
    add_box(
        root,
        "SiloCap",
        Vector3::new(3.6, 1.2, 3.6),
        Vector3::new(8.0, 14.8, -1.0),
        &silo,
    );
    build_fence_run(
        root,
        "Fence",
        Vector3::new(-8.0, 0.0, -6.0),
        0.0,
        1.0,
        8,
        3.8,
    );
    prop(
        root,
        "Hay",
        &format!("{MED}hay.gltf"),
        Vector3::new(-5.5, 0.0, 5.0),
        0.3,
        11.0,
    );
    prop(
        root,
        "Cart",
        MED_CART,
        Vector3::new(-7.0, 0.0, 2.0),
        0.6,
        1.4,
    );
}

fn build_silo_row(root: &mut Gd<Node3D>) {
    let silo = standard_material(Color::from_rgb(0.58, 0.60, 0.63), 0.32, 0.5);
    let timber = standard_material(Color::from_rgb(0.45, 0.32, 0.22), 0.85, 0.0);
    for (name, x) in [("SiloL", -5.5_f32), ("SiloR", 5.5)] {
        add_box(
            root,
            name,
            Vector3::new(3.8, 15.5, 3.8),
            Vector3::new(x, 7.75, 2.0),
            &silo,
        );
        add_box(
            root,
            &format!("{name}Cap"),
            Vector3::new(4.2, 1.4, 4.2),
            Vector3::new(x, 15.8, 2.0),
            &silo,
        );
    }
    add_box(
        root,
        "Bridge",
        Vector3::new(12.0, 1.0, 2.2),
        Vector3::new(0.0, 12.5, 2.0),
        &timber,
    );
    prop(
        root,
        "Loader",
        &format!("{MED}stable.gltf"),
        Vector3::new(0.0, 0.0, -4.0),
        0.1,
        2.0,
    );
    prop(
        root,
        "HayA",
        &format!("{MED}hay.gltf"),
        Vector3::new(-3.0, 0.0, -3.5),
        0.2,
        10.0,
    );
    prop(
        root,
        "HayB",
        &format!("{MED}hay.gltf"),
        Vector3::new(3.5, 0.0, -3.8),
        0.5,
        9.0,
    );
    prop(
        root,
        "Cart",
        MED_CART,
        Vector3::new(7.5, 0.0, -2.0),
        1.1,
        1.2,
    );
}

fn build_farmstead(root: &mut Gd<Node3D>) {
    prop(
        root,
        "HouseA",
        &format!("{MED}house_1.gltf"),
        Vector3::new(-6.0, 0.0, 4.0),
        0.3,
        2.2,
    );
    prop(
        root,
        "HouseB",
        &format!("{MED}house_3.gltf"),
        Vector3::new(5.0, 0.0, 3.0),
        -0.4,
        2.0,
    );
    prop(
        root,
        "Stable",
        &format!("{MED}stable.gltf"),
        Vector3::new(-1.0, 0.0, -5.0),
        0.0,
        2.1,
    );
    prop(
        root,
        "Mill",
        MED_MILL,
        Vector3::new(8.0, 0.0, -2.0),
        2.0,
        2.0,
    );
    let mut sails = sail_mesh();
    sails.set_position(Vector3::new(6.0, 6.1, -4.3));
    root.add_child(&sails);
    prop(
        root,
        "Well",
        &format!("{MED}well.gltf"),
        Vector3::new(1.5, 0.0, 1.0),
        0.5,
        2.2,
    );
    build_fence_run(
        root,
        "Fence",
        Vector3::new(0.0, 0.0, -8.0),
        0.0,
        1.0,
        10,
        3.8,
    );
    prop(
        root,
        "Hay",
        &format!("{MED}hay.gltf"),
        Vector3::new(-9.0, 0.0, -1.0),
        0.2,
        9.5,
    );
    prop(
        root,
        "Barrel",
        &format!("{MED}barrel.gltf"),
        Vector3::new(3.0, 0.0, 6.0),
        0.7,
        7.0,
    );
}

fn build_pasture(root: &mut Gd<Node3D>) {
    const PASTURE_SCALE: f32 = 3.8;
    build_fenced_pen(root, "Pasture", 9, 6, PASTURE_SCALE, Some(4));

    prop(
        root,
        "CowA",
        &format!("{ANIMAL}cow.gltf"),
        Vector3::new(-7.5, 0.0, -3.5),
        0.6,
        0.28,
    );
    prop(
        root,
        "CowB",
        &format!("{ANIMAL}cow.gltf"),
        Vector3::new(6.5, 0.0, -2.0),
        1.4,
        0.27,
    );
    prop(
        root,
        "CowC",
        &format!("{ANIMAL}cow.gltf"),
        Vector3::new(-1.0, 0.0, 4.2),
        2.0,
        0.26,
    );
    prop(
        root,
        "Sheep0",
        &format!("{ANIMAL}sheep.gltf"),
        Vector3::new(4.2, 0.0, 4.5),
        0.5,
        0.21,
    );
    prop(
        root,
        "Sheep1",
        &format!("{ANIMAL}sheep.gltf"),
        Vector3::new(7.0, 0.0, 3.2),
        1.2,
        0.20,
    );
    prop(
        root,
        "Sheep2",
        &format!("{ANIMAL}sheep.gltf"),
        Vector3::new(2.0, 0.0, 2.5),
        0.9,
        0.22,
    );
    prop(
        root,
        "Hay",
        &format!("{MED}hay.gltf"),
        Vector3::new(-11.0, 0.0, 6.5),
        0.3,
        5.0,
    );
    prop(
        root,
        "HayB",
        &format!("{MED}hay.gltf"),
        Vector3::new(10.5, 0.0, -6.0),
        0.6,
        5.0,
    );
    prop(
        root,
        "Trough",
        &format!("{MED}well.gltf"),
        Vector3::new(11.0, 0.0, 6.0),
        1.0,
        1.4,
    );
}

fn build_windmill(root: &mut Gd<Node3D>) {
    let wood = standard_material(Color::from_rgb(0.52, 0.40, 0.28), 0.8, 0.0);
    let steel = standard_material(Color::from_rgb(0.48, 0.50, 0.54), 0.45, 0.3);
    for (index, y, size) in [
        (0, 2.0, 2.8_f32),
        (1, 5.5, 2.2),
        (2, 8.5, 1.6),
        (3, 10.5, 1.2),
    ] {
        add_box(
            root,
            &format!("Strut{index}"),
            Vector3::new(size, 2.6, size),
            Vector3::new(0.0, y, 0.0),
            &wood,
        );
    }
    add_box(
        root,
        "Nacelle",
        Vector3::new(1.4, 1.0, 2.0),
        Vector3::new(0.0, 11.2, 0.4),
        &steel,
    );
    let mut blades = blade_mesh();
    blades.set_position(Vector3::new(0.0, 11.2, 1.2));
    root.add_child(&blades);
    add_box(
        root,
        "Tail",
        Vector3::new(0.2, 1.8, 2.6),
        Vector3::new(0.0, 11.0, -1.6),
        &wood,
    );
    add_box(
        root,
        "Tank",
        Vector3::new(2.8, 2.2, 2.8),
        Vector3::new(4.0, 1.1, 1.0),
        &wood,
    );
    build_fence_run(
        root,
        "Fence",
        Vector3::new(5.5, 0.0, -3.0),
        0.0,
        1.0,
        6,
        3.8,
    );
}

fn build_hay_yard(root: &mut Gd<Node3D>) {
    let shed = standard_material(Color::from_rgb(0.55, 0.14, 0.11), 0.82, 0.0);
    add_box(
        root,
        "Shed",
        Vector3::new(5.0, 3.2, 4.0),
        Vector3::new(-5.0, 1.6, 0.0),
        &shed,
    );
    for (name, x, z, yaw, scale) in [
        ("HayA", -1.0, 0.0, 0.2, 10.0_f32),
        ("HayB", 1.5, 1.5, 0.5, 9.5),
        ("HayC", 3.0, -1.0, 0.8, 11.0),
    ] {
        prop(
            root,
            name,
            &format!("{MED}hay.gltf"),
            Vector3::new(x, 0.0, z),
            yaw,
            scale,
        );
    }
    prop(
        root,
        "Cart",
        MED_CART,
        Vector3::new(2.0, 0.0, 3.0),
        0.9,
        1.3,
    );
    build_fence_run(
        root,
        "Fence",
        Vector3::new(6.0, 0.0, -4.0),
        0.0,
        1.0,
        7,
        3.8,
    );
}

fn add_horizon(parent: &mut Gd<Node3D>, course: &Course, field: &Field, theme: &Theme) {
    if theme.name == "farmland" {
        return;
    }
    let (x, z) = horizon_anchor(course, &theme.name, 3_600.0);
    let ground = field.height(x, z);
    let mut root = Node3D::new_alloc();
    root.set_name("HorizonPeak");
    root.set_position(Vector3::new(x, ground, z));
    if theme.name == "desert" {
        prop(
            &mut root,
            "Butte",
            &format!("{CASTLE}rocks-large.glb"),
            Vector3::new(0.0, 40.0, 0.0),
            0.3,
            220.0,
        );
        prop(
            &mut root,
            "Cap",
            &format!("{CASTLE}rocks-large.glb"),
            Vector3::new(0.0, 260.0, 0.0),
            1.1,
            160.0,
        );
    } else {
        prop(
            &mut root,
            "Mass",
            &format!("{NATURE}cliff_large_rock.glb"),
            Vector3::ZERO,
            0.4,
            320.0,
        );
        prop(
            &mut root,
            "Horn",
            &format!("{NATURE}rock_largeA.glb"),
            Vector3::new(0.0, 220.0, 0.0),
            0.2,
            180.0,
        );
        prop(
            &mut root,
            "Snow",
            &format!("{NATURE}cliff_large_rock.glb"),
            Vector3::new(0.0, 420.0, 0.0),
            0.8,
            70.0,
        );
        let mut plume = quad(50.0, 280.0, smoke_material());
        plume.set_name("Plume");
        plume.set_position(Vector3::new(10.0, 620.0, 0.0));
        root.add_child(&plume);
    }
    godot_print!("PENGUIN_PASS_LANDMARK id=horizon dist=0 x={x:.1} z={z:.1} yaw=0");
    parent.add_child(&root);
}

fn build_horn(root: &mut Gd<Node3D>) {
    let rock = standard_material(Color::from_rgb(0.42, 0.40, 0.38), 0.92, 0.0);
    let snow = standard_material(Color::from_rgb(0.93, 0.95, 0.97), 0.7, 0.0);
    add_box(
        root,
        "Flank",
        Vector3::new(780.0, 140.0, 780.0),
        Vector3::new(0.0, 70.0, 0.0),
        &rock,
    );
    add_box(
        root,
        "Shoulder",
        Vector3::new(360.0, 280.0, 360.0),
        Vector3::new(0.0, 260.0, 0.0),
        &rock,
    );
    add_box(
        root,
        "Horn",
        Vector3::new(120.0, 560.0, 120.0),
        Vector3::new(0.0, 640.0, 0.0),
        &snow,
    );
    add_box(
        root,
        "Cap",
        Vector3::new(48.0, 160.0, 48.0),
        Vector3::new(0.0, 960.0, 0.0),
        &snow,
    );
    let mut plume = quad(40.0, 220.0, smoke_material());
    plume.set_name("Plume");
    plume.set_position(Vector3::new(16.0, 1120.0, 0.0));
    root.add_child(&plume);
}

fn build_butte(root: &mut Gd<Node3D>) {
    let rock = standard_material(Color::from_rgb(0.72, 0.42, 0.28), 0.95, 0.0);
    let cap = standard_material(Color::from_rgb(0.55, 0.32, 0.22), 0.9, 0.0);
    add_box(
        root,
        "Talus",
        Vector3::new(980.0, 90.0, 860.0),
        Vector3::new(0.0, 45.0, 0.0),
        &rock,
    );
    add_box(
        root,
        "Wall",
        Vector3::new(520.0, 460.0, 440.0),
        Vector3::new(0.0, 300.0, 0.0),
        &rock,
    );
    add_box(
        root,
        "Top",
        Vector3::new(560.0, 48.0, 480.0),
        Vector3::new(0.0, 540.0, 0.0),
        &cap,
    );
}

fn water_material(tint: Color) -> Gd<ShaderMaterial> {
    let mut shader = Shader::new_gd();
    shader.set_code(WATER_SHADER);
    let mut material = ShaderMaterial::new_gd();
    material.set_shader(&shader);
    material.set_shader_parameter("tint", &Vector3::new(tint.r, tint.g, tint.b).to_variant());
    material
}

fn smoke_material() -> Gd<ShaderMaterial> {
    let mut shader = Shader::new_gd();
    shader.set_code(SMOKE_SHADER);
    let mut material = ShaderMaterial::new_gd();
    material.set_shader(&shader);
    material
}

fn blade_mesh() -> Gd<MeshInstance3D> {
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    for blade in 0..3 {
        let angle = blade as f32 * std::f32::consts::TAU / 3.0;
        let dir = Vector3::new(angle.cos(), angle.sin(), 0.0);
        let side = Vector3::new(-angle.sin(), angle.cos(), 0.0);
        let tip = dir * 16.0;
        let a = side * 0.9;
        let b = -side * 0.9;
        push_tri(&mut tool, Vector3::ZERO, a, tip);
        push_tri(&mut tool, Vector3::ZERO, tip, b);
    }
    let mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut shader = Shader::new_gd();
    shader.set_code(BLADE_SHADER);
    let mut material = ShaderMaterial::new_gd();
    material.set_shader(&shader);
    let mut node = MeshInstance3D::new_alloc();
    node.set_mesh(&mesh);
    node.set_surface_override_material(0, &material);
    node
}

fn quad(width: f32, height: f32, material: Gd<ShaderMaterial>) -> Gd<MeshInstance3D> {
    let mut tool = SurfaceTool::new_gd();
    tool.begin(PrimitiveType::TRIANGLES);
    let hx = width * 0.5;
    let hy = height * 0.5;
    push_tri(
        &mut tool,
        Vector3::new(-hx, -hy, 0.0),
        Vector3::new(hx, -hy, 0.0),
        Vector3::new(-hx, hy, 0.0),
    );
    push_tri(
        &mut tool,
        Vector3::new(hx, -hy, 0.0),
        Vector3::new(hx, hy, 0.0),
        Vector3::new(-hx, hy, 0.0),
    );
    let mesh = tool.commit().unwrap_or_else(ArrayMesh::new_gd);
    let mut node = MeshInstance3D::new_alloc();
    node.set_mesh(&mesh);
    node.set_surface_override_material(0, &material);
    node
}

fn push_tri(tool: &mut Gd<SurfaceTool>, a: Vector3, b: Vector3, c: Vector3) {
    let normal = (b - a).cross(c - a).normalized();
    for corner in [a, b, c] {
        tool.set_normal(normal);
        tool.add_vertex(corner);
    }
}
