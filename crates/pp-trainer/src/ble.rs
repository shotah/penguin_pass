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
    Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter, ValueNotification, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::StreamExt;
use pp_core::{TrainerCommand, TrainerSample};
use pp_ftms::{
    CYCLING_POWER_MEASUREMENT_UUID, FITNESS_MACHINE_CONTROL_POINT_UUID, FTMS_SERVICE_UUID,
    HEART_RATE_MEASUREMENT_UUID, INDOOR_BIKE_DATA_UUID,
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
        let shared = Arc::new(Shared::new(config.staleness));
        let notify = Arc::new(Notify::new());
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let shared_thread = Arc::clone(&shared);
        let notify_thread = Arc::clone(&notify);
        let thread = std::thread::Builder::new()
            .name("pp-trainer".into())
            .spawn(move || worker_main(config, shared_thread, notify_thread, cmd_rx))
            .map_err(|err| TrainerError::Message(err.to_string()))?;
        Ok(Self {
            shared,
            cmd_tx: Some(cmd_tx),
            notify,
            thread: Some(thread),
            mode,
        })
    }

    /// Trainers advertising the Fitness Machine Service. Does not block.
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
    runtime.block_on(run(config, shared, notify, cmd_rx));
}

async fn run(
    config: TrainerConfig,
    shared: Arc<Shared>,
    notify: Arc<Notify>,
    mut cmd_rx: UnboundedReceiver<ControlCmd>,
) {
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
        services: vec![FTMS_SERVICE_UUID],
    };
    if let Err(err) = adapter.start_scan(filter.clone()).await {
        shared.set_state(ConnectionState::Failed(err.to_string()));
        return;
    }
    shared.set_state(ConnectionState::Scanning);
    let mut events = match adapter.events().await {
        Ok(events) => events,
        Err(err) => {
            shared.set_state(ConnectionState::Failed(err.to_string()));
            return;
        }
    };

    let mut engine = Engine::new(config);
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
                    shared.set_state(ConnectionState::Failed(format!(
                        "no trainer matches {query}"
                    )));
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
    let _ = adapter.stop_scan().await;
    engine.stop();
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
        engine.start_connect(name.clone());
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
                        engine.stop();
                        return OnceStop::Shutdown;
                    }
                    Some(ControlCmd::Connect(query)) => {
                        engine.stop();
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
    let profile = profile_from(&peripheral.characteristics());
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
                    engine.stop();
                    return OnceStop::Shutdown;
                }
                Some(ControlCmd::Connect(query)) => {
                    engine.stop();
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
    }
}

fn publish(engine: &Engine, shared: &Shared, published: &mut FieldClocks) {
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
        if !advertises_ftms(&props) {
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
        if !advertises_ftms(&props) {
            continue;
        }
        let id = props.address.to_string();
        let name = device_name(&props, &id);
        found.push(DiscoveredTrainer {
            id,
            name,
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

fn advertises_ftms(props: &btleplug::api::PeripheralProperties) -> bool {
    props.services.contains(&FTMS_SERVICE_UUID)
        || props.service_data.contains_key(&FTMS_SERVICE_UUID)
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
