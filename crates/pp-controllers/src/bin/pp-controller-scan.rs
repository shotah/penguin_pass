//! List nearby Zwift controllers. Does not connect, pair, or write.

use std::env;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use pp_controllers::{LinkState, spawn_ble_controllers};

fn main() -> ExitCode {
    let seconds = match parse_seconds(env::args().skip(1)) {
        Ok(seconds) => seconds,
        Err(err) => {
            if !err.is_empty() {
                eprintln!("{err}");
            }
            eprintln!("usage: pp-controller-scan [--scan-seconds N]");
            return ExitCode::from(2);
        }
    };

    let link = match spawn_ble_controllers() {
        Ok(link) => link,
        Err(err) => {
            eprintln!("could not start bluetooth: {err}");
            return ExitCode::from(1);
        }
    };

    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        match link.state() {
            LinkState::Failed(reason) => {
                eprintln!("bluetooth: {reason}");
                return ExitCode::from(1);
            }
            LinkState::Unavailable => {
                eprintln!("bluetooth: no adapter");
                return ExitCode::from(1);
            }
            _ => thread::sleep(Duration::from_millis(200)),
        }
    }
    match link.state() {
        LinkState::Failed(reason) => {
            eprintln!("bluetooth: {reason}");
            return ExitCode::from(1);
        }
        LinkState::Unavailable => {
            eprintln!("bluetooth: no adapter");
            return ExitCode::from(1);
        }
        _ => {}
    }

    let found = link.discovered();
    if found.is_empty() {
        println!("no controllers found");
        return ExitCode::SUCCESS;
    }
    println!("{} controllers", found.len());
    for device in found {
        let side = device.side.map_or("unspecified", |side| side.label());
        match device.rssi_dbm {
            Some(rssi) => println!(
                "{}  {rssi} dBm  {}  {}  {side}",
                device.id, device.name, device.kind
            ),
            None => println!("{}  {}  {}  {side}", device.id, device.name, device.kind),
        }
    }
    ExitCode::SUCCESS
}

fn parse_seconds(args: impl Iterator<Item = String>) -> Result<u64, String> {
    let mut seconds = 8;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scan-seconds" => {
                let text = args
                    .next()
                    .filter(|value| !value.starts_with("--"))
                    .ok_or_else(|| "--scan-seconds needs a value".to_string())?;
                seconds = text
                    .parse::<u64>()
                    .map_err(|_| format!("--scan-seconds {text} is not a whole number"))?;
            }
            "--help" | "-h" => return Err(String::new()),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(seconds)
}
