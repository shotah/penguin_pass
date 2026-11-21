//! Write a seeded course as JSON and, optionally, an SVG map.
//!
//! ```text
//! cargo run -p pp-routegen --example generate -- \
//!   --seed 7 --distance-km 10 --preset desert --name "Desert 10" \
//!   --out route.json --svg route.svg
//! ```
//!
//! `--sheet out.svg` draws six seeds (`seed` through `seed + 5`) of the same
//! distance and preset.

use std::env;
use std::fs;
use std::process::ExitCode;
use std::time::Instant;

use pp_routegen::{
    RouteSpec, Terrain, TerrainPreset, fingerprint, generate, render_contact_sheet, render_svg,
    survey,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse(env::args().skip(1))?;
    let preset = TerrainPreset::parse(&args.preset)
        .ok_or_else(|| format!("unknown preset '{}'", args.preset))?;
    let distance_m = args.distance_km * 1000.0;
    let spec = RouteSpec::new(&args.name, args.seed, distance_m, Terrain::Preset(preset));
    let started = Instant::now();
    let route = generate(&spec).map_err(|err| err.to_string())?;
    let elapsed = started.elapsed();
    fs::write(&args.out, route.json()).map_err(|err| err.to_string())?;
    if let Some(path) = &args.svg {
        fs::write(path, render_svg(route.course())).map_err(|err| err.to_string())?;
    }
    if let Some(path) = &args.sheet {
        let mut owned = Vec::new();
        for offset in 0..6 {
            let spec = RouteSpec::new(
                format!("{} {}", args.name, args.seed + offset),
                args.seed + offset,
                distance_m,
                Terrain::Preset(preset),
            );
            owned.push(generate(&spec).map_err(|err| err.to_string())?);
        }
        let panels: Vec<(String, _)> = owned
            .iter()
            .enumerate()
            .map(|(i, route)| (format!("seed {}", args.seed + i as u64), route.course()))
            .collect();
        fs::write(path, render_contact_sheet(&panels)).map_err(|err| err.to_string())?;
    }
    let report = survey(route.course(), 5.0);
    println!("name={}", route.course().name());
    println!("preset={}", preset.as_str());
    println!("seed={}", args.seed);
    println!("attempts={}", route.attempts());
    println!("length_m={:.3}", report.length_m);
    println!("ascent_m={:.2}", report.ascent_m);
    println!("max_grade_pct={:.2}", report.max_grade_pct);
    println!("min_grade_pct={:.2}", report.min_grade_pct);
    println!("min_radius_m={:.2}", route.min_radius_m());
    println!(
        "footprint_m={:.1}x{:.1}",
        report.footprint_east_m, report.footprint_north_m
    );
    println!("closest_approach_m={:.2}", report.closest_approach_m);
    println!(
        "elevation_m={:.1}..{:.1}",
        report.min_elevation_m, report.max_elevation_m
    );
    println!("generate_ms={:.2}", elapsed.as_secs_f64() * 1000.0);
    println!("json_fnv64={:016x}", fingerprint(route.json()));
    Ok(())
}

struct Args {
    seed: u64,
    distance_km: f64,
    preset: String,
    name: String,
    out: String,
    svg: Option<String>,
    sheet: Option<String>,
}

impl Args {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let args: Vec<String> = args.collect();
        let mut seed = None;
        let mut distance_km = None;
        let mut preset = None;
        let mut name = None;
        let mut out = None;
        let mut svg = None;
        let mut sheet = None;
        let mut i = 0;
        while i < args.len() {
            let key = args[i].as_str();
            let value = args
                .get(i + 1)
                .ok_or_else(|| format!("missing value for {key}"))?;
            match key {
                "--seed" => seed = Some(parse_u64(value)?),
                "--distance-km" => distance_km = Some(parse_f64(value)?),
                "--preset" => preset = Some(value.clone()),
                "--name" => name = Some(value.clone()),
                "--out" => out = Some(value.clone()),
                "--svg" => svg = Some(value.clone()),
                "--sheet" => sheet = Some(value.clone()),
                other => return Err(format!("unknown argument {other}")),
            }
            i += 2;
        }
        Ok(Self {
            seed: seed.ok_or("missing --seed")?,
            distance_km: distance_km.ok_or("missing --distance-km")?,
            preset: preset.ok_or("missing --preset")?,
            name: name.ok_or("missing --name")?,
            out: out.ok_or("missing --out")?,
            svg,
            sheet,
        })
    }
}

fn parse_u64(text: &str) -> Result<u64, String> {
    text.parse::<u64>()
        .map_err(|_| format!("expected an integer, got {text}"))
}

fn parse_f64(text: &str) -> Result<f64, String> {
    let value = text
        .parse::<f64>()
        .map_err(|_| format!("expected a number, got {text}"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("expected a finite number, got {text}"))
    }
}
