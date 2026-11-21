//! Write the posed model as a binary glTF file.
//!
//! ```text
//! export_glb --out rider.glb [--crank DEG] [--wheel DEG] [--steer DEG]
//!            [--effort 0..1] [--stand 0..1] [--tuck 0..1] [--bike-only]
//!            [--stats]
//! ```
//!
//! One node per part, named as the part, carrying the pose as its transform.
//! The anchors are written as empty nodes named `anchor_*`.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use gltf::json;
use gltf::json::validation::Checked::Valid;
use gltf::json::validation::USize64;
use pp_rider::{Material, PartId, PoseInput, RiderModel, RiderStyle, Surface};

struct Args {
    out: PathBuf,
    input: PoseInput,
    bike_only: bool,
    stats: bool,
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut out = None;
    let mut input = PoseInput::default();
    let mut bike_only = false;
    let mut stats = false;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--bike-only" {
            bike_only = true;
            continue;
        }
        if flag == "--stats" {
            stats = true;
            continue;
        }
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let number = || -> Result<f32, Box<dyn Error>> {
            value
                .parse::<f32>()
                .map_err(|e| format!("{flag} {value}: {e}").into())
        };
        match flag.as_str() {
            "--out" => out = Some(PathBuf::from(&value)),
            "--crank" => input.crank_angle_rad = number()?.to_radians(),
            "--wheel" => input.wheel_angle_rad = number()?.to_radians(),
            "--steer" => input.steer_rad = number()?.to_radians(),
            "--effort" => input.effort = number()?,
            "--stand" => input.stand = number()?,
            "--tuck" => input.tuck = number()?,
            _ => return Err(format!("unknown argument {flag}").into()),
        }
    }
    Ok(Args {
        out: out.ok_or("--out <path> is required")?,
        input,
        bike_only,
        stats,
    })
}

/// sRGB to linear, which is what glTF stores.
fn linear(srgb: f32) -> f32 {
    if srgb <= 0.040_45 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

fn pad_to_four(bytes: &mut Vec<u8>) {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
}

struct Writer {
    root: json::Root,
    bin: Vec<u8>,
    materials: Vec<(Material, json::Index<json::Material>)>,
}

impl Writer {
    fn view(
        &mut self,
        bytes: &[u8],
        target: json::buffer::Target,
    ) -> json::Index<json::buffer::View> {
        pad_to_four(&mut self.bin);
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(bytes.len()),
            byte_offset: Some(USize64::from(offset)),
            byte_stride: None,
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            target: Some(Valid(target)),
        })
    }

    fn vec3_accessor(&mut self, data: &[[f32; 3]], bounds: bool) -> json::Index<json::Accessor> {
        let bytes: Vec<u8> = data
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let view = self.view(&bytes, json::buffer::Target::ArrayBuffer);
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in data {
            for c in 0..3 {
                min[c] = min[c].min(p[c]);
                max[c] = max[c].max(p[c]);
            }
        }
        self.root.push(json::Accessor {
            buffer_view: Some(view),
            byte_offset: Some(USize64(0)),
            count: USize64::from(data.len()),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            extensions: Default::default(),
            extras: Default::default(),
            type_: Valid(json::accessor::Type::Vec3),
            min: bounds.then(|| json::Value::from(min.to_vec())),
            max: bounds.then(|| json::Value::from(max.to_vec())),
            name: None,
            normalized: false,
            sparse: None,
        })
    }

    fn index_accessor(&mut self, indices: &[u32]) -> json::Index<json::Accessor> {
        let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let view = self.view(&bytes, json::buffer::Target::ElementArrayBuffer);
        self.root.push(json::Accessor {
            buffer_view: Some(view),
            byte_offset: Some(USize64(0)),
            count: USize64::from(indices.len()),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::U32,
            )),
            extensions: Default::default(),
            extras: Default::default(),
            type_: Valid(json::accessor::Type::Scalar),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        })
    }

    fn material(&mut self, material: Material) -> json::Index<json::Material> {
        if let Some((_, index)) = self.materials.iter().find(|(m, _)| *m == material) {
            return *index;
        }
        let [r, g, b] = material.base_color.map(linear);
        let index = self.root.push(json::Material {
            pbr_metallic_roughness: json::material::PbrMetallicRoughness {
                base_color_factor: json::material::PbrBaseColorFactor([r, g, b, 1.0]),
                metallic_factor: json::material::StrengthFactor(material.metallic),
                roughness_factor: json::material::StrengthFactor(material.roughness),
                ..Default::default()
            },
            ..Default::default()
        });
        self.materials.push((material, index));
        index
    }

    fn primitive(&mut self, surface: &Surface) -> json::mesh::Primitive {
        let positions = self.vec3_accessor(&surface.positions, true);
        let normals = self.vec3_accessor(&surface.normals, false);
        let indices = self.index_accessor(&surface.indices);
        let material = self.material(surface.material);
        let mut attributes = BTreeMap::new();
        attributes.insert(Valid(json::mesh::Semantic::Positions), positions);
        attributes.insert(Valid(json::mesh::Semantic::Normals), normals);
        json::mesh::Primitive {
            attributes,
            extensions: Default::default(),
            extras: Default::default(),
            indices: Some(indices),
            material: Some(material),
            mode: Valid(json::mesh::Mode::Triangles),
            targets: None,
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    let model = RiderModel::build(&RiderStyle::default());
    let pose = model.pose(&args.input);

    let mut writer = Writer {
        root: json::Root::default(),
        bin: Vec::new(),
        materials: Vec::new(),
    };
    writer.root.push(json::Buffer {
        byte_length: USize64(0),
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        uri: None,
    });

    let mut nodes = Vec::new();
    for ((part, transform), id) in model.parts().iter().zip(&pose.transforms).zip(PartId::ALL) {
        let skip = part.surfaces().is_empty() || (args.bike_only && !id.is_bike());
        let mesh = if skip {
            None
        } else {
            let primitives = part
                .surfaces()
                .iter()
                .map(|surface| writer.primitive(surface))
                .collect();
            Some(writer.root.push(json::Mesh {
                extensions: Default::default(),
                extras: Default::default(),
                name: Some(part.name().to_owned()),
                primitives,
                weights: None,
            }))
        };
        nodes.push(writer.root.push(json::Node {
            mesh,
            name: Some(part.name().to_owned()),
            translation: Some(transform.translation),
            rotation: Some(json::scene::UnitQuaternion(transform.rotation)),
            ..Default::default()
        }));
    }
    let anchors = [
        ("anchor_saddle_top", pose.anchors.saddle_top),
        ("anchor_hood_l", pose.anchors.hood_l),
        ("anchor_hood_r", pose.anchors.hood_r),
        ("anchor_eye", pose.anchors.eye),
        ("anchor_rear_axle", pose.anchors.rear_axle),
        ("anchor_front_axle", pose.anchors.front_axle),
        ("anchor_bottom_bracket", pose.anchors.bottom_bracket),
    ];
    for (name, at) in anchors {
        nodes.push(writer.root.push(json::Node {
            name: Some(name.to_owned()),
            translation: Some(at),
            ..Default::default()
        }));
    }
    let scene = writer.root.push(json::Scene {
        extensions: Default::default(),
        extras: Default::default(),
        name: Some("rider".to_owned()),
        nodes,
    });
    writer.root.scene = Some(scene);

    pad_to_four(&mut writer.bin);
    writer.root.buffers[0].byte_length = USize64::from(writer.bin.len());

    let json_bytes = json::serialize::to_string(&writer.root)?.into_bytes();
    let length = 12 + 8 + json_bytes.len().next_multiple_of(4) + 8 + writer.bin.len();
    let glb = gltf::binary::Glb {
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: length.try_into()?,
        },
        json: Cow::Owned(json_bytes),
        bin: Some(Cow::Owned(writer.bin)),
    };
    if let Some(dir) = args.out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    glb.to_writer(BufWriter::new(File::create(&args.out)?))?;

    println!(
        "wrote {} ({} triangles, {} parts)",
        args.out.display(),
        model.triangle_count(),
        model.parts().len()
    );
    if args.stats {
        for part in model.parts() {
            println!(
                "  {:<12} {:>6} triangles",
                part.name(),
                part.triangle_count()
            );
        }
        for (name, at) in anchors {
            println!("  {name:<22} {:>7.4} {:>7.4} {:>7.4}", at[0], at[1], at[2]);
        }
    }
    Ok(())
}
