//! Background thread that owns the HTTP server.

use crate::http::{self, AppState};
use pp_core::Telemetry;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::sync::watch;

/// Port used when [`crate::CastConfig`] is left at its default.
pub const DEFAULT_PORT: u16 = 7878;

const START_TIMEOUT: Duration = Duration::from_secs(5);
const JOIN_TIMEOUT: Duration = Duration::from_secs(3);

/// Why [`CastServer::start`] did not return a running server.
#[derive(Debug)]
pub enum CastError {
    /// The TCP socket could not be bound.
    Bind(io::Error),
    /// The background thread could not be created.
    Spawn(io::Error),
    /// The background thread exited before it reported that it was listening.
    Startup,
}

impl std::fmt::Display for CastError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bind(err) => write!(f, "bind dashboard socket: {err}"),
            Self::Spawn(err) => write!(f, "spawn dashboard thread: {err}"),
            Self::Startup => write!(f, "dashboard thread did not start listening"),
        }
    }
}

impl std::error::Error for CastError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bind(err) | Self::Spawn(err) => Some(err),
            Self::Startup => None,
        }
    }
}

/// HTTP dashboard running on a background thread.
///
/// Dropping the value stops the server. The frame loop should keep it alive
/// for the whole ride.
#[must_use = "the server stops when this value is dropped"]
pub struct CastServer {
    local_addr: SocketAddr,
    telemetry: watch::Sender<Telemetry>,
    /// Keeps the watch channel open so [`Self::publish`] still stores a value
    /// when no browser is connected.
    _telemetry_rx: watch::Receiver<Telemetry>,
    shutdown: Option<watch::Sender<bool>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for CastServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CastServer")
            .field("local_addr", &self.local_addr)
            .finish_non_exhaustive()
    }
}

impl CastServer {
    /// Bind `config.bind` and serve the dashboard until this value is dropped.
    ///
    /// Returns after the socket is listening. Port `0` selects a free port;
    /// read it back with [`Self::local_addr`]. A failure leaves nothing
    /// running.
    ///
    /// # Errors
    ///
    /// Returns [`CastError::Bind`] when the address cannot be bound (for
    /// example the port is already in use), [`CastError::Spawn`] when the
    /// operating system refuses to create the thread, and [`CastError::Startup`]
    /// when that thread exits before it is listening.
    pub fn start(config: crate::CastConfig) -> Result<Self, CastError> {
        let listener = std::net::TcpListener::bind(config.bind).map_err(CastError::Bind)?;
        listener.set_nonblocking(true).map_err(CastError::Bind)?;
        let local_addr = listener.local_addr().map_err(CastError::Bind)?;

        let (telemetry, telemetry_rx) = watch::channel(Telemetry::default());
        let (shutdown, _) = watch::channel(false);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let state = AppState {
            telemetry: telemetry.clone(),
            course: std::sync::Arc::new(config.course),
            shutdown: shutdown.clone(),
        };

        let thread = thread::Builder::new()
            .name("pp-cast".to_owned())
            .spawn(move || server_thread(listener, state, ready_tx))
            .map_err(CastError::Spawn)?;

        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(()) => Ok(Self {
                local_addr,
                telemetry,
                _telemetry_rx: telemetry_rx,
                shutdown: Some(shutdown),
                thread: Some(thread),
            }),
            Err(_) => {
                let _ = shutdown.send(true);
                join_timeout(thread, START_TIMEOUT);
                Err(CastError::Startup)
            }
        }
    }

    /// Store the latest ride snapshot for connected browsers.
    ///
    /// Non-blocking. Safe to call on every frame from the game thread. Only
    /// the newest value is kept; browsers are updated a few times a second.
    pub fn publish(&self, telemetry: Telemetry) {
        self.telemetry.send_replace(telemetry);
    }

    /// Address the server actually bound, including a port chosen by the
    /// operating system when the requested port was `0`.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// URLs a rider can open on a TV or phone.
    ///
    /// When the socket is bound to a specific address, the only URL is that
    /// address. When it is bound to all interfaces, each non-loopback IPv4
    /// address on the machine is returned. Link-local and multicast addresses
    /// are skipped. A loopback URL is used only when the server is bound to
    /// loopback, or when no other usable IPv4 address exists.
    pub fn urls(&self) -> Vec<String> {
        urls_for(self.local_addr, &lan_ipv4())
    }

    /// Stop accepting connections, end event streams, and join the server thread.
    ///
    /// Also runs on drop. Join waits at most a few seconds so a stuck client
    /// cannot hang the game.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(true);
        }
        if let Some(thread) = self.thread.take() {
            join_timeout(thread, JOIN_TIMEOUT);
        }
    }
}

impl Drop for CastServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn server_thread(listener: std::net::TcpListener, state: AppState, ready: mpsc::SyncSender<()>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return,
    };

    runtime.block_on(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(_) => return,
        };
        let mut stop = state.shutdown.subscribe();
        let app = http::router(state);
        if ready.send(()).is_err() {
            return;
        }
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stop.wait_for(|flag| *flag).await;
            })
            .await;
    });
    runtime.shutdown_timeout(Duration::from_millis(200));
}

fn join_timeout(handle: JoinHandle<()>, timeout: Duration) {
    let (done_tx, done_rx) = mpsc::channel();
    let waiter = thread::Builder::new()
        .name("pp-cast-join".to_owned())
        .spawn(move || {
            let _ = handle.join();
            let _ = done_tx.send(());
        });
    let Ok(waiter) = waiter else {
        return;
    };
    if done_rx.recv_timeout(timeout).is_ok() {
        let _ = waiter.join();
    }
}

fn urls_for(bound: SocketAddr, lan_ipv4: &[Ipv4Addr]) -> Vec<String> {
    let port = bound.port();
    match bound.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() => {
            return vec![format!("http://{ip}:{port}/")];
        }
        IpAddr::V6(ip) if !ip.is_unspecified() => {
            return vec![format!("http://[{ip}]:{port}/")];
        }
        _ => {}
    }

    let mut hosts: Vec<Ipv4Addr> = lan_ipv4.iter().copied().filter(|ip| is_lan(*ip)).collect();
    hosts.sort_unstable();
    hosts.dedup();
    if hosts.is_empty() {
        vec![format!("http://127.0.0.1:{port}/")]
    } else {
        hosts
            .into_iter()
            .map(|ip| format!("http://{ip}:{port}/"))
            .collect()
    }
}

fn lan_ipv4() -> Vec<Ipv4Addr> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut hosts = Vec::new();
    for interface in interfaces {
        if let if_addrs::IfAddr::V4(addr) = interface.addr
            && is_lan(addr.ip)
        {
            hosts.push(addr.ip);
        }
    }
    hosts
}

fn is_lan(ip: Ipv4Addr) -> bool {
    !ip.is_unspecified() && !ip.is_loopback() && !ip.is_link_local() && !ip.is_multicast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specific_bind_is_the_only_url() {
        let bound = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 20), 7878));
        let urls = urls_for(bound, &[Ipv4Addr::new(10, 0, 0, 1)]);
        assert_eq!(urls, vec!["http://192.168.1.20:7878/".to_owned()]);
    }

    #[test]
    fn loopback_bind_does_not_advertise_lan_addresses() {
        let bound = SocketAddr::from((Ipv4Addr::LOCALHOST, 9));
        let urls = urls_for(bound, &[Ipv4Addr::new(10, 0, 0, 8)]);
        assert_eq!(urls, vec!["http://127.0.0.1:9/".to_owned()]);
    }

    #[test]
    fn wildcard_bind_lists_lan_addresses_and_skips_loopback() {
        let bound = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 7878));
        let urls = urls_for(
            bound,
            &[
                Ipv4Addr::new(192, 168, 1, 20),
                Ipv4Addr::LOCALHOST,
                Ipv4Addr::new(10, 0, 0, 4),
                Ipv4Addr::new(169, 254, 1, 1),
                Ipv4Addr::new(192, 168, 1, 20),
            ],
        );
        assert_eq!(
            urls,
            vec![
                "http://10.0.0.4:7878/".to_owned(),
                "http://192.168.1.20:7878/".to_owned(),
            ]
        );
    }

    #[test]
    fn wildcard_bind_falls_back_to_loopback() {
        let bound = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 7878));
        let urls = urls_for(bound, &[Ipv4Addr::LOCALHOST, Ipv4Addr::new(169, 254, 4, 4)]);
        assert_eq!(urls, vec!["http://127.0.0.1:7878/".to_owned()]);
    }
}
