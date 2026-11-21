//! Read-only Bluetooth heart-rate strap.
//!
//! This session is separate from the trainer's. Each has its own thread,
//! Tokio runtime, and BlueZ connection. BlueZ merges discovery filters, so
//! the two scans can run together. Sharing one adapter object would tie
//! their shutdown together; a dropped strap must not take the trainer with it.
//!
//! The only write is enabling notifications on Heart Rate Measurement.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::StreamExt;
use pp_ftms::{HEART_RATE_MEASUREMENT_UUID, HEART_RATE_SERVICE_UUID};
use tokio::sync::{Notify, mpsc::UnboundedReceiver};
use uuid::Uuid;

use crate::heart::{Engine, InEvent, OutAction};
use crate::{ConnectionState, DiscoveredSensor, HeartRateLink, lock, sample_is_fresh};

struct TimedBpm {
    bpm: u16,
    at: Instant,
    engine_at: u64,
}

struct Shared {
    state: Mutex<ConnectionState>,
    name: Mutex<Option<String>>,
    bpm: Mutex<Option<TimedBpm>>,
    discovered: Mutex<Vec<DiscoveredSensor>>,
    staleness: Duration,
}

impl Shared {
    fn new() -> Self {
        Self {
            state: Mutex::new(ConnectionState::Scanning),
            name: Mutex::new(None),
            bpm: Mutex::new(None),
            discovered: Mutex::new(Vec::new()),
            staleness: crate::SAMPLE_STALENESS,
        }
    }

    fn state(&self) -> ConnectionState {
        lock(&self.state).clone()
    }

    fn set_state(&self, state: ConnectionState) {
        *lock(&self.state) = state;
    }

    fn name(&self) -> Option<String> {
        lock(&self.name).clone()
    }

    fn set_name(&self, name: Option<String>) {
        *lock(&self.name) = name;
    }

    fn bpm(&self) -> Option<u16> {
        let slot = lock(&self.bpm);
        let timed = slot.as_ref()?;
        if sample_is_fresh(timed.at.elapsed(), self.staleness) {
            Some(timed.bpm)
        } else {
            None
        }
    }

    fn set_sample(&self, bpm: Option<u16>, engine_at: Option<u64>) {
        let mut slot = lock(&self.bpm);
        match (bpm, engine_at) {
            (Some(bpm), Some(engine_at)) => {
                let same = slot
                    .as_ref()
                    .is_some_and(|timed| timed.engine_at == engine_at && timed.bpm == bpm);
                if !same {
                    *slot = Some(TimedBpm {
                        bpm,
                        at: Instant::now(),
                        engine_at,
                    });
                }
            }
            _ => *slot = None,
        }
    }

    fn discovered(&self) -> Vec<DiscoveredSensor> {
        lock(&self.discovered).clone()
    }

    fn set_discovered(&self, found: Vec<DiscoveredSensor>) {
        *lock(&self.discovered) = found;
    }
}

enum Cmd {
    Connect(String),
    Disconnect,
    Shutdown,
}

enum Stop {
    Shutdown,
    Disconnected,
    Lost,
    Switch(String),
}

/// A Bluetooth heart-rate strap on BlueZ.
///
/// [`BleHeartRate::spawn`] starts scanning immediately. Drop the value to
/// disconnect and join the background thread.
pub struct BleHeartRate {
    shared: Arc<Shared>,
    cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<Cmd>>,
    notify: Arc<Notify>,
    thread: Option<JoinHandle<()>>,
}

impl BleHeartRate {
    /// Scan for straps that advertise the Heart Rate Service.
    ///
    /// # Errors
    ///
    /// Returns an error when the background thread cannot be started. A missing
    /// adapter shows up later as [`ConnectionState::Failed`].
    pub fn spawn() -> Result<Self, String> {
        let shared = Arc::new(Shared::new());
        let notify = Arc::new(Notify::new());
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let shared_thread = Arc::clone(&shared);
        let notify_thread = Arc::clone(&notify);
        let thread = std::thread::Builder::new()
            .name("pp-heart".into())
            .spawn(move || worker_main(shared_thread, notify_thread, cmd_rx))
            .map_err(|err| err.to_string())?;
        Ok(Self {
            shared,
            cmd_tx: Some(cmd_tx),
            notify,
            thread: Some(thread),
        })
    }
}

impl HeartRateLink for BleHeartRate {
    fn state(&self) -> ConnectionState {
        self.shared.state()
    }

    fn discovered(&self) -> Vec<DiscoveredSensor> {
        self.shared.discovered()
    }

    fn connect(&mut self, id_or_name: &str) {
        let query = id_or_name.trim();
        if query.is_empty() {
            self.shared.set_state(ConnectionState::Failed(
                "heart-rate strap id or name is empty".into(),
            ));
            return;
        }
        self.shared.set_state(ConnectionState::Connecting);
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(Cmd::Connect(query.to_string()));
        }
        self.notify.notify_one();
    }

    fn disconnect(&mut self) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(Cmd::Disconnect);
        }
        self.notify.notify_one();
    }

    fn bpm(&self) -> Option<u16> {
        self.shared.bpm()
    }

    fn name(&self) -> Option<String> {
        self.shared.name()
    }
}

impl Drop for BleHeartRate {
    fn drop(&mut self) {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(Cmd::Shutdown);
        }
        self.notify.notify_one();
        if let Some(handle) = self.thread.take() {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = handle.join();
                let _ = tx.send(());
            });
            let _ = rx.recv_timeout(Duration::from_secs(3));
        }
    }
}

fn worker_main(shared: Arc<Shared>, notify: Arc<Notify>, cmd_rx: UnboundedReceiver<Cmd>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("pp-heart-rt")
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            shared.set_state(ConnectionState::Failed(err.to_string()));
            return;
        }
    };
    let shared_run = Arc::clone(&shared);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(run(shared_run, notify, cmd_rx));
    }));
    if panicked.is_err() {
        shared.set_state(ConnectionState::Failed(
            "heart-rate thread panicked".to_string(),
        ));
    }
}

async fn run(shared: Arc<Shared>, notify: Arc<Notify>, mut cmd_rx: UnboundedReceiver<Cmd>) {
    let _setup = crate::enter_bluetooth_setup().await;
    let manager = match Manager::new().await {
        Ok(manager) => manager,
        Err(err) => {
            shared.set_state(ConnectionState::Failed(err.to_string()));
            return;
        }
    };
    let adapter = match first_adapter(&manager).await {
        Ok(adapter) => adapter,
        Err(err) => {
            shared.set_state(ConnectionState::Failed(err));
            return;
        }
    };
    let filter = ScanFilter {
        services: vec![HEART_RATE_SERVICE_UUID],
    };
    if let Err(err) = adapter.start_scan(filter.clone()).await {
        shared.set_state(ConnectionState::Failed(err.to_string()));
        return;
    }
    shared.set_state(ConnectionState::Scanning);
    let mut events = match adapter.events().await {
        Ok(events) => events,
        Err(err) => {
            let _ = adapter.stop_scan().await;
            shared.set_state(ConnectionState::Failed(err.to_string()));
            return;
        }
    };
    drop(_setup);

    let mut engine = Engine::new();
    let started = Instant::now();
    let mut pending: Option<Cmd> = None;

    loop {
        if take_shutdown(&mut cmd_rx, &mut pending) {
            break;
        }
        if let Some(cmd) = pending.take() {
            match cmd {
                Cmd::Connect(query) => {
                    let _ = adapter.stop_scan().await;
                    match resolve(&adapter, &query).await {
                        Some((peripheral, name)) => {
                            let stop = maintain(
                                peripheral,
                                name,
                                &mut engine,
                                &shared,
                                &notify,
                                &mut cmd_rx,
                                started,
                            )
                            .await;
                            match stop {
                                Stop::Shutdown => break,
                                Stop::Switch(query) => pending = Some(Cmd::Connect(query)),
                                Stop::Disconnected | Stop::Lost => {}
                            }
                        }
                        None => {
                            let _ = engine.handle(
                                elapsed_ms(started),
                                InEvent::NotFound {
                                    query: query.clone(),
                                },
                            );
                            publish(&engine, &shared, elapsed_ms(started));
                        }
                    }
                    if let Err(err) = adapter.start_scan(filter.clone()).await {
                        shared.set_state(ConnectionState::Failed(err.to_string()));
                        break;
                    }
                    if !matches!(shared.state(), ConnectionState::Failed(_)) {
                        if matches!(engine.state(), ConnectionState::Disconnected) {
                            shared.set_state(ConnectionState::Disconnected);
                        } else if !matches!(engine.state(), ConnectionState::Failed(_)) {
                            shared.set_state(ConnectionState::Scanning);
                        }
                    }
                }
                Cmd::Disconnect => {
                    let actions = engine.handle(elapsed_ms(started), InEvent::Disconnect);
                    let _ = actions;
                    publish(&engine, &shared, elapsed_ms(started));
                }
                Cmd::Shutdown => break,
            }
            continue;
        }

        tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                Some(cmd @ (Cmd::Connect(_) | Cmd::Disconnect)) => pending = Some(cmd),
                Some(Cmd::Shutdown) | None => break,
            },
            event = events.next() => {
                if let Some(event) = event {
                    note_event(&adapter, &shared, &event).await;
                }
            }
            () = notify.notified() => {}
            () = tokio::time::sleep(Duration::from_millis(250)) => {
                refresh_discovered(&adapter, &shared).await;
            }
        }
    }

    // Drop the event stream before the adapter so bluez-async can remove its
    // D-Bus match while the session still exists. See the trainer scan loop.
    drop(events);
    tokio::time::sleep(Duration::from_millis(250)).await;
    let _ = adapter.stop_scan().await;
    shared.set_state(ConnectionState::Disconnected);
}

async fn maintain(
    mut peripheral: Peripheral,
    name: String,
    engine: &mut Engine,
    shared: &Shared,
    notify: &Notify,
    cmd_rx: &mut UnboundedReceiver<Cmd>,
    started: Instant,
) -> Stop {
    loop {
        let _ = engine.handle(
            elapsed_ms(started),
            InEvent::Resolved { name: name.clone() },
        );
        publish(engine, shared, elapsed_ms(started));
        match run_once(&mut peripheral, engine, shared, notify, cmd_rx, started).await {
            Stop::Shutdown => return Stop::Shutdown,
            Stop::Disconnected => return Stop::Disconnected,
            Stop::Switch(query) => return Stop::Switch(query),
            Stop::Lost => {}
        }
        loop {
            let now = elapsed_ms(started);
            let actions = engine.poll(now);
            publish(engine, shared, now);
            if actions
                .iter()
                .any(|action| matches!(action, OutAction::Reconnect))
            {
                break;
            }
            tokio::select! {
                cmd = cmd_rx.recv() => match cmd {
                    Some(Cmd::Shutdown) | None => return Stop::Shutdown,
                    Some(Cmd::Disconnect) => {
                        let _ = engine.handle(elapsed_ms(started), InEvent::Disconnect);
                        publish(engine, shared, elapsed_ms(started));
                        return Stop::Disconnected;
                    }
                    Some(Cmd::Connect(query)) => {
                        let _ = engine.handle(elapsed_ms(started), InEvent::Disconnect);
                        publish(engine, shared, elapsed_ms(started));
                        return Stop::Switch(query);
                    }
                },
                () = notify.notified() => {}
                () = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
        }
    }
}

async fn run_once(
    peripheral: &mut Peripheral,
    engine: &mut Engine,
    shared: &Shared,
    notify: &Notify,
    cmd_rx: &mut UnboundedReceiver<Cmd>,
    started: Instant,
) -> Stop {
    if let Err(err) = peripheral.connect().await {
        let _ = engine.handle(
            elapsed_ms(started),
            InEvent::ConnectFailed {
                reason: err.to_string(),
            },
        );
        publish(engine, shared, elapsed_ms(started));
        return Stop::Lost;
    }
    let notifications = match peripheral.notifications().await {
        Ok(stream) => stream,
        Err(err) => {
            let _ = peripheral.disconnect().await;
            let _ = engine.handle(
                elapsed_ms(started),
                InEvent::ConnectFailed {
                    reason: err.to_string(),
                },
            );
            publish(engine, shared, elapsed_ms(started));
            return Stop::Lost;
        }
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let forward = tokio::spawn(async move {
        let mut notifications = notifications;
        while let Some(note) = notifications.next().await {
            if tx.send(note).is_err() {
                break;
            }
        }
    });

    let stop = session_loop(peripheral, engine, shared, notify, cmd_rx, &mut rx, started).await;
    forward.abort();
    let _ = peripheral.disconnect().await;
    stop
}

async fn session_loop(
    peripheral: &mut Peripheral,
    engine: &mut Engine,
    shared: &Shared,
    notify: &Notify,
    cmd_rx: &mut UnboundedReceiver<Cmd>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<btleplug::api::ValueNotification>,
    started: Instant,
) -> Stop {
    if let Err(err) = peripheral.discover_services().await {
        let _ = engine.handle(
            elapsed_ms(started),
            InEvent::ConnectFailed {
                reason: err.to_string(),
            },
        );
        publish(engine, shared, elapsed_ms(started));
        return Stop::Lost;
    }
    let measurement = peripheral
        .characteristics()
        .into_iter()
        .any(|characteristic| characteristic.uuid == HEART_RATE_MEASUREMENT_UUID);
    let actions = engine.handle(elapsed_ms(started), InEvent::Ready { measurement });
    if let Err(err) = perform(peripheral, &actions).await {
        let _ = engine.handle(elapsed_ms(started), InEvent::ConnectFailed { reason: err });
        publish(engine, shared, elapsed_ms(started));
        return Stop::Lost;
    }
    publish(engine, shared, elapsed_ms(started));
    if actions
        .iter()
        .any(|action| matches!(action, OutAction::Disconnect))
    {
        return Stop::Lost;
    }

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                Some(Cmd::Shutdown) | None => return Stop::Shutdown,
                Some(Cmd::Disconnect) => {
                    let _ = engine.handle(elapsed_ms(started), InEvent::Disconnect);
                    publish(engine, shared, elapsed_ms(started));
                    return Stop::Disconnected;
                }
                Some(Cmd::Connect(query)) => {
                    let _ = engine.handle(elapsed_ms(started), InEvent::Disconnect);
                    publish(engine, shared, elapsed_ms(started));
                    return Stop::Switch(query);
                }
            },
            note = rx.recv() => {
                match note {
                    Some(note) if note.uuid == HEART_RATE_MEASUREMENT_UUID => {
                        let _ = engine.handle(elapsed_ms(started), InEvent::Value(note.value));
                        publish(engine, shared, elapsed_ms(started));
                    }
                    Some(_) => {}
                    None => {
                        let _ = engine.handle(elapsed_ms(started), InEvent::LinkLost);
                        publish(engine, shared, elapsed_ms(started));
                        return Stop::Lost;
                    }
                }
            }
            () = notify.notified() => {}
            () = tokio::time::sleep(Duration::from_millis(250)) => {
                if peripheral.is_connected().await.ok() == Some(false) {
                    let _ = engine.handle(elapsed_ms(started), InEvent::LinkLost);
                    publish(engine, shared, elapsed_ms(started));
                    return Stop::Lost;
                }
                publish(engine, shared, elapsed_ms(started));
            }
        }
    }
}

async fn perform(peripheral: &Peripheral, actions: &[OutAction]) -> Result<(), String> {
    for action in actions {
        match action {
            OutAction::Subscribe => {
                let characteristic = peripheral
                    .characteristics()
                    .into_iter()
                    .find(|characteristic| characteristic.uuid == HEART_RATE_MEASUREMENT_UUID)
                    .ok_or_else(|| "missing heart rate measurement".to_string())?;
                peripheral
                    .subscribe(&characteristic)
                    .await
                    .map_err(|err| err.to_string())?;
            }
            OutAction::Disconnect | OutAction::Reconnect => {}
        }
    }
    Ok(())
}

fn publish(engine: &Engine, shared: &Shared, now_ms: u64) {
    let previous = shared.state();
    let next = engine.state();
    note_transition(&previous, &next, engine.label());
    shared.set_state(next);
    shared.set_name(engine.connected_name().map(str::to_string));
    shared.set_sample(engine.bpm(now_ms), engine.sample_at_ms(now_ms));
}

fn note_transition(previous: &ConnectionState, next: &ConnectionState, name: &str) {
    match (previous, next) {
        (ConnectionState::Connected, ConnectionState::Connected) => {}
        (_, ConnectionState::Connected) => {
            eprintln!("pp-trainer: heart-rate connected \"{name}\"");
        }
        (ConnectionState::Failed(before), ConnectionState::Failed(after)) if before == after => {}
        (_, ConnectionState::Failed(reason)) => {
            eprintln!("pp-trainer: heart-rate connect failed \"{name}\": {reason}");
        }
        _ => {}
    }
}

async fn first_adapter(manager: &Manager) -> Result<Adapter, String> {
    let mut adapters = manager.adapters().await.map_err(|err| err.to_string())?;
    adapters
        .pop()
        .ok_or_else(|| "no bluetooth adapter".to_string())
}

async fn resolve(adapter: &Adapter, query: &str) -> Option<(Peripheral, String)> {
    let peripherals = adapter.peripherals().await.ok()?;
    let needle = query.to_ascii_lowercase();
    for peripheral in peripherals {
        let Some(props) = peripheral.properties().await.ok().flatten() else {
            continue;
        };
        if !advertises_heart_rate(
            &props.services,
            &props.service_data.keys().copied().collect::<Vec<_>>(),
        ) {
            continue;
        }
        let id = props.address.to_string();
        let name = device_name(&props, &id);
        let id_match = id.eq_ignore_ascii_case(query);
        let name_match = name.to_ascii_lowercase().contains(&needle);
        if id_match || name_match {
            return Some((peripheral, name));
        }
    }
    None
}

async fn refresh_discovered(adapter: &Adapter, shared: &Shared) {
    let Ok(peripherals) = adapter.peripherals().await else {
        return;
    };
    let mut found = Vec::new();
    for peripheral in peripherals {
        let Ok(Some(props)) = peripheral.properties().await else {
            continue;
        };
        let service_data: Vec<Uuid> = props.service_data.keys().copied().collect();
        if !advertises_heart_rate(&props.services, &service_data) {
            continue;
        }
        let id = props.address.to_string();
        found.push(DiscoveredSensor {
            name: device_name(&props, &id),
            id,
            rssi_dbm: props.rssi,
        });
    }
    found.sort_by_key(|device| std::cmp::Reverse(device.rssi_dbm.unwrap_or(i16::MIN)));
    shared.set_discovered(found);
}

async fn note_event(adapter: &Adapter, shared: &Shared, event: &CentralEvent) {
    match event {
        CentralEvent::DeviceDiscovered(_)
        | CentralEvent::DeviceUpdated(_)
        | CentralEvent::ServicesAdvertisement { .. }
        | CentralEvent::ServiceDataAdvertisement { .. }
        | CentralEvent::RssiUpdate { .. } => refresh_discovered(adapter, shared).await,
        _ => {}
    }
}

pub(crate) fn advertises_heart_rate(services: &[Uuid], service_data: &[Uuid]) -> bool {
    services.contains(&HEART_RATE_SERVICE_UUID) || service_data.contains(&HEART_RATE_SERVICE_UUID)
}

fn device_name(props: &btleplug::api::PeripheralProperties, id: &str) -> String {
    props
        .local_name
        .clone()
        .filter(|name| !name.is_empty())
        .or_else(|| {
            props
                .advertisement_name
                .clone()
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| id.to_string())
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn take_shutdown(cmd_rx: &mut UnboundedReceiver<Cmd>, pending: &mut Option<Cmd>) -> bool {
    while let Ok(cmd) = cmd_rx.try_recv() {
        match cmd {
            Cmd::Shutdown => return true,
            other => *pending = Some(other),
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_heart_rate_service_is_listed() {
        assert!(advertises_heart_rate(&[HEART_RATE_SERVICE_UUID], &[]));
        assert!(advertises_heart_rate(&[], &[HEART_RATE_SERVICE_UUID]));
        assert!(!advertises_heart_rate(
            &[pp_ftms::FTMS_SERVICE_UUID, pp_ftms::uuid16(0x180A)],
            &[]
        ));
        let dishwasher = Uuid::from_u128(0x6e40_0001_b5a3_f393_e0a9_e50e_24dc_ca9e);
        assert!(!advertises_heart_rate(&[dishwasher], &[]));
    }
}
