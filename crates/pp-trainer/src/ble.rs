//! Linux BlueZ trainer.
//!
//! The game thread never waits on D-Bus. A background thread owns a Tokio
//! runtime and the btleplug connection. [`BleTrainer`] sends it commands and
//! reads the latest sample out of shared state.

use std::collections::BTreeSet;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use btleplug::api::{
    Central, CentralEvent, CharPropFlags, Manager as _, Peripheral as _, ScanFilter,
    ValueNotification, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::StreamExt;
use pp_core::{TrainerCommand, TrainerSample};
use pp_ftms::{
    CSC_MEASUREMENT_UUID, CYCLING_POWER_MEASUREMENT_UUID, CYCLING_POWER_SERVICE_UUID,
    FEC_NOTIFY_UUID, FEC_SERVICE_UUID, FEC_WRITE_UUID, FITNESS_MACHINE_CONTROL_POINT_UUID,
    FTMS_SERVICE_UUID, HEART_RATE_MEASUREMENT_UUID, INDOOR_BIKE_DATA_UUID,
    WAHOO_TRAINER_CONTROL_UUID, ZWIFT_TRAINER_NOTIFY_UUID, ZWIFT_TRAINER_RESPONSE_UUID,
    ZWIFT_TRAINER_WRITE_UUID,
};
use tokio::sync::{Notify, mpsc::UnboundedReceiver};
use uuid::Uuid;

use crate::config::TrainerConfig;
use crate::error::TrainerError;
use crate::session::{DeviceProfile, Engine, FieldClocks, InEvent, OutAction};
use crate::{ConnectionState, SessionMode, Trainer, lock, sample_is_fresh};

/// A trainer seen while scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredTrainer {
    /// Bluetooth address, for example `AA:BB:CC:DD:EE:FF`.
    pub id: String,
    /// Advertised name, or the address when the peripheral has none.
    pub name: String,
    /// Received signal strength, in dBm, when the adapter reported it.
    pub rssi_dbm: Option<i16>,
    /// Advertises the Fitness Machine Service (0x1826).
    pub ftms: bool,
    /// Advertises the Cycling Power Service (0x1818).
    pub cycling_power: bool,
    /// Advertises the Tacx FE-C service (`6e40fec1-…`).
    pub fec: bool,
}

struct Timed<T> {
    value: T,
    at: Instant,
}

struct LiveFields {
    speed_kph: Option<Timed<f32>>,
    cadence_rpm: Option<Timed<f32>>,
    power_w: Option<Timed<u16>>,
    heart_rate_bpm: Option<Timed<u8>>,
}

impl LiveFields {
    fn sample(&self, staleness: Duration) -> Option<TrainerSample> {
        let sample = TrainerSample {
            power_w: fresh_timed(&self.power_w, staleness),
            cadence_rpm: fresh_timed(&self.cadence_rpm, staleness),
            speed_kph: fresh_timed(&self.speed_kph, staleness),
            heart_rate_bpm: fresh_timed(&self.heart_rate_bpm, staleness),
        };
        if sample.power_w.is_none()
            && sample.cadence_rpm.is_none()
            && sample.speed_kph.is_none()
            && sample.heart_rate_bpm.is_none()
        {
            None
        } else {
            Some(sample)
        }
    }
}

fn fresh_timed<T: Copy>(slot: &Option<Timed<T>>, staleness: Duration) -> Option<T> {
    let timed = slot.as_ref()?;
    if sample_is_fresh(timed.at.elapsed(), staleness) {
        Some(timed.value)
    } else {
        None
    }
}

struct Shared {
    state: Mutex<ConnectionState>,
    name: Mutex<String>,
    live: Mutex<LiveFields>,
    warning: Mutex<Option<String>>,
    settled: Mutex<bool>,
    discovered: Mutex<Vec<DiscoveredTrainer>>,
    pending: Mutex<Option<TrainerCommand>>,
    staleness: Duration,
    /// Short service ids from the last GATT discovery, for the connect log line.
    services: Mutex<String>,
}

impl Shared {
    fn new(staleness: Duration) -> Self {
        Self {
            state: Mutex::new(ConnectionState::Scanning),
            name: Mutex::new(String::from("FTMS")),
            live: Mutex::new(LiveFields {
                speed_kph: None,
                cadence_rpm: None,
                power_w: None,
                heart_rate_bpm: None,
            }),
            warning: Mutex::new(None),
            settled: Mutex::new(true),
            discovered: Mutex::new(Vec::new()),
            pending: Mutex::new(None),
            staleness,
            services: Mutex::new(String::new()),
        }
    }

    fn set_state(&self, state: ConnectionState) {
        *lock(&self.state) = state;
    }

    fn state(&self) -> ConnectionState {
        lock(&self.state).clone()
    }

    fn set_name(&self, name: String) {
        *lock(&self.name) = name;
    }

    fn name(&self) -> String {
        lock(&self.name).clone()
    }

    fn sample(&self) -> Option<TrainerSample> {
        lock(&self.live).sample(self.staleness)
    }

    fn set_warning(&self, warning: Option<String>) {
        *lock(&self.warning) = warning;
    }

    fn warning(&self) -> Option<String> {
        lock(&self.warning).clone()
    }

    fn set_settled(&self, settled: bool) {
        *lock(&self.settled) = settled;
    }

    fn settled(&self) -> bool {
        *lock(&self.settled)
    }

    fn set_discovered(&self, found: Vec<DiscoveredTrainer>) {
        *lock(&self.discovered) = found;
    }

    fn discovered(&self) -> Vec<DiscoveredTrainer> {
        lock(&self.discovered).clone()
    }

    fn has_pending(&self) -> bool {
        lock(&self.pending).is_some()
    }

    fn set_pending(&self, command: TrainerCommand) {
        *lock(&self.pending) = Some(command);
    }

    fn take_pending(&self) -> Option<TrainerCommand> {
        lock(&self.pending).take()
    }

    fn set_services(&self, services: String) {
        *lock(&self.services) = services;
    }

    fn services(&self) -> String {
        lock(&self.services).clone()
    }
}

enum ControlCmd {
    Connect(String),
    Shutdown,
}

enum OnceStop {
    Shutdown,
    Switch(String),
    Lost,
}

/// A Bluetooth FTMS trainer on BlueZ.
///
/// [`BleTrainer::spawn`] starts scanning immediately. [`BleTrainer::connect`]
/// queues a connection and returns. The background thread performs service
/// discovery, subscriptions, and the control-point handshake. Drop the value
/// to unsubscribe, disconnect, and join that thread.
pub struct BleTrainer {
    shared: Arc<Shared>,
    cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<ControlCmd>>,
    notify: Arc<Notify>,
    thread: Option<JoinHandle<()>>,
    mode: SessionMode,
}

impl BleTrainer {
    /// Scan for FTMS trainers and accept control commands once connected.
    ///
    /// # Errors
    ///
    /// Returns an error when the background thread cannot be started. A missing
    /// adapter shows up later as [`ConnectionState::Failed`].
    pub fn spawn() -> Result<Self, TrainerError> {
        Self::spawn_with(TrainerConfig::default())
    }

    /// [`BleTrainer::spawn`] with explicit timing and a [`SessionMode`].
    ///
    /// # Errors
    ///
    /// Returns an error when the background thread cannot be started.
    pub fn spawn_with(config: TrainerConfig) -> Result<Self, TrainerError> {
        let mode = config.mode;
        let trace = crate::trace::BleTrace::from_env();
        let shared = Arc::new(Shared::new(config.staleness));
        let notify = Arc::new(Notify::new());
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let shared_thread = Arc::clone(&shared);
        let notify_thread = Arc::clone(&notify);
        let thread = std::thread::Builder::new()
            .name("pp-trainer".into())
            .spawn(move || worker_main(config, trace, shared_thread, notify_thread, cmd_rx))
            .map_err(|err| TrainerError::Message(err.to_string()))?;
        Ok(Self {
            shared,
            cmd_tx: Some(cmd_tx),
            notify,
            thread: Some(thread),
            mode,
        })
    }

    /// Trainers advertising FTMS, Cycling Power, or Tacx FE-C. Does not block.
    #[must_use]
    pub fn discovered(&self) -> Vec<DiscoveredTrainer> {
        self.shared.discovered()
    }

    /// Queue a connection to an address or a case-insensitive name substring.
    ///
    /// Returns when the request is queued. Poll [`Trainer::state`] for
    /// [`ConnectionState::Connected`] or [`ConnectionState::Failed`].
    ///
    /// # Errors
    ///
    /// Returns an error when `id_or_name` is empty or the background thread
    /// has stopped.
    pub fn connect(&self, id_or_name: &str) -> Result<(), TrainerError> {
        let query = id_or_name.trim();
        if query.is_empty() {
            return Err(TrainerError::Message("trainer id or name is empty".into()));
        }
        self.send_control(ControlCmd::Connect(query.to_string()))
    }

    /// True when no control-point write is in flight and no command is queued.
    #[must_use]
    pub fn control_idle(&self) -> bool {
        self.shared.settled()
    }

    fn send_control(&self, command: ControlCmd) -> Result<(), TrainerError> {
        let Some(tx) = &self.cmd_tx else {
            return Err(TrainerError::Stopped);
        };
        tx.send(command).map_err(|_| TrainerError::Stopped)?;
        self.notify.notify_one();
        Ok(())
    }
}

impl Trainer for BleTrainer {
    fn latest_sample(&self) -> Option<TrainerSample> {
        self.shared.sample()
    }

    fn send(&mut self, command: TrainerCommand) -> Result<(), TrainerError> {
        if self.mode == SessionMode::ReadOnly {
            return Err(TrainerError::ReadOnly);
        }
        if self.cmd_tx.as_ref().is_none_or(|tx| tx.is_closed()) {
            return Err(TrainerError::Stopped);
        }
        if !matches!(self.state(), ConnectionState::Connected) {
            return Err(TrainerError::NotConnected);
        }
        self.shared.set_pending(command);
        self.shared.set_settled(false);
        self.notify.notify_one();
        Ok(())
    }

    fn state(&self) -> ConnectionState {
        self.shared.state()
    }

    fn name(&self) -> String {
        self.shared.name()
    }

    fn last_warning(&self) -> Option<String> {
        self.shared.warning()
    }
}

impl Drop for BleTrainer {
    fn drop(&mut self) {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(ControlCmd::Shutdown);
        }
        self.notify.notify_one();
        if let Some(handle) = self.thread.take() {
            join_timeout(handle, Duration::from_secs(3));
        }
    }
}

fn join_timeout(handle: JoinHandle<()>, timeout: Duration) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = handle.join();
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(timeout);
}

fn worker_main(
    config: TrainerConfig,
    trace: crate::trace::BleTrace,
    shared: Arc<Shared>,
    notify: Arc<Notify>,
    cmd_rx: UnboundedReceiver<ControlCmd>,
) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("pp-trainer-rt")
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
        runtime.block_on(run(config, trace, shared_run, notify, cmd_rx));
    }));
    if panicked.is_err() {
        shared.set_state(ConnectionState::Failed(
            "bluetooth thread panicked".to_string(),
        ));
    }
}

async fn run(
    config: TrainerConfig,
    trace: crate::trace::BleTrace,
    shared: Arc<Shared>,
    notify: Arc<Notify>,
    mut cmd_rx: UnboundedReceiver<ControlCmd>,
) {
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
    // An empty filter is what BlueZ treats as "every advertisement". A
    // service-UUID filter hides the Tacx Neo 2T, which advertises Cycling
    // Power and FE-C and does not advertise FTMS. Household devices are
    // dropped afterwards by [`is_trainer`].
    let filter = ScanFilter::default();
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

    let mut engine = Engine::new(config);
    engine.set_trace_log(trace);
    let started = Instant::now();
    let mut pending_query: Option<String> = None;

    loop {
        if take_shutdown(&mut cmd_rx, &mut pending_query) {
            break;
        }
        if let Some(query) = pending_query.take() {
            let _ = adapter.stop_scan().await;
            match resolve(&adapter, &query).await {
                Some((peripheral, name)) => {
                    let ctx = SessionCtx {
                        shared: shared.as_ref(),
                        notify: notify.as_ref(),
                        started,
                    };
                    let stop = maintain(peripheral, name, &mut engine, &ctx, &mut cmd_rx).await;
                    match stop {
                        OnceStop::Shutdown => break,
                        OnceStop::Switch(query) => pending_query = Some(query),
                        OnceStop::Lost => {}
                    }
                }
                None => {
                    let reason = format!("no trainer matches {query}");
                    eprintln!("pp-trainer: connect failed \"{query}\": {reason}");
                    shared.set_state(ConnectionState::Failed(reason));
                }
            }
            if pending_query.is_none() {
                if let Err(err) = adapter.start_scan(filter.clone()).await {
                    shared.set_state(ConnectionState::Failed(err.to_string()));
                    break;
                }
                if !matches!(shared.state(), ConnectionState::Failed(_)) {
                    shared.set_state(ConnectionState::Scanning);
                }
            }
            continue;
        }

        tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                Some(ControlCmd::Connect(query)) => pending_query = Some(query),
                Some(ControlCmd::Shutdown) | None => break,
            },
            event = events.next() => {
                if let Some(event) = event {
                    note_event(&adapter, &shared, &event).await;
                }
            }
            _ = notify.notified() => {}
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                refresh_discovered(&adapter, &shared).await;
            }
        }
        match drain_control(&mut cmd_rx) {
            Drained::Shutdown => break,
            Drained::Connect(query) => pending_query = Some(query),
            Drained::Quiet => {}
        }
    }
    // Drop the event stream before the adapter. bluez-async removes the D-Bus
    // match from `MessageStream::drop` on a spawned task and unwraps the
    // result. If the session is already gone, BlueZ returns "No match with
    // that id found" and that worker panics. The sleep lets the round trip
    // finish while this adapter still holds the session.
    drop(events);
    tokio::time::sleep(Duration::from_millis(250)).await;
    let _ = adapter.stop_scan().await;
    let _ = engine.stop();
    shared.set_state(ConnectionState::Disconnected);
}

struct SessionCtx<'a> {
    shared: &'a Shared,
    notify: &'a Notify,
    started: Instant,
}

async fn maintain(
    mut peripheral: Peripheral,
    name: String,
    engine: &mut Engine,
    ctx: &SessionCtx<'_>,
    cmd_rx: &mut UnboundedReceiver<ControlCmd>,
) -> OnceStop {
    let mut published = FieldClocks::default();
    loop {
        engine.start_connect(elapsed_ms(ctx.started), name.clone());
        publish(engine, ctx.shared, &mut published);
        match run_once(&mut peripheral, engine, ctx, cmd_rx, &mut published).await {
            OnceStop::Shutdown => return OnceStop::Shutdown,
            OnceStop::Switch(query) => return OnceStop::Switch(query),
            OnceStop::Lost => {}
        }
        loop {
            let actions = engine.poll(elapsed_ms(ctx.started));
            let reconnect = actions
                .iter()
                .any(|action| matches!(action, OutAction::Reconnect));
            publish(engine, ctx.shared, &mut published);
            if reconnect {
                break;
            }
            tokio::select! {
                cmd = cmd_rx.recv() => match cmd {
                    Some(ControlCmd::Shutdown) | None => {
                        let _ = engine.stop();
                        return OnceStop::Shutdown;
                    }
                    Some(ControlCmd::Connect(query)) => {
                        let _ = engine.stop();
                        return OnceStop::Switch(query);
                    }
                },
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
        }
    }
}

async fn run_once(
    peripheral: &mut Peripheral,
    engine: &mut Engine,
    ctx: &SessionCtx<'_>,
    cmd_rx: &mut UnboundedReceiver<ControlCmd>,
    published: &mut FieldClocks,
) -> OnceStop {
    ctx.shared.set_state(ConnectionState::Connecting);
    if let Err(err) = peripheral.connect().await {
        let _ = engine.handle(
            elapsed_ms(ctx.started),
            InEvent::ConnectFailed {
                reason: err.to_string(),
            },
        );
        publish(engine, ctx.shared, published);
        return OnceStop::Lost;
    }

    let notifications = match peripheral.notifications().await {
        Ok(stream) => stream,
        Err(err) => {
            let _ = peripheral.disconnect().await;
            fail_connect(engine, ctx, published, err.to_string());
            return OnceStop::Lost;
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

    let stop = session_loop(peripheral, engine, ctx, cmd_rx, published, &mut rx).await;
    if matches!(stop, OnceStop::Lost) {
        let actions = engine.release_flat();
        if !actions.is_empty() {
            let _ = tokio::time::timeout(Duration::from_millis(500), perform(peripheral, &actions))
                .await;
        }
    }
    forward.abort();
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        let chars = peripheral.characteristics();
        for characteristic in &chars {
            let _ = peripheral.unsubscribe(characteristic).await;
        }
        let _ = peripheral.disconnect().await;
    })
    .await;
    stop
}

async fn session_loop(
    peripheral: &mut Peripheral,
    engine: &mut Engine,
    ctx: &SessionCtx<'_>,
    cmd_rx: &mut UnboundedReceiver<ControlCmd>,
    published: &mut FieldClocks,
    rx: &mut UnboundedReceiver<ValueNotification>,
) -> OnceStop {
    if let Err(err) = peripheral.discover_services().await {
        fail_connect(engine, ctx, published, err.to_string());
        return OnceStop::Lost;
    }
    let characteristics = peripheral.characteristics();
    let services = short_services(&characteristics);
    ctx.shared.set_services(services.clone());
    engine.set_services(&services);
    let profile = profile_from(&characteristics);
    let actions = engine.handle(elapsed_ms(ctx.started), InEvent::Ready(profile));
    if let Err(err) = perform(peripheral, &actions).await {
        fail_connect(engine, ctx, published, err);
        return OnceStop::Lost;
    }
    publish(engine, ctx.shared, published);
    if actions
        .iter()
        .any(|action| matches!(action, OutAction::Disconnect))
    {
        return OnceStop::Lost;
    }

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                Some(ControlCmd::Shutdown) | None => {
                    let actions = engine.stop();
                    let _ = perform(peripheral, &actions).await;
                    return OnceStop::Shutdown;
                }
                Some(ControlCmd::Connect(query)) => {
                    let actions = engine.stop();
                    let _ = perform(peripheral, &actions).await;
                    return OnceStop::Switch(query);
                }
            },
            note = rx.recv() => {
                match note {
                    Some(note) => {
                        let actions = engine.handle(elapsed_ms(ctx.started), InEvent::Value {
                            uuid: note.uuid,
                            bytes: note.value,
                        });
                        if let Err(err) = perform(peripheral, &actions).await {
                            fail_connect(engine, ctx, published, err);
                            return OnceStop::Lost;
                        }
                        publish(engine, ctx.shared, published);
                        if actions.iter().any(|action| matches!(action, OutAction::Disconnect)) {
                            return OnceStop::Lost;
                        }
                    }
                    None => {
                        let _ = engine.handle(elapsed_ms(ctx.started), InEvent::LinkLost);
                        publish(engine, ctx.shared, published);
                        return OnceStop::Lost;
                    }
                }
            }
            _ = ctx.notify.notified() => {
                if let Some(stop) = apply_pending(peripheral, engine, ctx, published).await {
                    return stop;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                let actions = engine.poll(elapsed_ms(ctx.started));
                if let Err(err) = perform(peripheral, &actions).await {
                    fail_connect(engine, ctx, published, err);
                    return OnceStop::Lost;
                }
                publish(engine, ctx.shared, published);
                if actions.iter().any(|action| matches!(action, OutAction::Disconnect)) {
                    return OnceStop::Lost;
                }
                if peripheral.is_connected().await.ok() == Some(false) {
                    let _ = engine.handle(elapsed_ms(ctx.started), InEvent::LinkLost);
                    publish(engine, ctx.shared, published);
                    return OnceStop::Lost;
                }
            }
        }
    }
}

async fn apply_pending(
    peripheral: &Peripheral,
    engine: &mut Engine,
    ctx: &SessionCtx<'_>,
    published: &mut FieldClocks,
) -> Option<OnceStop> {
    let command = ctx.shared.take_pending()?;
    let actions = engine.handle(elapsed_ms(ctx.started), InEvent::Command(command));
    if let Err(err) = perform(peripheral, &actions).await {
        fail_connect(engine, ctx, published, err);
        return Some(OnceStop::Lost);
    }
    publish(engine, ctx.shared, published);
    if actions
        .iter()
        .any(|action| matches!(action, OutAction::Disconnect))
    {
        return Some(OnceStop::Lost);
    }
    None
}

fn fail_connect(
    engine: &mut Engine,
    ctx: &SessionCtx<'_>,
    published: &mut FieldClocks,
    reason: String,
) {
    let _ = engine.handle(elapsed_ms(ctx.started), InEvent::ConnectFailed { reason });
    publish(engine, ctx.shared, published);
}

async fn perform(peripheral: &Peripheral, actions: &[OutAction]) -> Result<(), String> {
    for action in actions {
        match action {
            OutAction::Subscribe(uuid) => {
                let characteristic = find_char(peripheral, *uuid)
                    .ok_or_else(|| format!("missing characteristic {uuid}"))?;
                peripheral
                    .subscribe(&characteristic)
                    .await
                    .map_err(|err| err.to_string())?;
            }
            OutAction::WriteControl(bytes) => {
                let characteristic = find_char(peripheral, FITNESS_MACHINE_CONTROL_POINT_UUID)
                    .ok_or_else(|| "missing fitness machine control point".to_string())?;
                peripheral
                    .write(&characteristic, bytes, WriteType::WithResponse)
                    .await
                    .map_err(|err| err.to_string())?;
            }
            OutAction::WriteFec(bytes) => {
                let characteristic = find_char(peripheral, FEC_WRITE_UUID)
                    .ok_or_else(|| "missing tacx fe-c control characteristic".to_string())?;
                let write_type = if characteristic
                    .properties
                    .contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
                {
                    WriteType::WithoutResponse
                } else {
                    WriteType::WithResponse
                };
                peripheral
                    .write(&characteristic, bytes, write_type)
                    .await
                    .map_err(|err| err.to_string())?;
            }
            OutAction::WriteWahoo(bytes) => {
                let characteristic = find_char(peripheral, WAHOO_TRAINER_CONTROL_UUID)
                    .ok_or_else(|| "missing wahoo trainer control".to_string())?;
                let write_type = if characteristic.properties.contains(CharPropFlags::WRITE) {
                    WriteType::WithResponse
                } else {
                    WriteType::WithoutResponse
                };
                peripheral
                    .write(&characteristic, bytes, write_type)
                    .await
                    .map_err(|err| err.to_string())?;
            }
            OutAction::WriteZwift(bytes) => {
                let characteristic = find_char(peripheral, ZWIFT_TRAINER_WRITE_UUID)
                    .ok_or_else(|| "missing zwift trainer write".to_string())?;
                let write_type = if characteristic
                    .properties
                    .contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
                {
                    WriteType::WithoutResponse
                } else {
                    WriteType::WithResponse
                };
                peripheral
                    .write(&characteristic, bytes, write_type)
                    .await
                    .map_err(|err| err.to_string())?;
            }
            OutAction::Disconnect | OutAction::Reconnect => {}
        }
    }
    Ok(())
}

fn find_char(peripheral: &Peripheral, uuid: Uuid) -> Option<btleplug::api::Characteristic> {
    peripheral
        .characteristics()
        .into_iter()
        .find(|characteristic| characteristic.uuid == uuid)
}

fn profile_from(chars: &BTreeSet<btleplug::api::Characteristic>) -> DeviceProfile {
    let has = |uuid: Uuid| {
        chars
            .iter()
            .any(|characteristic| characteristic.uuid == uuid)
    };
    DeviceProfile {
        indoor_bike: has(INDOOR_BIKE_DATA_UUID),
        cycling_power: has(CYCLING_POWER_MEASUREMENT_UUID),
        heart_rate: has(HEART_RATE_MEASUREMENT_UUID),
        control_point: has(FITNESS_MACHINE_CONTROL_POINT_UUID),
        fec_notify: has(FEC_NOTIFY_UUID),
        fec_write: has(FEC_WRITE_UUID),
        csc: has(CSC_MEASUREMENT_UUID),
        wahoo_control: has(WAHOO_TRAINER_CONTROL_UUID),
        zwift_notify: has(ZWIFT_TRAINER_NOTIFY_UUID),
        zwift_write: has(ZWIFT_TRAINER_WRITE_UUID),
        zwift_response: has(ZWIFT_TRAINER_RESPONSE_UUID),
    }
}

fn publish(engine: &Engine, shared: &Shared, published: &mut FieldClocks) {
    let previous = shared.state();
    note_transition(&previous, engine, &shared.services());
    shared.set_state(engine.state());
    let name = engine.name();
    if !name.is_empty() {
        shared.set_name(name.to_string());
    }
    shared.set_warning(engine.warning().map(ToString::to_string));
    shared.set_settled(engine.control_settled() && !shared.has_pending());
    let fields = engine.fields();
    let mut live = lock(&shared.live);
    sync_field(&mut live.speed_kph, published.speed_kph, fields.speed_kph);
    sync_field(
        &mut live.cadence_rpm,
        published.cadence_rpm,
        fields.cadence_rpm,
    );
    sync_field(&mut live.power_w, published.power_w, fields.power_w);
    sync_field(
        &mut live.heart_rate_bpm,
        published.heart_rate_bpm,
        fields.heart_rate_bpm,
    );
    drop(live);
    *published = fields;
}

fn sync_field<T: Copy + PartialEq>(
    slot: &mut Option<Timed<T>>,
    previous: Option<crate::session::FieldClock<T>>,
    next: Option<crate::session::FieldClock<T>>,
) {
    let same = match (previous, next) {
        (Some(before), Some(after)) => before.at_ms == after.at_ms && before.value == after.value,
        (None, None) => true,
        _ => false,
    };
    if same {
        return;
    }
    *slot = next.map(|field| Timed {
        value: field.value,
        at: Instant::now(),
    });
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
        let flags = advertised(&props);
        if !is_trainer(&flags) {
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
        let flags = advertised(&props);
        if !is_trainer(&flags) {
            continue;
        }
        let id = props.address.to_string();
        let name = device_name(&props, &id);
        found.push(DiscoveredTrainer {
            id,
            name,
            rssi_dbm: props.rssi,
            ftms: flags.ftms,
            cycling_power: flags.cycling_power,
            fec: flags.fec,
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

struct Advertised {
    ftms: bool,
    cycling_power: bool,
    fec: bool,
}

fn advertised(props: &btleplug::api::PeripheralProperties) -> Advertised {
    let services: Vec<Uuid> = props.service_data.keys().copied().collect();
    advertised_ids(&props.services, &services)
}

fn advertised_ids(services: &[Uuid], service_data: &[Uuid]) -> Advertised {
    let has = |uuid: Uuid| services.contains(&uuid) || service_data.contains(&uuid);
    Advertised {
        ftms: has(FTMS_SERVICE_UUID),
        cycling_power: has(CYCLING_POWER_SERVICE_UUID),
        fec: has(FEC_SERVICE_UUID),
    }
}

/// A trainer advertises FTMS, Cycling Power, or Tacx FE-C.
///
/// Zwift Play, Click, and Ride controllers advertise `0xFC82` and none of
/// those. They stay off the trainer list.
fn is_trainer(flags: &Advertised) -> bool {
    flags.ftms || flags.cycling_power || flags.fec
}

fn note_transition(previous: &ConnectionState, engine: &Engine, services: &str) {
    let next = engine.state();
    match (previous, &next) {
        (ConnectionState::Connected, ConnectionState::Connected) => {}
        (_, ConnectionState::Connected) => {
            eprintln!(
                "pp-trainer: connected \"{}\" services=[{services}] telemetry={} control={}",
                engine.name(),
                engine.telemetry_label(),
                engine.control_label()
            );
        }
        (ConnectionState::Failed(before), ConnectionState::Failed(after)) if before == after => {}
        (_, ConnectionState::Failed(reason)) => {
            eprintln!("pp-trainer: connect failed \"{}\": {reason}", engine.name());
        }
        _ => {}
    }
}

fn short_services(chars: &BTreeSet<btleplug::api::Characteristic>) -> String {
    let mut ids: Vec<String> = chars
        .iter()
        .map(|characteristic| short_uuid(characteristic.service_uuid))
        .collect();
    ids.sort();
    ids.dedup();
    ids.join(",")
}

fn short_uuid(uuid: Uuid) -> String {
    let value = uuid.as_u128();
    let high = u32::try_from(value >> 96).unwrap_or(u32::MAX);
    let bluetooth_base = 0x0000_1000_8000_0080_5f9b_34fb_u128;
    let rest = value & ((1_u128 << 96) - 1);
    if high <= u32::from(u16::MAX) && rest == bluetooth_base {
        format!("{high:04x}")
    } else {
        format!("{high:08x}")
    }
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

fn take_shutdown(
    cmd_rx: &mut UnboundedReceiver<ControlCmd>,
    pending_query: &mut Option<String>,
) -> bool {
    while let Ok(cmd) = cmd_rx.try_recv() {
        match cmd {
            ControlCmd::Shutdown => return true,
            ControlCmd::Connect(query) => *pending_query = Some(query),
        }
    }
    false
}

enum Drained {
    Shutdown,
    Connect(String),
    Quiet,
}

fn drain_control(cmd_rx: &mut UnboundedReceiver<ControlCmd>) -> Drained {
    let mut query = None;
    while let Ok(cmd) = cmd_rx.try_recv() {
        match cmd {
            ControlCmd::Shutdown => return Drained::Shutdown,
            ControlCmd::Connect(next) => query = Some(next),
        }
    }
    match query {
        Some(query) => Drained::Connect(query),
        None => Drained::Quiet,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_trainer_services_are_listed() {
        let tacx = advertised_ids(
            &[
                pp_ftms::uuid16(0x180A),
                CYCLING_POWER_SERVICE_UUID,
                pp_ftms::uuid16(0x1816),
                FEC_SERVICE_UUID,
            ],
            &[],
        );
        assert!(is_trainer(&tacx));
        assert!(!tacx.ftms);
        assert!(tacx.cycling_power);
        assert!(tacx.fec);

        let ftms = advertised_ids(&[], &[FTMS_SERVICE_UUID]);
        assert!(is_trainer(&ftms));
        assert!(ftms.ftms);

        let dishwasher = advertised_ids(&[pp_ftms::uuid16(0x180A), Uuid::from_u128(0x1234)], &[]);
        assert!(!is_trainer(&dishwasher));
        let headphones = advertised_ids(&[pp_ftms::uuid16(0x180F), pp_ftms::uuid16(0x1812)], &[]);
        assert!(!is_trainer(&headphones));
    }

    #[test]
    fn zwift_controller_service_alone_is_not_a_trainer() {
        let play = advertised_ids(&[pp_ftms::ZWIFT_TRAINER_SERVICE_UUID], &[]);
        assert!(!is_trainer(&play));
        assert!(!play.ftms);
        assert!(!play.cycling_power);
        assert!(!play.fec);

        let click = advertised_ids(&[], &[pp_ftms::ZWIFT_TRAINER_SERVICE_UUID]);
        assert!(!is_trainer(&click));

        let ride = advertised_ids(
            &[pp_ftms::ZWIFT_TRAINER_SERVICE_UUID, FTMS_SERVICE_UUID],
            &[],
        );
        assert!(is_trainer(&ride));
        assert!(ride.ftms);
    }

    #[test]
    fn profile_from_marks_csc_wahoo_and_zwift() {
        let chars = BTreeSet::from([
            characteristic(CYCLING_POWER_MEASUREMENT_UUID),
            characteristic(WAHOO_TRAINER_CONTROL_UUID),
            characteristic(CSC_MEASUREMENT_UUID),
            characteristic(ZWIFT_TRAINER_NOTIFY_UUID),
            characteristic(ZWIFT_TRAINER_WRITE_UUID),
            characteristic(ZWIFT_TRAINER_RESPONSE_UUID),
        ]);
        let profile = profile_from(&chars);
        assert!(profile.cycling_power);
        assert!(profile.wahoo_control);
        assert!(profile.csc);
        assert!(profile.zwift_notify);
        assert!(profile.zwift_write);
        assert!(profile.zwift_response);
        assert!(!profile.indoor_bike);
        assert!(!profile.control_point);
        assert!(!profile.fec_notify);
        assert!(!profile.fec_write);
        assert!(!profile.heart_rate);
    }

    fn characteristic(uuid: Uuid) -> btleplug::api::Characteristic {
        btleplug::api::Characteristic {
            uuid,
            service_uuid: uuid,
            properties: CharPropFlags::empty(),
            descriptors: BTreeSet::new(),
        }
    }

    #[test]
    fn short_uuids_keep_the_sig_number_or_the_vendor_prefix() {
        assert_eq!(short_uuid(FTMS_SERVICE_UUID), "1826");
        assert_eq!(short_uuid(CYCLING_POWER_SERVICE_UUID), "1818");
        assert_eq!(short_uuid(FEC_SERVICE_UUID), "6e40fec1");
        assert_eq!(short_uuid(FEC_NOTIFY_UUID), "6e40fec2");
    }
}
