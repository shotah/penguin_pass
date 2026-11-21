//! List nearby trainers, or print live samples from one of them.
//!
//! With no arguments this scans for about eight seconds. `--connect` subscribes
//! to measurements and does not request control. `--grade` requests control,
//! sets a grade, and sets the grade back to 0% before it exits.

use std::env;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use pp_core::{TrainerCommand, TrainerSample};
use pp_trainer::{
    BleTrainer, ConnectionState, HeartRateLink, SessionMode, Trainer, TrainerConfig,
    spawn_ble_heart_rate,
};

fn main() -> ExitCode {
    let args = match Args::parse(env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            eprintln!("{}", Args::usage());
            return ExitCode::from(2);
        }
    };

    let mut config = TrainerConfig::default();
    if args.grade.is_some() {
        config.mode = SessionMode::Control;
    } else if args.connect.is_some() {
        config.mode = SessionMode::ReadOnly;
    }

    let trainer = match BleTrainer::spawn_with(config) {
        Ok(trainer) => trainer,
        Err(err) => {
            eprintln!("could not start bluetooth: {err}");
            return ExitCode::from(1);
        }
    };

    let heart = spawn_ble_heart_rate();

    if let Err(err) = scan(&trainer, args.scan_seconds) {
        eprintln!("{err}");
        return ExitCode::from(1);
    }

    let Some(query) = args.connect else {
        print_discovered(&trainer);
        println!();
        print_straps(&heart);
        return ExitCode::SUCCESS;
    };
    drop(heart);

    if !trainer
        .discovered()
        .iter()
        .any(|device| matches_query(device, &query))
    {
        print_discovered(&trainer);
        eprintln!("no trainer matches {query}");
        return ExitCode::from(1);
    }

    if let Err(err) = trainer.connect(&query) {
        eprintln!("connect failed: {err}");
        return ExitCode::from(1);
    }
    if let Err(err) = wait_until_connected(&trainer, Duration::from_secs(20)) {
        eprintln!("connect failed: {err}");
        return ExitCode::from(1);
    }

    let mut trainer = trainer;
    if let Some(grade_pct) = args.grade {
        if let Err(err) = trainer.send(TrainerCommand::SetGrade { grade_pct }) {
            eprintln!("could not set grade: {err}");
            return ExitCode::from(1);
        }
        println!("requested grade {grade_pct:.1}%");
    }

    for second in 1..=args.seconds {
        thread::sleep(Duration::from_secs(1));
        if let ConnectionState::Failed(reason) = trainer.state() {
            eprintln!("failed: {reason}");
            return ExitCode::from(1);
        }
        let line = trainer
            .latest_sample()
            .map(|sample| format_sample(&sample))
            .unwrap_or_else(|| "power=- cadence=- speed=- hr=-".to_string());
        println!("t={second}s {line}");
    }
    if args.grade.is_some() {
        if let Err(err) = trainer.send(TrainerCommand::SetGrade { grade_pct: 0.0 }) {
            eprintln!("could not clear grade: {err}");
            return ExitCode::from(1);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while !trainer.control_idle() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        println!("grade cleared");
    }
    ExitCode::SUCCESS
}

struct Args {
    scan_seconds: u64,
    connect: Option<String>,
    seconds: u64,
    grade: Option<f32>,
}

impl Args {
    fn usage() -> &'static str {
        "usage: pp-scan [--scan-seconds N]\n       \
         pp-scan --connect <id-or-name> [--seconds N] [--scan-seconds N]\n       \
         pp-scan --connect <id-or-name> --grade <percent> [--seconds N]"
    }

    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut scan_seconds = 8;
        let mut connect = None;
        let mut seconds = 10;
        let mut grade = None;
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--scan-seconds" => {
                    scan_seconds = number(&mut args, "--scan-seconds")?;
                }
                "--seconds" => {
                    seconds = number(&mut args, "--seconds")?;
                }
                "--connect" => {
                    connect = Some(required(&mut args, "--connect")?);
                }
                "--grade" => {
                    let text = required(&mut args, "--grade")?;
                    let value = text
                        .parse::<f32>()
                        .map_err(|_| format!("--grade {text} is not a number"))?;
                    if !value.is_finite() {
                        return Err("--grade is not finite".into());
                    }
                    grade = Some(value);
                }
                "--help" | "-h" => return Err(String::new()),
                other => return Err(format!("unknown argument {other}")),
            }
        }
        if grade.is_some() && connect.is_none() {
            return Err("--grade requires --connect".into());
        }
        Ok(Self {
            scan_seconds,
            connect,
            seconds,
            grade,
        })
    }
}

fn number(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<u64, String> {
    let text = required(args, flag)?;
    text.parse::<u64>()
        .map_err(|_| format!("{flag} {text} is not a whole number"))
}

fn required(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| format!("{flag} needs a value"))
}

fn scan(trainer: &BleTrainer, seconds: u64) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if let ConnectionState::Failed(reason) = trainer.state() {
            return Err(format!("bluetooth: {reason}"));
        }
        thread::sleep(Duration::from_millis(200));
    }
    if let ConnectionState::Failed(reason) = trainer.state() {
        return Err(format!("bluetooth: {reason}"));
    }
    Ok(())
}

fn print_discovered(trainer: &BleTrainer) {
    let found = trainer.discovered();
    if found.is_empty() {
        println!("no trainers found");
        return;
    }
    println!("{} trainers", found.len());
    for device in found {
        let services = advertised_services(&device);
        match device.rssi_dbm {
            Some(rssi) => {
                println!("{}  {rssi} dBm  {}  [{services}]", device.id, device.name);
            }
            None => println!("{}  {}  [{services}]", device.id, device.name),
        }
    }
}

fn print_straps(heart: &Result<Box<dyn HeartRateLink>, String>) {
    let heart = match heart {
        Ok(heart) => heart,
        Err(err) => {
            eprintln!("heart-rate: {err}");
            println!("no heart-rate straps found");
            return;
        }
    };
    if let ConnectionState::Failed(reason) = heart.state() {
        eprintln!("heart-rate: {reason}");
    }
    let found = heart.discovered();
    if found.is_empty() {
        println!("no heart-rate straps found");
        return;
    }
    println!("{} heart-rate straps", found.len());
    for device in found {
        match device.rssi_dbm {
            Some(rssi) => println!("{}  {rssi} dBm  {}", device.id, device.name),
            None => println!("{}  {}", device.id, device.name),
        }
    }
}

fn advertised_services(device: &pp_trainer::DiscoveredTrainer) -> String {
    let mut parts = Vec::new();
    if device.ftms {
        parts.push("ftms");
    }
    if device.cycling_power {
        parts.push("power");
    }
    if device.fec {
        parts.push("fe-c");
    }
    parts.join(",")
}

fn matches_query(device: &pp_trainer::DiscoveredTrainer, query: &str) -> bool {
    let needle = query.to_ascii_lowercase();
    device.id.eq_ignore_ascii_case(query) || device.name.to_ascii_lowercase().contains(&needle)
}

fn wait_until_connected(trainer: &BleTrainer, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match trainer.state() {
            ConnectionState::Connected => return Ok(()),
            ConnectionState::Failed(reason) => return Err(reason),
            _ if Instant::now() >= deadline => return Err("timed out connecting".into()),
            _ => thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn format_sample(sample: &TrainerSample) -> String {
    let power = match sample.power_w {
        Some(watts) => format!("{watts} W"),
        None => "-".to_string(),
    };
    let cadence = match sample.cadence_rpm {
        Some(rpm) => format!("{rpm:.1} rpm"),
        None => "-".to_string(),
    };
    let speed = match sample.speed_kph {
        Some(kph) => format!("{kph:.2} km/h"),
        None => "-".to_string(),
    };
    let heart = match sample.heart_rate_bpm {
        Some(bpm) => format!("{bpm} bpm"),
        None => "-".to_string(),
    };
    format!("power={power} cadence={cadence} speed={speed} hr={heart}")
}
