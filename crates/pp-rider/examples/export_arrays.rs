//! Write the posed model as plain arrays in JSON, the way an engine
//! integration reads it: no glTF in between.
//!
//! ```text
//! export_arrays --out rider.json [--crank DEG] [--wheel DEG] [--effort 0..1]
//! ```
//!
//! The preview project builds Godot `ArrayMesh` surfaces from this file to
//! prove the integration recipe in `docs/rider.md`: clockwise indices, sRGB
//! colours, and the four-column transform.
//!
//! ```text
//! { "parts": [ { "name": "frame",
//!                "transform": [[x], [y], [z], [origin]],
//!                "surfaces": [ { "color": [r, g, b], "roughness": r, "metallic": m,
//!                                "positions": [x, y, z, ...],
//!                                "normals": [x, y, z, ...],
//!                                "indices_clockwise": [a, b, c, ...] } ] } ] }
//! ```

use std::error::Error;
use std::fmt::Write as _;
use std::path::PathBuf;

use pp_rider::{PoseInput, RiderModel, RiderStyle};

fn list(out: &mut String, values: impl IntoIterator<Item = f32>) {
    out.push('[');
    for (i, value) in values.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        // `{:?}` keeps the shortest text that reads back as the same f32.
        let _ = write!(out, "{value:?}");
    }
    out.push(']');
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut path = None;
    let mut input = PoseInput::default();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--out" => path = Some(PathBuf::from(&value)),
            "--crank" => input.crank_angle_rad = value.parse::<f32>()?.to_radians(),
            "--wheel" => input.wheel_angle_rad = value.parse::<f32>()?.to_radians(),
            "--effort" => input.effort = value.parse()?,
            _ => return Err(format!("unknown argument {flag}").into()),
        }
    }
    let path = path.ok_or("--out <path> is required")?;

    let model = RiderModel::build(&RiderStyle::default());
    let pose = model.pose(&input);

    let mut out = String::from("{\"parts\":[");
    for (p, (part, transform)) in model.parts().iter().zip(&pose.transforms).enumerate() {
        if p > 0 {
            out.push(',');
        }
        let _ = write!(out, "{{\"name\":\"{}\",\"transform\":[", part.name());
        for (c, column) in transform.to_cols().into_iter().enumerate() {
            if c > 0 {
                out.push(',');
            }
            list(&mut out, column);
        }
        out.push_str("],\"surfaces\":[");
        for (s, surface) in part.surfaces().iter().enumerate() {
            if s > 0 {
                out.push(',');
            }
            let material = surface.material;
            out.push_str("{\"color\":");
            list(&mut out, material.base_color);
            let _ = write!(
                out,
                ",\"roughness\":{:?},\"metallic\":{:?},\"positions\":",
                material.roughness, material.metallic
            );
            list(&mut out, surface.positions.iter().flatten().copied());
            out.push_str(",\"normals\":");
            list(&mut out, surface.normals.iter().flatten().copied());
            out.push_str(",\"indices_clockwise\":[");
            for (i, index) in surface.indices_clockwise().into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = write!(out, "{index}");
            }
            out.push_str("]}");
        }
        out.push_str("]}");
    }
    out.push_str("]}\n");

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, out)?;
    println!("wrote {}", path.display());
    Ok(())
}
