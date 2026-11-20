//! Ride a synthetic 5 km loop and print the dashboard URLs.
//!
//! Bind address defaults to `0.0.0.0:7878`. Pass another socket address as the
//! first argument (`127.0.0.1:0` picks a free local port).

use pp_cast::{CastConfig, CastServer, CourseProfile, DEFAULT_PORT, ProfilePoint};
use pp_core::Telemetry;
use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

const LENGTH_M: f64 = 5000.0;

fn main() {
    let bind = bind_address();
    let course = demo_course();
    let server = match CastServer::start(CastConfig {
        bind,
        course: course.clone(),
    }) {
        Ok(server) => server,
        Err(err) => {
            eprintln!("pp-cast demo: {err}");
            std::process::exit(1);
        }
    };

    println!("Penguin Pass TV dashboard");
    for url in server.urls() {
        println!("  {url}");
    }
    println!("Open one of these in a browser on the same Wi-Fi.");
    println!("Ctrl-C to stop.");
    let _ = io::stdout().flush();

    let mut distance_m = 0.0_f64;
    let started = Instant::now();
    let mut tick = Instant::now();
    let step = Duration::from_millis(50);
    loop {
        let now = Instant::now();
        let dt_s = now.saturating_duration_since(tick).as_secs_f64().min(0.25);
        tick = now;
        let elapsed_s = now.saturating_duration_since(started).as_secs_f64();
        distance_m = publish_frame(&server, &course, elapsed_s, dt_s, distance_m);
        std::thread::sleep(step);
    }
}

fn bind_address() -> SocketAddr {
    let mut args = std::env::args().skip(1);
    let Some(raw) = args.next() else {
        return SocketAddr::from((Ipv4Addr::UNSPECIFIED, DEFAULT_PORT));
    };
    if args.next().is_some() || raw == "-h" || raw == "--help" {
        eprintln!("usage: demo [bind-address]");
        eprintln!("example: demo 127.0.0.1:0");
        std::process::exit(2);
    }
    raw.parse().unwrap_or_else(|_| {
        eprintln!("usage: demo [bind-address]");
        eprintln!("could not parse bind address: {raw}");
        std::process::exit(2);
    })
}

fn demo_course() -> CourseProfile {
    let samples = [
        (0.0, 100.0_f32),
        (400.0, 108.0),
        (900.0, 145.0),
        (1400.0, 160.0),
        (1800.0, 150.0),
        (2300.0, 90.0),
        (2800.0, 95.0),
        (3400.0, 170.0),
        (3900.0, 155.0),
        (4500.0, 115.0),
        (LENGTH_M, 100.0),
    ];
    CourseProfile {
        name: "5 km Mountain Loop".to_owned(),
        length_m: LENGTH_M,
        points: samples
            .into_iter()
            .map(|(distance_m, elevation_m)| ProfilePoint {
                distance_m,
                elevation_m,
            })
            .collect(),
    }
}

fn publish_frame(
    server: &CastServer,
    course: &CourseProfile,
    elapsed_s: f64,
    dt_s: f64,
    distance_m: f64,
) -> f64 {
    let t = elapsed_s as f32;
    let power_w = 200.0 + 22.0 * (t * 0.35).sin() + 8.0 * (t * 1.3).sin();
    let lap_distance_m = distance_m % course.length_m;
    let grade_pct = grade_at(&course.points, lap_distance_m);
    let speed = speed_mps(power_w, grade_pct);
    let cadence = (86.0 - grade_pct * 0.6 + 3.0 * (t * 0.8).sin()).clamp(60.0, 110.0);
    let heart = (148.0 + (power_w - 200.0) * 0.12 + grade_pct * 0.8).clamp(90.0, 185.0);
    // Truncates toward zero, so lap 1 covers [0, length) and the next lap
    // starts on each exact multiple of the course length.
    let lap = 1 + (distance_m / course.length_m) as u32;
    server.publish(Telemetry {
        power_w,
        cadence_rpm: Some(cadence),
        heart_rate_bpm: Some(heart.round() as u8),
        speed_mps: speed,
        distance_m,
        lap_distance_m,
        lap,
        grade_pct,
        elevation_m: elevation_at(&course.points, lap_distance_m),
        elapsed_s,
    });
    distance_m + f64::from(speed) * dt_s
}

fn speed_mps(power_w: f32, grade_pct: f32) -> f32 {
    let flat = 3.5 + power_w * 0.027;
    let adjusted = if grade_pct >= 0.0 {
        flat - grade_pct * 0.42
    } else {
        flat - grade_pct * 0.18
    };
    adjusted.clamp(1.4, 14.0)
}

fn grade_at(points: &[ProfilePoint], distance_m: f64) -> f32 {
    let Some((start, end)) = segment(points, distance_m) else {
        return 0.0;
    };
    let run = end.distance_m - start.distance_m;
    if run <= f64::EPSILON {
        return 0.0;
    }
    let rise = f64::from(end.elevation_m - start.elevation_m);
    (rise / run * 100.0) as f32
}

fn elevation_at(points: &[ProfilePoint], distance_m: f64) -> f32 {
    let Some((start, end)) = segment(points, distance_m) else {
        return points.last().map(|point| point.elevation_m).unwrap_or(0.0);
    };
    let run = end.distance_m - start.distance_m;
    if run <= f64::EPSILON {
        return end.elevation_m;
    }
    let t = ((distance_m - start.distance_m) / run) as f32;
    start.elevation_m + (end.elevation_m - start.elevation_m) * t
}

fn segment(points: &[ProfilePoint], distance_m: f64) -> Option<(ProfilePoint, ProfilePoint)> {
    for pair in points.windows(2) {
        if distance_m >= pair[0].distance_m && distance_m <= pair[1].distance_m {
            return Some((pair[0], pair[1]));
        }
    }
    None
}
