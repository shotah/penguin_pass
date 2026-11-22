//! Runtime asset library. Reads `res://assets/manifest.json` and loads glTF
//! and images with `GLTFDocument` and `Image.load`. A missing manifest, a
//! broken file, or an unknown id falls back to the procedural placeholders.
//! The game still starts when `godot/assets/` is absent.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use godot::classes::base_material_3d::{DiffuseMode, SpecularMode, Transparency};
use godot::classes::{
    GltfDocument, GltfState, Image, ImageTexture, Material, Mesh, MeshInstance3D, Node,
    StandardMaterial3D,
};
use godot::global::Error;
use godot::obj::Gd;
use godot::prelude::*;
use serde::Deserialize;

use crate::theme::ScatterLayer;

/// One mesh the scatterer can instance.
pub struct ScatterProp {
    /// Imported mesh, materials included.
    pub mesh: Gd<Mesh>,
    /// Metres to raise the instance so its lowest vertex sits on the ground.
    pub lift: f32,
}

/// Parsed manifest plus a cache of meshes and images loaded so far.
pub struct Catalog {
    entries: Vec<Entry>,
    by_id: HashMap<String, usize>,
    warned: RefCell<HashSet<String>>,
    meshes: RefCell<HashMap<String, Option<ScatterProp>>>,
    images: RefCell<HashMap<String, Option<Gd<ImageTexture>>>>,
    models_loaded: Cell<i32>,
    textures_loaded: Cell<i32>,
    sky_loaded: Cell<i32>,
}

#[derive(Clone, Deserialize)]
struct Entry {
    id: String,
    kind: String,
    path: String,
    #[serde(default)]
    lift_m: f32,
    #[serde(default)]
    maps: Maps,
    #[serde(default)]
    tile_m: f32,
}

#[derive(Clone, Default, Deserialize)]
struct Maps {
    #[serde(default)]
    albedo: String,
}

#[derive(Deserialize)]
struct ManifestFile {
    assets: Vec<Entry>,
}

impl Catalog {
    /// Empty catalog. Scatter uses placeholders and the ground stays procedural.
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            by_id: HashMap::new(),
            warned: RefCell::new(HashSet::new()),
            meshes: RefCell::new(HashMap::new()),
            images: RefCell::new(HashMap::new()),
            models_loaded: Cell::new(0),
            textures_loaded: Cell::new(0),
            sky_loaded: Cell::new(0),
        }
    }

    /// Load the manifest. A missing or unreadable file is an empty catalog.
    pub fn load() -> Self {
        let Some(file) = godot::classes::FileAccess::open(
            "res://assets/manifest.json",
            godot::classes::file_access::ModeFlags::READ,
        ) else {
            godot_error!("Penguin Pass: assets/manifest.json missing, using placeholders");
            return Self::empty();
        };
        let text = file.get_as_text().to_string();
        match serde_json::from_str::<ManifestFile>(&text) {
            Ok(file) => Self::from_entries(file.assets),
            Err(error) => {
                godot_error!("Penguin Pass: asset manifest rejected ({error})");
                Self::empty()
            }
        }
    }

    fn from_entries(entries: Vec<Entry>) -> Self {
        let by_id = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.id.clone(), index))
            .collect();
        Self {
            entries,
            by_id,
            warned: RefCell::new(HashSet::new()),
            meshes: RefCell::new(HashMap::new()),
            images: RefCell::new(HashMap::new()),
            models_loaded: Cell::new(0),
            textures_loaded: Cell::new(0),
            sky_loaded: Cell::new(0),
        }
    }

    /// glTF files that actually loaded. Zero when the library is absent.
    pub fn loaded_models(&self) -> i32 {
        self.models_loaded.get()
    }

    /// Image files that actually loaded, including the sky.
    pub fn loaded_textures(&self) -> i32 {
        self.textures_loaded.get()
    }

    /// 1 when the sky image loaded, otherwise 0.
    pub fn loaded_sky(&self) -> i32 {
        self.sky_loaded.get()
    }

    /// True when this kind (or these ids) names at least one manifest entry.
    pub fn has_models(&self, kind: &str, ids: &[String]) -> bool {
        !self.matching(kind, ids).is_empty()
    }

    /// Loaded meshes for a scatter layer. Failed files are skipped and logged once.
    pub fn props_for(&self, layer: &ScatterLayer) -> Vec<ScatterProp> {
        let mut props = Vec::new();
        for index in self.matching(&layer.kind, &layer.ids) {
            let entry = &self.entries[index];
            let id = entry.id.clone();
            let path = entry.path.clone();
            let lift = entry.lift_m;
            if let Some(cached) = self.meshes.borrow().get(&id) {
                if let Some(prop) = cached {
                    props.push(ScatterProp {
                        mesh: prop.mesh.clone(),
                        lift: prop.lift,
                    });
                }
                continue;
            }
            let loaded = load_gltf(&path).map(|mesh| ScatterProp { mesh, lift });
            if loaded.is_some() {
                self.models_loaded.set(self.models_loaded.get() + 1);
            }
            if loaded.is_none() {
                self.warn_once(&id, &format!("could not load model '{id}' ({path})"));
            }
            if let Some(prop) = &loaded {
                props.push(ScatterProp {
                    mesh: prop.mesh.clone(),
                    lift: prop.lift,
                });
            }
            self.meshes.borrow_mut().insert(id, loaded);
        }
        if props.is_empty() && (!layer.ids.is_empty() || !layer.kind.is_empty()) {
            self.warn_once(
                &format!("kind:{}", layer.kind),
                &format!(
                    "no model for scatter kind '{}' — using the placeholder",
                    layer.kind
                ),
            );
        }
        props
    }

    /// Bind up to four ground textures. Returns whether any real image loaded.
    pub fn bind_ground(
        &self,
        material: &mut Gd<godot::classes::ShaderMaterial>,
        theme: &crate::theme::Theme,
    ) -> bool {
        let mut any = false;
        for (index, layer) in theme.ground.iter().take(4).enumerate() {
            let tile = self.tile_of(&layer.texture).unwrap_or(4.0);
            if !layer.normal.is_empty() {
                self.warn_once(
                    &format!("normal:{}", layer.normal),
                    &format!(
                        "ground normal '{}' is named; the terrain shader uses the albedo only",
                        layer.normal
                    ),
                );
            }
            let albedo = self.texture_or_color(&layer.texture, "albedo", layer.color);
            any = any || self.entry(&layer.texture).is_some();
            material.set_shader_parameter(&format!("layer{index}"), &albedo.to_variant());
            material.set_shader_parameter(&format!("tile{index}"), &tile.to_variant());
        }
        for index in theme.ground.len()..4 {
            material.set_shader_parameter(
                &format!("layer{index}"),
                &solid([1.0, 1.0, 1.0]).to_variant(),
            );
            material.set_shader_parameter(&format!("tile{index}"), &4.0_f32.to_variant());
        }
        material.set_shader_parameter("textured", &(if any { 1.0_f32 } else { 0.0 }).to_variant());
        any
    }

    /// Bind asphalt and gravel. Returns whether the asphalt image loaded.
    pub fn bind_road(&self, material: &mut Gd<godot::classes::ShaderMaterial>) -> bool {
        let asphalt = self.texture_of("road_asphalt", "albedo");
        let gravel = self.texture_of("ground_gravel", "albedo");
        let loaded = asphalt.is_some();
        material.set_shader_parameter(
            "asphalt_tex",
            &asphalt
                .unwrap_or_else(|| solid([0.34, 0.34, 0.35]))
                .to_variant(),
        );
        material.set_shader_parameter(
            "gravel_tex",
            &gravel
                .unwrap_or_else(|| solid([0.48, 0.42, 0.32]))
                .to_variant(),
        );
        material.set_shader_parameter(
            "asphalt_tile",
            &self.tile_of("road_asphalt").unwrap_or(4.0).to_variant(),
        );
        material.set_shader_parameter(
            "gravel_tile",
            &self.tile_of("ground_gravel").unwrap_or(3.0).to_variant(),
        );
        material.set_shader_parameter(
            "textured",
            &(if loaded { 1.0_f32 } else { 0.0 }).to_variant(),
        );
        loaded
    }

    /// Equirectangular sky, if the manifest names one and the file loads.
    pub fn sky_texture(&self) -> Option<Gd<ImageTexture>> {
        let entry = self.entries.iter().find(|entry| entry.kind == "sky")?;
        let path = entry.path.clone();
        let id = entry.id.clone();
        let image = self.image_file(&path);
        if image.is_some() {
            self.sky_loaded.set(1);
        } else {
            self.warn_once(&id, &format!("could not load sky '{id}' ({path})"));
        }
        image
    }

    fn matching(&self, kind: &str, ids: &[String]) -> Vec<usize> {
        if !ids.is_empty() {
            return ids
                .iter()
                .filter_map(|id| self.by_id.get(id).copied())
                .collect();
        }
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| kind_matches(kind, &entry.kind))
            .map(|(index, _)| index)
            .collect()
    }

    fn entry(&self, id: &str) -> Option<&Entry> {
        self.by_id.get(id).map(|index| &self.entries[*index])
    }

    fn tile_of(&self, id: &str) -> Option<f32> {
        let tile = self.entry(id)?.tile_m;
        (tile > 0.0).then_some(tile)
    }

    fn texture_of(&self, id: &str, slot: &str) -> Option<Gd<ImageTexture>> {
        let entry = self.entry(id)?;
        let path = match slot {
            "albedo" => {
                if entry.maps.albedo.is_empty() {
                    entry.path.clone()
                } else {
                    entry.maps.albedo.clone()
                }
            }
            _ => String::new(),
        };
        if path.is_empty() {
            return None;
        }
        let image = self.image_file(&path);
        if image.is_none() {
            self.warn_once(
                &format!("{id}:{slot}"),
                &format!("could not load {slot} for '{id}'"),
            );
        }
        image
    }

    fn texture_or_color(&self, id: &str, slot: &str, color: [f32; 3]) -> Gd<ImageTexture> {
        self.texture_of(id, slot).unwrap_or_else(|| solid(color))
    }

    fn image_file(&self, path: &str) -> Option<Gd<ImageTexture>> {
        if let Some(cached) = self.images.borrow().get(path) {
            return cached.clone();
        }
        let loaded = load_image(path);
        if loaded.is_some() {
            self.textures_loaded.set(self.textures_loaded.get() + 1);
        }
        self.images
            .borrow_mut()
            .insert(path.to_string(), loaded.clone());
        loaded
    }

    fn warn_once(&self, key: &str, message: &str) {
        if self.warned.borrow_mut().insert(key.to_string()) {
            godot_error!("Penguin Pass: {message}");
        }
    }
}

fn kind_matches(wanted: &str, have: &str) -> bool {
    if wanted == have {
        return true;
    }
    wanted == "tree" && have.starts_with("tree")
}

fn load_gltf(path: &str) -> Option<Gd<Mesh>> {
    let mut document = GltfDocument::new_gd();
    let state = GltfState::new_gd();
    let error = document.append_from_file(path, &state);
    if error != Error::OK {
        return None;
    }
    // `generate_scene` returns a node that is not in the tree. Dropping the
    // `Gd` does not free it, so the imported graph — and the mesh, materials,
    // and textures it still references — stays alive until shutdown. The
    // renderer then frees those leftover RIDs, and ObjectDB frees the nodes
    // afterwards, which frees the same RIDs again. The release export aborts
    // with "double free or corruption". Take the mesh ref, then free the graph
    // while the engine is still up.
    let scene = document.generate_scene(&state)?;
    let mut mesh = find_mesh(&scene)?;
    scene.free();
    tune_prop_materials(&mut mesh);
    Some(mesh)
}

fn tune_prop_materials(mesh: &mut Gd<Mesh>) {
    let count = mesh.get_surface_count();
    for surface in 0..count {
        let Some(material) = mesh.surface_get_material(surface) else {
            continue;
        };
        let Ok(mut mat) = material.try_cast::<StandardMaterial3D>() else {
            continue;
        };
        mat.set_specular_mode(SpecularMode::DISABLED);
        mat.set_metallic(0.0);
        mat.set_specular(0.0);
        let roughness = mat.get_roughness().max(0.96);
        mat.set_roughness(roughness);
        if is_foliage_material(&mat) {
            mat.set_diffuse_mode(DiffuseMode::LAMBERT_WRAP);
        }
        mesh.surface_set_material(surface, &mat.upcast::<Material>());
    }
}

fn is_foliage_material(mat: &Gd<StandardMaterial3D>) -> bool {
    let transparency = mat.get_transparency();
    if transparency == Transparency::ALPHA_SCISSOR || transparency == Transparency::ALPHA_HASH {
        return true;
    }
    const FOLIAGE: &[&str] = &[
        "leaf", "leaves", "grass", "flower", "foliage", "bush", "fern",
    ];
    let label = mat.get("resource_name").to_string();
    let name = label.to_ascii_lowercase();
    FOLIAGE.iter().any(|token| name.contains(token))
}

fn find_mesh(node: &Gd<Node>) -> Option<Gd<Mesh>> {
    if let Ok(mesh_node) = node.clone().try_cast::<MeshInstance3D>()
        && let Some(mesh) = mesh_node.get_mesh()
    {
        return Some(mesh);
    }
    for child in node.get_children().iter_shared() {
        if let Some(mesh) = find_mesh(&child) {
            return Some(mesh);
        }
    }
    None
}

fn load_image(path: &str) -> Option<Gd<ImageTexture>> {
    let mut image = Image::new_gd();
    if image.load(path) != Error::OK {
        return None;
    }
    let _mipmaps = image.generate_mipmaps();
    ImageTexture::create_from_image(&image)
}

fn solid(color: [f32; 3]) -> Gd<ImageTexture> {
    let mut image = Image::create_empty(1, 1, false, godot::classes::image::Format::RGB8)
        .unwrap_or_else(Image::new_gd);
    image.set_pixel(0, 0, Color::from_rgb(color[0], color[1], color[2]));
    ImageTexture::create_from_image(&image).unwrap_or_else(ImageTexture::new_gd)
}
