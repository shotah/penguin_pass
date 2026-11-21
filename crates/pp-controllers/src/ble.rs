//! Linux BlueZ session for Zwift controllers.
//!
//! The game thread never waits on D-Bus. A background thread owns a Tokio
//! runtime and the btleplug adapters. Shutdown follows `pp-trainer`: stop the
//! links, drop the adapter event stream, wait for BlueZ to remove the D-Bus
//! match, then stop the scan and drop the adapter. Dropping the stream after
//! the adapter is gone panics inside bluez-async (`No match with that id found`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use btleplug::api::{
    Central, CentralEvent, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::StreamExt;
use tokio::sync::{Notify, mpsc::UnboundedReceiver, oneshot};
use tokio::task::JoinHandle as TokioJoin;

use crate::protocol::{self, DeviceIdentity, InputDecoder, ProtocolEvent};
use crate::{
    ControllerEvent, ControllerLink, DiscoveredController, LinkState, RIDE_ON, Side,
    ZWIFT_ASYNC_CHAR, ZWIFT_MANUFACTURER_ID, ZWIFT_SYNC_RX_CHAR, ZWIFT_SYNC_TX_CHAR,
};

const RECONNECT_AFTER: Duration = Duration::from_secs(1);
const MAX_QUEUED_EVENTS: usize = 512;

struct Shared {
    state: Mutex<LinkState>,
    discovered: Mutex<Vec<DiscoveredController>>,
    events: Mutex<std::collections::VecDeque<ControllerEvent>>,
    connected: AtomicUsize,
    inflight: AtomicUsize,
    want: AtomicBool,
}

impl Shared {
    fn new() -> Self {
        Self {
            state: Mutex::new(LinkState::Scanning),
            discovered: Mutex::new(Vec::new()),
            events: Mutex::new(std::collections::VecDeque::new()),
            connected: AtomicUsize::new(0),
            inflight: AtomicUsize::new(0),
            want: AtomicBool::new(false),
        }
    }

    fn state(&self) -> LinkState {
        lock(&self.state).clone()
    }

    fn set_state(&self, state: LinkState) {
        *lock(&self.state) = state;
    }

    fn discovered(&self) -> Vec<DiscoveredController> {
        lock(&self.discovered).clone()
    }

    fn set_discovered(&self, found: Vec<DiscoveredController>) {
        *lock(&self.discovered) = found;
    }

    fn push(&self, event: ControllerEvent) {
        let mut queue = lock(&self.events);
        if queue.len() >= MAX_QUEUED_EVENTS {
            queue.pop_front();
        }
        queue.push_back(event);
    }

    fn drain(&self) -> Vec<ControllerEvent> {
        lock(&self.events).drain(..).collect()
    }

    fn set_want(&self, want: bool) {
        self.want.store(want, Ordering::SeqCst);
    }

    fn wants(&self) -> bool {
        self.want.load(Ordering::SeqCst)
    }

    fn fail(&self, reason: &str) {
        let mut state = lock(&self.state);
        if !matches!(*state, LinkState::Unavailable) {
            *state = LinkState::Failed(reason.to_string());
        }
    }

    fn publish(&self) {
        let have_devices = !lock(&self.discovered).is_empty();
        let connected = self.connected.load(Ordering::SeqCst);
        let inflight = self.inflight.load(Ordering::SeqCst);
        let want = self.wants();
        let mut state = lock(&self.state);
        if matches!(*state, LinkState::Failed(_) | LinkState::Unavailable) {
            return;
        }
        *state = if connected > 0 {
            LinkState::Connected(connected)
        } else if want && (inflight > 0 || have_devices) {
            LinkState::Connecting
        } else {
            LinkState::Scanning
        };
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

enum Cmd {
    ConnectAll,
    Disconnect,
    Shutdown,
}

struct Live {
    stop: Option<oneshot::Sender<()>>,
    task: TokioJoin<()>,
    stopping: bool,
}

struct BleControllers {
    shared: Arc<Shared>,
    cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<Cmd>>,
    notify: Arc<Notify>,
    thread: Option<JoinHandle<()>>,
}

impl BleControllers {
    fn spawn() -> Result<Self, String> {
        let shared = Arc::new(Shared::new());
        let notify = Arc::new(Notify::new());
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let shared_thread = Arc::clone(&shared);
        let notify_thread = Arc::clone(&notify);
        let thread = std::thread::Builder::new()
            .name("pp-controllers".into())
            .spawn(move || worker_main(shared_thread, notify_thread, cmd_rx))
            .map_err(|err| err.to_string())?;
        Ok(Self {
            shared,
            cmd_tx: Some(cmd_tx),
            notify,
            thread: Some(thread),
        })
    }

    fn send(&self, command: Cmd) -> Result<(), ()> {
        let Some(tx) = &self.cmd_tx else {
            return Err(());
        };
        tx.send(command).map_err(|_| ())?;
        self.notify.notify_one();
        Ok(())
    }
}

impl ControllerLink for BleControllers {
    fn state(&self) -> LinkState {
        self.shared.state()
    }

    fn discovered(&self) -> Vec<DiscoveredController> {
        self.shared.discovered()
    }

    fn connect_all(&mut self) {
        self.shared.set_want(true);
        if self.send(Cmd::ConnectAll).is_err() {
            self.shared.fail("bluetooth stopped");
        }
    }

    fn disconnect(&mut self) {
        self.shared.set_want(false);
        if self.send(Cmd::Disconnect).is_err() {
            self.shared.fail("bluetooth stopped");
        }
    }

    fn drain_events(&mut self) -> Vec<ControllerEvent> {
        self.shared.drain()
    }
}

impl Drop for BleControllers {
    fn drop(&mut self) {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(Cmd::Shutdown);
        }
        self.notify.notify_one();
        if let Some(handle) = self.thread.take() {
            join_timeout(handle, Duration::from_secs(3));
        }
    }
}

pub(crate) fn spawn() -> Result<Box<dyn ControllerLink>, String> {
    Ok(Box::new(BleControllers::spawn()?))
}

fn join_timeout(handle: JoinHandle<()>, timeout: Duration) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = handle.join();
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(timeout);
}

fn worker_main(shared: Arc<Shared>, notify: Arc<Notify>, cmd_rx: UnboundedReceiver<Cmd>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("pp-controllers-rt")
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            shared.set_state(LinkState::Failed(err.to_string()));
            return;
        }
    };
    let shared_run = Arc::clone(&shared);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(run(shared_run, notify, cmd_rx));
    }));
    if panicked.is_err() {
        shared.fail("bluetooth thread panicked");
    }
}

async fn run(shared: Arc<Shared>, notify: Arc<Notify>, mut cmd_rx: UnboundedReceiver<Cmd>) {
    let manager = match Manager::new().await {
        Ok(manager) => manager,
        Err(err) => {
            shared.fail(&err.to_string());
            return;
        }
    };
    let adapter = match first_adapter(&manager).await {
        Ok(adapter) => adapter,
        Err(err) => {
            if err == "no bluetooth adapter" {
                shared.set_state(LinkState::Unavailable);
            } else {
                shared.fail(&err);
            }
            return;
        }
    };
    if let Err(err) = adapter.start_scan(ScanFilter::default()).await {
        shared.fail(&err.to_string());
        return;
    }
    shared.set_state(LinkState::Scanning);
    let mut events = match adapter.events().await {
        Ok(events) => events,
        Err(err) => {
            let _ = adapter.stop_scan().await;
            shared.fail(&err.to_string());
            return;
        }
    };

    let mut live: HashMap<String, Live> = HashMap::new();
    let mut retries: HashMap<String, Instant> = HashMap::new();

    loop {
        if take_commands(&mut cmd_rx, &shared, &mut live, &mut retries) {
            break;
        }
        tokio::select! {
            cmd = cmd_rx.recv() => {
                if apply_command(cmd, &shared, &mut live, &mut retries) {
                    break;
                }
            }
            event = events.next() => {
                if let Some(event) = event
                    && advertisement_matters(&event)
                {
                    refresh_discovered(&adapter, &shared).await;
                }
            }
            () = notify.notified() => {}
            () = tokio::time::sleep(Duration::from_millis(250)) => {
                refresh_discovered(&adapter, &shared).await;
            }
        }
        if take_commands(&mut cmd_rx, &shared, &mut live, &mut retries) {
            break;
        }
        reap(&mut live, &mut retries, shared.wants());
        if shared.wants() {
            connect_new(&adapter, &shared, &mut live, &mut retries).await;
        } else {
            stop_sessions(&mut live);
        }
        shared.publish();
    }

    stop_sessions(&mut live);
    finish_tasks(&mut live).await;
    // Drop the event stream before the adapter. See the module note.
    drop(events);
    tokio::time::sleep(Duration::from_millis(250)).await;
    let _ = adapter.stop_scan().await;
    let mut state = lock(&shared.state);
    if !matches!(*state, LinkState::Failed(_) | LinkState::Unavailable) {
        *state = LinkState::Idle;
    }
}

fn take_commands(
    cmd_rx: &mut UnboundedReceiver<Cmd>,
    shared: &Shared,
    live: &mut HashMap<String, Live>,
    retries: &mut HashMap<String, Instant>,
) -> bool {
    while let Ok(cmd) = cmd_rx.try_recv() {
        if apply_command(Some(cmd), shared, live, retries) {
            return true;
        }
    }
    false
}

fn apply_command(
    cmd: Option<Cmd>,
    shared: &Shared,
    live: &mut HashMap<String, Live>,
    retries: &mut HashMap<String, Instant>,
) -> bool {
    match cmd {
        Some(Cmd::ConnectAll) => shared.set_want(true),
        Some(Cmd::Disconnect) => {
            shared.set_want(false);
            retries.clear();
            stop_sessions(live);
        }
        Some(Cmd::Shutdown) | None => return true,
    }
    false
}

fn stop_sessions(live: &mut HashMap<String, Live>) {
    for slot in live.values_mut() {
        signal_stop(slot);
    }
}

fn signal_stop(slot: &mut Live) {
    slot.stopping = true;
    if let Some(tx) = slot.stop.take() {
        let _ = tx.send(());
    }
}

fn reap(live: &mut HashMap<String, Live>, retries: &mut HashMap<String, Instant>, want: bool) {
    let finished: Vec<String> = live
        .iter()
        .filter(|(_, slot)| slot.task.is_finished())
        .map(|(id, _)| id.clone())
        .collect();
    for id in finished {
        let Some(slot) = live.remove(&id) else {
            continue;
        };
        if want && !slot.stopping {
            retries.insert(id, Instant::now() + RECONNECT_AFTER);
        }
    }
}

async fn finish_tasks(live: &mut HashMap<String, Live>) {
    stop_sessions(live);
    let mut pending: Vec<TokioJoin<()>> = live.drain().map(|(_, slot)| slot.task).collect();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        pending.retain(|task| !task.is_finished());
        if pending.is_empty() {
            return;
        }
        if Instant::now() >= deadline {
            for task in pending {
                task.abort();
            }
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn connect_new(
    adapter: &Adapter,
    shared: &Arc<Shared>,
    live: &mut HashMap<String, Live>,
    retries: &mut HashMap<String, Instant>,
) {
    let found = shared.discovered();
    if found.is_empty() {
        return;
    }
    let Ok(mut peripherals) = adapter.peripherals().await else {
        return;
    };
    for device in found {
        if live.contains_key(&device.id) {
            continue;
        }
        if retries
            .get(&device.id)
            .is_some_and(|when| Instant::now() < *when)
        {
            continue;
        }
        let Some(index) = index_of(&peripherals, &device.id).await else {
            continue;
        };
        retries.remove(&device.id);
        let peripheral = peripherals.swap_remove(index);
        let (stop_tx, stop_rx) = oneshot::channel();
        let task = tokio::spawn(run_session(
            peripheral,
            DeviceIdentity {
                kind: device.kind,
                side: device.side,
            },
            device.id.clone(),
            device.name.clone(),
            Arc::clone(shared),
            stop_rx,
        ));
        live.insert(
            device.id,
            Live {
                stop: Some(stop_tx),
                task,
                stopping: false,
            },
        );
    }
}

async fn index_of(peripherals: &[Peripheral], id: &str) -> Option<usize> {
    for (index, peripheral) in peripherals.iter().enumerate() {
        let Ok(Some(props)) = peripheral.properties().await else {
            continue;
        };
        if props.address.to_string() == id {
            return Some(index);
        }
    }
    None
}

async fn run_session(
    peripheral: Peripheral,
    identity: DeviceIdentity,
    id: String,
    name: String,
    shared: Arc<Shared>,
    mut stop: oneshot::Receiver<()>,
) {
    let mut guard = FlightGuard::begin(Arc::clone(&shared), id, name);
    tokio::select! {
        result = session_body(&peripheral, &mut guard, identity) => {
            if let Err(err) = result {
                eprintln!(
                    "pp-controllers: connect failed \"{}\": {err}",
                    guard.name
                );
            }
        }
        _ = &mut stop => {}
    }
    cleanup(&peripheral).await;
}

async fn session_body(
    peripheral: &Peripheral,
    guard: &mut FlightGuard,
    identity: DeviceIdentity,
) -> Result<(), String> {
    let mut notes = open_link(peripheral).await?;
    guard.mark_connected(identity);
    let mut decoder = InputDecoder::new();
    while let Some(note) = notes.next().await {
        if note.uuid != ZWIFT_ASYNC_CHAR {
            continue;
        }
        for event in decoder.push(identity, &note.value) {
            guard.push_protocol(event);
        }
    }
    drop(notes);
    Ok(())
}

async fn open_link(
    peripheral: &Peripheral,
) -> Result<impl futures_util::Stream<Item = btleplug::api::ValueNotification> + use<>, String> {
    if !peripheral
        .is_connected()
        .await
        .map_err(|err| err.to_string())?
    {
        match tokio::time::timeout(Duration::from_secs(10), peripheral.connect()).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(err.to_string()),
            Err(_) => return Err("connection timed out".to_string()),
        }
    }
    peripheral
        .discover_services()
        .await
        .map_err(|err| err.to_string())?;
    let chars = peripheral.characteristics();
    let async_char = chars
        .iter()
        .find(|characteristic| characteristic.uuid == ZWIFT_ASYNC_CHAR)
        .cloned()
        .ok_or_else(|| "missing zwift async characteristic".to_string())?;
    let sync_tx = chars
        .iter()
        .find(|characteristic| characteristic.uuid == ZWIFT_SYNC_TX_CHAR)
        .cloned();
    let sync_rx = chars
        .iter()
        .find(|characteristic| characteristic.uuid == ZWIFT_SYNC_RX_CHAR)
        .cloned()
        .ok_or_else(|| "missing zwift sync rx characteristic".to_string())?;
    peripheral
        .subscribe(&async_char)
        .await
        .map_err(|err| err.to_string())?;
    if let Some(sync_tx) = sync_tx.as_ref() {
        let _ = peripheral.subscribe(sync_tx).await;
    }
    let write_type = if sync_rx
        .properties
        .contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
    {
        WriteType::WithoutResponse
    } else {
        WriteType::WithResponse
    };
    peripheral
        .write(&sync_rx, RIDE_ON, write_type)
        .await
        .map_err(|err| err.to_string())?;
    peripheral
        .notifications()
        .await
        .map_err(|err| err.to_string())
}

async fn cleanup(peripheral: &Peripheral) {
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        for characteristic in peripheral.characteristics() {
            let _ = peripheral.unsubscribe(&characteristic).await;
        }
        let _ = peripheral.disconnect().await;
    })
    .await;
}

struct FlightGuard {
    shared: Arc<Shared>,
    id: String,
    name: String,
    connected: bool,
}

impl FlightGuard {
    fn begin(shared: Arc<Shared>, id: String, name: String) -> Self {
        shared.inflight.fetch_add(1, Ordering::SeqCst);
        shared.publish();
        Self {
            shared,
            id,
            name,
            connected: false,
        }
    }

    fn mark_connected(&mut self, identity: DeviceIdentity) {
        if self.connected {
            return;
        }
        self.connected = true;
        self.shared.connected.fetch_add(1, Ordering::SeqCst);
        eprintln!(
            "pp-controllers: connected \"{}\" id={} kind={} side={}",
            self.name,
            self.id,
            identity.kind,
            side_label(identity.side),
        );
        self.shared.push(ControllerEvent::Connected {
            id: self.id.clone(),
            name: self.name.clone(),
            side: identity.side,
            kind: identity.kind,
        });
        self.shared.publish();
    }

    fn push_protocol(&self, event: ProtocolEvent) {
        let id = self.id.clone();
        let stamped = match event {
            ProtocolEvent::Pressed { side, button } => {
                ControllerEvent::Pressed { id, side, button }
            }
            ProtocolEvent::Released { side, button } => {
                ControllerEvent::Released { id, side, button }
            }
            ProtocolEvent::Paddle { side, axis, raw } => ControllerEvent::Paddle {
                id,
                side,
                axis,
                raw,
            },
        };
        self.shared.push(stamped);
    }
}

impl Drop for FlightGuard {
    fn drop(&mut self) {
        self.shared.inflight.fetch_sub(1, Ordering::SeqCst);
        if self.connected {
            self.shared.connected.fetch_sub(1, Ordering::SeqCst);
            eprintln!(
                "pp-controllers: disconnected \"{}\" id={}",
                self.name, self.id
            );
            self.shared.push(ControllerEvent::Disconnected {
                id: self.id.clone(),
                name: self.name.clone(),
            });
        }
        self.shared.publish();
    }
}

fn side_label(side: Option<Side>) -> &'static str {
    match side {
        Some(side) => side.label(),
        None => "none",
    }
}

async fn first_adapter(manager: &Manager) -> Result<Adapter, String> {
    let mut adapters = manager.adapters().await.map_err(|err| err.to_string())?;
    adapters.pop().ok_or_else(|| "no bluetooth adapter".into())
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
        let Some(identity) = identity_of(&props) else {
            continue;
        };
        let id = props.address.to_string();
        found.push(DiscoveredController {
            name: device_name(&props, &id),
            id,
            rssi_dbm: props.rssi,
            kind: identity.kind,
            side: identity.side,
        });
    }
    sort_discovered(&mut found);
    shared.set_discovered(found);
}

fn identity_of(props: &btleplug::api::PeripheralProperties) -> Option<DeviceIdentity> {
    let bytes = props.manufacturer_data.get(&ZWIFT_MANUFACTURER_ID)?;
    protocol::identify(ZWIFT_MANUFACTURER_ID, bytes)
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

pub(crate) fn sort_discovered(found: &mut [DiscoveredController]) {
    found.sort_by(|left, right| {
        let rssi = right
            .rssi_dbm
            .unwrap_or(i16::MIN)
            .cmp(&left.rssi_dbm.unwrap_or(i16::MIN));
        rssi.then_with(|| left.id.cmp(&right.id))
    });
}

fn advertisement_matters(event: &CentralEvent) -> bool {
    matches!(
        event,
        CentralEvent::DeviceDiscovered(_)
            | CentralEvent::DeviceUpdated(_)
            | CentralEvent::ManufacturerDataAdvertisement { .. }
            | CentralEvent::ServiceDataAdvertisement { .. }
            | CentralEvent::ServicesAdvertisement { .. }
            | CentralEvent::RssiUpdate { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{DeviceKind, Side};

    fn device(id: &str, rssi: Option<i16>) -> DiscoveredController {
        DiscoveredController {
            id: id.to_string(),
            name: id.to_string(),
            rssi_dbm: rssi,
            kind: DeviceKind::Play,
            side: Some(Side::Left),
        }
    }

    #[test]
    fn strongest_signal_is_listed_first() {
        let mut found = vec![
            device("BB", Some(-80)),
            device("AA", None),
            device("CC", Some(-40)),
            device("DD", Some(-40)),
        ];
        sort_discovered(&mut found);
        let ids: Vec<_> = found.into_iter().map(|device| device.id).collect();
        assert_eq!(ids, ["CC", "DD", "BB", "AA"]);
    }
}
