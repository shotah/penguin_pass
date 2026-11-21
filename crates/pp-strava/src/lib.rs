//! Strava connect and ride upload for Penguin Pass.
//!
//! [`StravaLink`] keeps the player's Strava application keys and tokens in
//! one TOML file and uploads finished FIT rides. It does not link Godot.
//! Every method returns at once; a background thread does the network work.
//! Call [`StravaLink::drain_events`] from the game loop.
//!
//! The Strava origins are [`Endpoints`]. Tests point them at a local server.
//! Nothing in this crate contacts Strava by itself until the game asks it to.
//!
//! ```
//! let path = std::env::temp_dir().join("pp-strava-doctest.toml");
//! let _ = std::fs::remove_file(&path);
//! let link = pp_strava::StravaLink::open(path, pp_strava::Endpoints::strava());
//! assert!(matches!(link.state(), pp_strava::StravaState::NoKeys));
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tiny_http::Request;

mod api;
mod pages;
mod store;

use api::{ApiError, UploadProgress};
use store::Store;

const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const POLL_BUDGET: Duration = Duration::from_secs(120);
const POLL_FIRST: Duration = Duration::from_secs(1);
const CONNECT_BUDGET: Duration = Duration::from_secs(5 * 60);
const REFRESH_WITHIN_SECS: u64 = 5 * 60;
const PREFERRED_PORTS: [u16; 5] = [53_682, 53_683, 53_684, 53_685, 53_686];

const CORRUPT_WARNING: &str =
    "pp-strava: the Strava store could not be read; starting with no keys";
const SAVE_WARNING: &str = "pp-strava: could not save the Strava store";

/// Strava origins. The defaults are the real site.
///
/// `oauth_base` is the site origin used for `/oauth/authorize` and
/// `/oauth/token`. `api_base` is the origin used for `/api/v3/uploads`.
/// Both default to `https://www.strava.com`. A trailing slash is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// Origin for the OAuth authorize and token calls.
    pub oauth_base: String,
    /// Origin for `/api/v3/` calls.
    pub api_base: String,
}

impl Endpoints {
    /// The public Strava origins.
    #[must_use]
    pub fn strava() -> Self {
        Self {
            oauth_base: "https://www.strava.com".to_owned(),
            api_base: "https://www.strava.com".to_owned(),
        }
    }
}

impl Default for Endpoints {
    fn default() -> Self {
        Self::strava()
    }
}

/// The player's own Strava application keys.
///
/// Debug output redacts `client_secret`.
#[derive(Clone)]
pub struct Keys {
    /// Application id from the Strava API settings page.
    pub client_id: String,
    /// Application secret. This stays in the local store.
    pub client_secret: String,
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Keys")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

/// What the game can show without waiting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StravaState {
    /// The store has no client id and secret.
    NoKeys,
    /// Keys are saved. Strava has not been approved, or the login was dropped.
    NotConnected,
    /// The browser flow is in progress. `url` is the page to open.
    Connecting {
        /// Form page, or the Strava authorize page.
        url: String,
    },
    /// A refresh token is saved. `athlete` is the name to show.
    Connected {
        /// Athlete name from the token response, or a short fallback.
        athlete: String,
    },
    /// The local connect page could not be opened.
    Error(String),
}

/// What happened since the last [`StravaLink::drain_events`].
///
/// These values never include the client secret, access token, or refresh token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StravaEvent {
    /// Keys were written to the store.
    KeysSaved,
    /// The browser flow finished and tokens are saved.
    Connected {
        /// Athlete name from the token response.
        athlete: String,
    },
    /// The browser flow stopped without a saved login.
    ConnectFailed {
        /// A short reason safe to show.
        reason: String,
    },
    /// Tokens were forgotten. Keys are still in the store.
    Disconnected,
    /// An upload request has started. One attempt follows.
    UploadStarted {
        /// The path passed to [`StravaLink::upload`].
        file: PathBuf,
    },
    /// Strava accepted the ride.
    Uploaded {
        /// The path passed to [`StravaLink::upload`].
        file: PathBuf,
        /// Strava activity id.
        activity_id: u64,
        /// `https://www.strava.com/activities/{activity_id}`.
        url: String,
    },
    /// This attempt did not produce a new activity.
    ///
    /// `duplicate` is true when Strava says the file was already uploaded.
    /// That ride is still recorded in [`StravaLink::was_uploaded`].
    UploadFailed {
        /// The path passed to [`StravaLink::upload`].
        file: PathBuf,
        /// A short reason safe to show.
        reason: String,
        /// Strava reported this file as a duplicate.
        duplicate: bool,
    },
}

/// Name and description sent with one FIT file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadMeta {
    /// Activity title.
    pub name: String,
    /// Activity description. May be empty.
    pub description: String,
}

/// One Strava connection for the game.
///
/// The value owns a background thread. Dropping it stops that thread and the
/// loopback listener. A request already in flight runs until its timeout.
pub struct StravaLink {
    shared: Arc<Mutex<Shared>>,
    tx: Sender<Cmd>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for StravaLink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = lock(&self.shared);
        formatter
            .debug_struct("StravaLink")
            .field("path", &guard.path)
            .field("endpoints", &guard.endpoints)
            .field("state", &guard.state)
            .finish()
    }
}

impl StravaLink {
    /// Open or create the store at `store_path` and start the background thread.
    ///
    /// A missing file means [`StravaState::NoKeys`] and is not created yet.
    /// A file that does not parse also means [`StravaState::NoKeys`]. A warning
    /// is printed, and the file is left unchanged.
    #[must_use]
    pub fn open(store_path: PathBuf, endpoints: Endpoints) -> Self {
        let (store, corrupt) = store::load(&store_path);
        let mut shared = Shared::new(store_path, endpoints, store);
        if corrupt {
            shared.note(CORRUPT_WARNING);
        }
        shared.state = shared.rest_state();
        let shared = Arc::new(Mutex::new(shared));
        let (tx, rx) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("pp-strava".to_owned())
            .spawn(move || worker(worker_shared, rx))
            .expect("pp-strava thread");
        Self {
            shared,
            tx,
            thread: Some(thread),
        }
    }

    /// Current state. This does not wait.
    #[must_use]
    pub fn state(&self) -> StravaState {
        lock(&self.shared).state.clone()
    }

    /// Save application keys.
    ///
    /// Empty values are ignored. When the id or the secret changes, saved
    /// tokens are dropped. The same keys leave an existing login in place.
    pub fn set_keys(&self, keys: Keys) {
        let client_id = keys.client_id.trim().to_owned();
        let client_secret = keys.client_secret.trim().to_owned();
        if client_id.is_empty() || client_secret.is_empty() {
            return;
        }
        let mut guard = lock(&self.shared);
        let changed =
            guard.store.client_id != client_id || guard.store.client_secret != client_secret;
        guard.store.client_id = client_id;
        guard.store.client_secret = client_secret;
        if changed {
            guard.store.clear_tokens();
            if !matches!(guard.state, StravaState::Connecting { .. }) {
                guard.state = guard.rest_state();
            }
        }
        guard.warn_save();
        guard.events.push(StravaEvent::KeysSaved);
    }

    /// Start the browser flow.
    ///
    /// With keys, the returned URL is Strava's authorize page. Without keys,
    /// it is a local form that saves the keys and then sends the browser on.
    /// The listener waits up to five minutes. Calling this again while a
    /// flow is already running returns that same URL.
    ///
    /// `None` means the loopback port could not be opened.
    #[must_use]
    pub fn begin_connect(&self) -> Option<String> {
        let mut guard = lock(&self.shared);
        if guard.shutdown {
            return None;
        }
        if let StravaState::Connecting { url } = &guard.state {
            return Some(url.clone());
        }
        let Some((server, port)) = bind_listener() else {
            let reason = "Could not open a local page for the Strava connection.".to_owned();
            guard.state = StravaState::Error(reason.clone());
            guard.events.push(StravaEvent::ConnectFailed { reason });
            return None;
        };
        let redirect = format!("http://127.0.0.1:{port}/callback");
        let (url, oauth_state) = if guard.store.has_keys() {
            let Some(state) = random_state() else {
                let reason = "Could not start the Strava connection.".to_owned();
                guard.state = StravaState::Error(reason.clone());
                guard.events.push(StravaEvent::ConnectFailed { reason });
                return None;
            };
            let url =
                api::authorize_url(&guard.endpoints, &guard.store.client_id, &redirect, &state);
            (url, state)
        } else {
            (format!("http://127.0.0.1:{port}/"), String::new())
        };
        guard.redirect = redirect;
        guard.oauth_state = oauth_state;
        guard.listen_port = Some(port);
        guard.connect_deadline = Some(Instant::now() + guard.connect_budget);
        guard.stop_listener = false;
        guard.pending_server = Some(server);
        guard.state = StravaState::Connecting { url: url.clone() };
        drop(guard);
        let _ = self.tx.send(Cmd::Wake);
        Some(url)
    }

    /// Stop the browser flow and leave the saved keys and tokens as they are.
    pub fn cancel_connect(&self) {
        let port = {
            let mut guard = lock(&self.shared);
            if !matches!(guard.state, StravaState::Connecting { .. }) {
                return;
            }
            guard.stop_listener = true;
            guard.oauth_state.clear();
            guard.connect_deadline = None;
            guard.state = guard.rest_state();
            guard.listen_port
        };
        self.wake(port);
    }

    /// Forget tokens and keep the application keys.
    pub fn disconnect(&self) {
        let port = {
            let mut guard = lock(&self.shared);
            guard.store.clear_tokens();
            guard.warn_save();
            guard.stop_listener = true;
            guard.oauth_state.clear();
            guard.connect_deadline = None;
            guard.state = guard.rest_state();
            guard.events.push(StravaEvent::Disconnected);
            guard.listen_port
        };
        self.wake(port);
    }

    /// Queue one upload. The result arrives later as an event.
    ///
    /// The call does not retry. A duplicate reported by Strava is stored as
    /// uploaded and emitted as [`StravaEvent::UploadFailed`] with `duplicate`.
    pub fn upload(&self, file: PathBuf, meta: UploadMeta) {
        lock(&self.shared).uploads.push_back(Job { file, meta });
        let _ = self.tx.send(Cmd::Wake);
    }

    /// Whether this file name was uploaded, including a duplicate.
    ///
    /// The comparison uses the file name, not the directory.
    #[must_use]
    pub fn was_uploaded(&self, file: &Path) -> bool {
        let label = api::file_label(file);
        lock(&self.shared).store.was_uploaded(&label)
    }

    /// Events queued since the last call, in order.
    #[must_use]
    pub fn drain_events(&self) -> Vec<StravaEvent> {
        std::mem::take(&mut lock(&self.shared).events)
    }

    fn wake(&self, port: Option<u16>) {
        if let Some(port) = port {
            pages::poke(port);
        }
        let _ = self.tx.send(Cmd::Wake);
    }
}

impl Drop for StravaLink {
    fn drop(&mut self) {
        let port = {
            let mut guard = lock(&self.shared);
            guard.shutdown = true;
            guard.stop_listener = true;
            guard.listen_port
        };
        if let Some(port) = port {
            pages::poke(port);
        }
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
impl StravaLink {
    fn set_http_timeout(&self, timeout: Duration) {
        lock(&self.shared).http_timeout = timeout;
    }

    fn set_poll_timing(&self, first: Duration, budget: Duration) {
        let mut guard = lock(&self.shared);
        guard.poll_first = first;
        guard.poll_budget = budget;
    }

    fn warnings(&self) -> Vec<String> {
        lock(&self.shared).warnings.clone()
    }
}

struct Job {
    file: PathBuf,
    meta: UploadMeta,
}

struct Creds {
    endpoints: Endpoints,
    timeout: Duration,
    client_id: String,
    client_secret: String,
    access_token: String,
    refresh_token: String,
    expires_at: u64,
}

struct Shared {
    path: PathBuf,
    endpoints: Endpoints,
    store: Store,
    state: StravaState,
    events: Vec<StravaEvent>,
    warnings: Vec<String>,
    http_timeout: Duration,
    poll_budget: Duration,
    poll_first: Duration,
    connect_budget: Duration,
    uploads: std::collections::VecDeque<Job>,
    pending_server: Option<tiny_http::Server>,
    listen_port: Option<u16>,
    redirect: String,
    oauth_state: String,
    connect_deadline: Option<Instant>,
    stop_listener: bool,
    shutdown: bool,
}

enum Cmd {
    Wake,
    Shutdown,
}

impl Shared {
    fn new(path: PathBuf, endpoints: Endpoints, store: Store) -> Self {
        Self {
            path,
            endpoints,
            store,
            state: StravaState::NoKeys,
            events: Vec::new(),
            warnings: Vec::new(),
            http_timeout: HTTP_TIMEOUT,
            poll_budget: POLL_BUDGET,
            poll_first: POLL_FIRST,
            connect_budget: CONNECT_BUDGET,
            uploads: std::collections::VecDeque::new(),
            pending_server: None,
            listen_port: None,
            redirect: String::new(),
            oauth_state: String::new(),
            connect_deadline: None,
            stop_listener: false,
            shutdown: false,
        }
    }

    fn rest_state(&self) -> StravaState {
        if !self.store.has_keys() {
            StravaState::NoKeys
        } else if !self.store.has_tokens() {
            StravaState::NotConnected
        } else {
            StravaState::Connected {
                athlete: athlete_label(&self.store.athlete),
            }
        }
    }

    fn note(&mut self, message: &'static str) {
        eprintln!("{message}");
        self.warnings.push(message.to_owned());
    }

    fn warn_save(&mut self) {
        if let Err(err) = store::save(&self.path, &self.store) {
            let _ = err.to_string();
            self.note(SAVE_WARNING);
        }
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|err| err.into_inner())
}

fn worker(shared: Arc<Mutex<Shared>>, rx: Receiver<Cmd>) {
    let mut server: Option<tiny_http::Server> = None;
    loop {
        if lock(&shared).shutdown {
            break;
        }
        if server.is_none() {
            server = lock(&shared).pending_server.take();
        }
        let stop_listener = lock(&shared).stop_listener;
        if stop_listener {
            server = None;
            let mut guard = lock(&shared);
            guard.stop_listener = false;
            guard.listen_port = None;
            continue;
        }
        if server.is_some() {
            if connect_expired(&shared) {
                finish_failure(&shared, "Strava was not connected within five minutes.");
                server = None;
                continue;
            }
            let received = if let Some(current) = server.as_ref() {
                current.recv_timeout(Duration::from_millis(200))
            } else {
                continue;
            };
            match received {
                Ok(Some(request)) => {
                    let stop = handle_request(&shared, request);
                    let listener_stopping = lock(&shared).stop_listener;
                    if stop || listener_stopping {
                        server = None;
                        let mut guard = lock(&shared);
                        guard.stop_listener = false;
                        guard.listen_port = None;
                    }
                }
                Ok(None) => {}
                Err(err)
                    if err.kind() == ErrorKind::TimedOut || err.kind() == ErrorKind::WouldBlock => {
                }
                Err(_) => {
                    server = None;
                    let mut guard = lock(&shared);
                    guard.listen_port = None;
                    if matches!(guard.state, StravaState::Connecting { .. }) && !guard.shutdown {
                        guard.connect_deadline = None;
                        guard.oauth_state.clear();
                        guard.state = guard.rest_state();
                        guard.events.push(StravaEvent::ConnectFailed {
                            reason: "The local connection page closed.".to_owned(),
                        });
                    }
                }
            }
            continue;
        }
        let next_job = lock(&shared).uploads.pop_front();
        if let Some(job) = next_job {
            run_upload(&shared, job);
            continue;
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Cmd::Shutdown) => break,
            Ok(Cmd::Wake) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn connect_expired(shared: &Mutex<Shared>) -> bool {
    lock(shared)
        .connect_deadline
        .is_some_and(|deadline| Instant::now() >= deadline)
}

fn handle_request(shared: &Mutex<Shared>, mut request: Request) -> bool {
    let stopping = {
        let guard = lock(shared);
        guard.stop_listener || guard.shutdown
    };
    if stopping {
        pages::empty(request, 204);
        return true;
    }
    let loopback = request
        .remote_addr()
        .is_some_and(|addr| addr.ip().is_loopback());
    if !loopback {
        pages::html(request, 403, pages::FAILED_PAGE);
        return false;
    }
    let url = request.url().to_owned();
    let method = request.method().as_str().to_owned();
    let path = url.split('?').next().unwrap_or(url.as_str()).to_owned();
    if path == "/poke" {
        pages::empty(request, 204);
        return false;
    }
    if method.eq_ignore_ascii_case("GET") && path == "/" {
        pages::html(request, 200, pages::FORM_PAGE);
        return false;
    }
    if method.eq_ignore_ascii_case("POST") && path == "/" {
        let body = pages::read_body(&mut request, 16 * 1024);
        return handle_form(shared, request, &body);
    }
    if method.eq_ignore_ascii_case("GET") && path == "/callback" {
        return handle_callback(shared, request, &url);
    }
    pages::html(request, 404, pages::FAILED_PAGE);
    false
}

fn handle_form(shared: &Mutex<Shared>, request: Request, body: &[u8]) -> bool {
    let pairs = form_pairs(body);
    let client_id = field(&pairs, "client_id").trim().to_owned();
    let client_secret = field(&pairs, "client_secret").trim().to_owned();
    if client_id.is_empty() || client_secret.is_empty() {
        pages::html(request, 400, pages::FORM_AGAIN_PAGE);
        return false;
    }
    let url = {
        let mut guard = lock(shared);
        if guard.stop_listener || guard.shutdown {
            drop(guard);
            pages::empty(request, 204);
            return true;
        }
        let changed =
            guard.store.client_id != client_id || guard.store.client_secret != client_secret;
        guard.store.client_id = client_id;
        guard.store.client_secret = client_secret;
        if changed {
            guard.store.clear_tokens();
        }
        let Some(state) = random_state() else {
            guard.state = StravaState::Error("Could not start the Strava connection.".to_owned());
            guard.events.push(StravaEvent::ConnectFailed {
                reason: "Could not start the Strava connection.".to_owned(),
            });
            drop(guard);
            pages::html(request, 500, pages::FAILED_PAGE);
            return true;
        };
        let url = api::authorize_url(
            &guard.endpoints,
            &guard.store.client_id,
            &guard.redirect,
            &state,
        );
        guard.oauth_state = state;
        guard.state = StravaState::Connecting { url: url.clone() };
        guard.warn_save();
        guard.events.push(StravaEvent::KeysSaved);
        url
    };
    pages::redirect(request, &url);
    false
}

fn handle_callback(shared: &Mutex<Shared>, request: Request, url: &str) -> bool {
    let pairs = form_pairs(
        url.split_once('?')
            .map(|(_, query)| query)
            .unwrap_or("")
            .as_bytes(),
    );
    let sent = field(&pairs, "state").to_owned();
    let expected = lock(shared).oauth_state.clone();
    if expected.is_empty() || sent != expected {
        pages::html(request, 400, pages::MISMATCH_PAGE);
        return false;
    }
    let error = field(&pairs, "error");
    if !error.is_empty() {
        let reason = if error == "access_denied" {
            "Strava access was declined."
        } else {
            "Strava did not finish the connection."
        };
        finish_failure(shared, reason);
        let page = if error == "access_denied" {
            pages::DENIED_PAGE
        } else {
            pages::FAILED_PAGE
        };
        pages::html(request, 200, page);
        return true;
    }
    let code = field(&pairs, "code").to_owned();
    if code.is_empty() {
        finish_failure(shared, "Strava did not return a connection code.");
        pages::html(request, 200, pages::FAILED_PAGE);
        return true;
    }
    let creds = creds(shared);
    match api::exchange_code(
        &creds.endpoints,
        creds.timeout,
        &creds.client_id,
        &creds.client_secret,
        &code,
    ) {
        Ok(tokens) if api::scope_allows_upload(tokens.scope.as_deref()) => {
            apply_connected(shared, tokens);
            pages::html(request, 200, pages::CONNECTED_PAGE);
            true
        }
        Ok(_) => {
            finish_failure(shared, "Strava did not grant permission to upload rides.");
            pages::html(request, 200, pages::FAILED_PAGE);
            true
        }
        Err(err) => {
            let reason = connect_reason(&err);
            finish_failure(shared, &reason);
            pages::html(request, 200, pages::FAILED_PAGE);
            true
        }
    }
}

fn apply_connected(shared: &Mutex<Shared>, tokens: api::TokenSet) {
    let mut guard = lock(shared);
    guard.store.access_token = tokens.access_token;
    guard.store.refresh_token = tokens.refresh_token;
    guard.store.expires_at = tokens.expires_at;
    let athlete = tokens
        .athlete_name
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Strava athlete".to_owned());
    guard.store.athlete = athlete.clone();
    guard.warn_save();
    guard.oauth_state.clear();
    guard.connect_deadline = None;
    guard.stop_listener = true;
    guard.state = StravaState::Connected {
        athlete: athlete.clone(),
    };
    guard.events.push(StravaEvent::Connected { athlete });
}

fn finish_failure(shared: &Mutex<Shared>, reason: &str) {
    let mut guard = lock(shared);
    guard.oauth_state.clear();
    guard.connect_deadline = None;
    guard.stop_listener = true;
    guard.state = guard.rest_state();
    guard.events.push(StravaEvent::ConnectFailed {
        reason: reason.to_owned(),
    });
}

fn run_upload(shared: &Mutex<Shared>, job: Job) {
    if lock(shared).shutdown {
        return;
    }
    let label = api::file_label(&job.file);
    let connected = {
        let guard = lock(shared);
        guard.store.has_keys() && guard.store.has_tokens()
    };
    if !connected {
        push_upload_failed(shared, job.file, "Strava is not connected.", false);
        return;
    }
    if !job.file.is_file() {
        push_upload_failed(shared, job.file, "The ride file could not be read.", false);
        return;
    }
    lock(shared).events.push(StravaEvent::UploadStarted {
        file: job.file.clone(),
    });
    match obtain_activity(shared, &job, &label) {
        Ok(activity_id) => remember_success(shared, job.file, &label, activity_id),
        Err(err) => fail_upload(shared, job.file, &label, err),
    }
}

fn obtain_activity(shared: &Mutex<Shared>, job: &Job, label: &str) -> Result<u64, ApiError> {
    let mut refreshed = false;
    if expires_soon(creds(shared).expires_at) {
        do_refresh(shared)?;
        refreshed = true;
    }
    let progress = match post_upload(shared, job, label) {
        Err(ApiError::Unauthorized) if !refreshed => {
            do_refresh(shared)?;
            post_upload(shared, job, label)?
        }
        other => other?,
    };
    match progress {
        UploadProgress::Ready { activity_id } => Ok(activity_id),
        UploadProgress::Pending { id } => poll_until(shared, id),
    }
}

fn post_upload(shared: &Mutex<Shared>, job: &Job, label: &str) -> Result<UploadProgress, ApiError> {
    let creds = creds(shared);
    api::create_upload(
        &creds.endpoints,
        creds.timeout,
        &creds.access_token,
        &job.file,
        label,
        &job.meta.name,
        &job.meta.description,
    )
}

fn do_refresh(shared: &Mutex<Shared>) -> Result<(), ApiError> {
    let creds = creds(shared);
    let tokens = api::refresh(
        &creds.endpoints,
        creds.timeout,
        &creds.client_id,
        &creds.client_secret,
        &creds.refresh_token,
    )?;
    let mut guard = lock(shared);
    if tokens.access_token.is_empty() || tokens.refresh_token.is_empty() {
        return Err(ApiError::BadResponse);
    }
    guard.store.access_token = tokens.access_token;
    guard.store.refresh_token = tokens.refresh_token;
    guard.store.expires_at = tokens.expires_at;
    if let Some(name) = tokens.athlete_name.filter(|name| !name.is_empty()) {
        guard.store.athlete = name.clone();
        if let StravaState::Connected { athlete } = &mut guard.state {
            *athlete = name;
        }
    }
    guard.warn_save();
    Ok(())
}

fn poll_until(shared: &Mutex<Shared>, upload_id: u64) -> Result<u64, ApiError> {
    let started = Instant::now();
    let budget = lock(shared).poll_budget;
    let mut wait = lock(shared).poll_first;
    loop {
        if lock(shared).shutdown {
            return Err(ApiError::Transport);
        }
        let progress = {
            let creds = creds(shared);
            api::poll_upload(
                &creds.endpoints,
                creds.timeout,
                &creds.access_token,
                upload_id,
            )?
        };
        if let UploadProgress::Ready { activity_id } = progress {
            return Ok(activity_id);
        }
        if started.elapsed() >= budget || started.elapsed().saturating_add(wait) > budget {
            return Err(ApiError::NotFinished);
        }
        if !sleep_unless_shutdown(shared, wait) {
            return Err(ApiError::Transport);
        }
        wait = (wait * 2).min(Duration::from_secs(8));
    }
}

fn remember_success(shared: &Mutex<Shared>, file: PathBuf, label: &str, activity_id: u64) {
    let mut guard = lock(shared);
    guard.store.remember_upload(label.to_owned(), activity_id);
    guard.warn_save();
    guard.events.push(StravaEvent::Uploaded {
        file,
        activity_id,
        url: api::activity_url(activity_id),
    });
}

fn fail_upload(shared: &Mutex<Shared>, file: PathBuf, label: &str, err: ApiError) {
    if lock(shared).shutdown {
        return;
    }
    if let ApiError::Duplicate { activity_id } = err {
        let reason = duplicate_reason(activity_id);
        let mut guard = lock(shared);
        guard.store.remember_upload(label.to_owned(), activity_id);
        guard.warn_save();
        guard.events.push(StravaEvent::UploadFailed {
            file,
            reason,
            duplicate: true,
        });
        return;
    }
    if matches!(err, ApiError::Unauthorized) {
        forget_tokens(shared);
    }
    let reason = upload_reason(&err);
    push_upload_failed(shared, file, &reason, false);
}

fn forget_tokens(shared: &Mutex<Shared>) {
    let mut guard = lock(shared);
    guard.store.clear_tokens();
    guard.warn_save();
    if !matches!(guard.state, StravaState::Connecting { .. }) {
        guard.state = guard.rest_state();
    }
}

fn push_upload_failed(shared: &Mutex<Shared>, file: PathBuf, reason: &str, duplicate: bool) {
    lock(shared).events.push(StravaEvent::UploadFailed {
        file,
        reason: reason.to_owned(),
        duplicate,
    });
}

fn creds(shared: &Mutex<Shared>) -> Creds {
    let guard = lock(shared);
    Creds {
        endpoints: guard.endpoints.clone(),
        timeout: guard.http_timeout,
        client_id: guard.store.client_id.clone(),
        client_secret: guard.store.client_secret.clone(),
        access_token: guard.store.access_token.clone(),
        refresh_token: guard.store.refresh_token.clone(),
        expires_at: guard.store.expires_at,
    }
}

fn expires_soon(expires_at: u64) -> bool {
    expires_at == 0 || expires_at <= api::now_unix().saturating_add(REFRESH_WITHIN_SECS)
}

fn sleep_unless_shutdown(shared: &Mutex<Shared>, total: Duration) -> bool {
    let step = Duration::from_millis(50);
    let started = Instant::now();
    while started.elapsed() < total {
        if lock(shared).shutdown {
            return false;
        }
        let left = total.saturating_sub(started.elapsed());
        thread::sleep(step.min(left));
    }
    !lock(shared).shutdown
}

fn upload_reason(err: &ApiError) -> String {
    match err {
        ApiError::Timeout => "Strava did not respond in time.".to_owned(),
        ApiError::NotFinished => "Strava did not finish processing the ride in time.".to_owned(),
        ApiError::RateLimited => {
            "Strava rate limit reached (HTTP 429). Wait and try this ride again.".to_owned()
        }
        ApiError::Unauthorized => "Strava rejected the saved login. Connect again.".to_owned(),
        ApiError::Unreadable => "The ride file could not be read.".to_owned(),
        ApiError::BadResponse => "Strava returned a response that could not be read.".to_owned(),
        ApiError::Transport => "The game could not reach Strava.".to_owned(),
        ApiError::Rejected => "Strava could not process the ride.".to_owned(),
        ApiError::Status(code) => format!("Strava returned HTTP {code}."),
        ApiError::Duplicate { activity_id } => duplicate_reason(*activity_id),
    }
}

fn connect_reason(err: &ApiError) -> String {
    match err {
        ApiError::Timeout | ApiError::NotFinished | ApiError::Transport | ApiError::Unreadable => {
            "The game could not reach Strava.".to_owned()
        }
        ApiError::RateLimited => {
            "Strava rate limit reached (HTTP 429). Wait and try connecting again.".to_owned()
        }
        ApiError::Unauthorized => "Strava rejected the connection.".to_owned(),
        ApiError::BadResponse => {
            "Strava returned a connection response that could not be read.".to_owned()
        }
        ApiError::Status(code) => format!("Strava did not accept the connection (HTTP {code})."),
        ApiError::Rejected | ApiError::Duplicate { .. } => {
            "Strava did not accept the connection.".to_owned()
        }
    }
}

fn duplicate_reason(activity_id: u64) -> String {
    if activity_id == 0 {
        "Strava already has this ride.".to_owned()
    } else {
        format!("Strava already has this ride (activity {activity_id}).")
    }
}

fn athlete_label(name: &str) -> String {
    if name.is_empty() {
        "Strava athlete".to_owned()
    } else {
        name.to_owned()
    }
}

fn bind_listener() -> Option<(tiny_http::Server, u16)> {
    for port in PREFERRED_PORTS {
        if let Ok(server) = tiny_http::Server::http(format!("127.0.0.1:{port}"))
            && let Some(found) = server_port(&server)
        {
            return Some((server, found));
        }
    }
    let server = tiny_http::Server::http("127.0.0.1:0").ok()?;
    let port = server_port(&server)?;
    Some((server, port))
}

pub(crate) fn server_port(server: &tiny_http::Server) -> Option<u16> {
    match server.server_addr() {
        tiny_http::ListenAddr::IP(addr) => Some(addr.port()),
        tiny_http::ListenAddr::Unix(_) => None,
    }
}

fn random_state() -> Option<String> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).ok()?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(32);
    for byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Some(text)
}

fn form_pairs(body: &[u8]) -> Vec<(String, String)> {
    form_urlencoded::parse(body)
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

fn field<'a>(pairs: &'a [(String, String)], key: &str) -> &'a str {
    pairs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
        .unwrap_or("")
}

#[cfg(test)]
mod tests;
