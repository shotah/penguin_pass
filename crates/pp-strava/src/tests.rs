use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{Endpoints, Keys, StravaEvent, StravaLink, StravaState, UploadMeta};

const SECRET: &str = "pp-secret-SENTINEL-a17c";
const ACCESS: &str = "pp-access-SENTINEL-a17c";
const REFRESH: &str = "pp-refresh-SENTINEL-a17c";

fn scratch() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pp-strava-{}-{nanos}-{n}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        Self(scratch())
    }

    fn path(&self) -> PathBuf {
        self.0.join("strava.toml")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tune(link: &StravaLink) {
    link.set_http_timeout(Duration::from_secs(2));
    link.set_poll_timing(Duration::from_millis(30), Duration::from_secs(3));
}

fn endpoints_of(base: &str) -> Endpoints {
    Endpoints {
        oauth_base: base.to_owned(),
        api_base: base.to_owned(),
    }
}

struct Mock {
    base: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Mock {
    fn spawn(handler: impl Fn(tiny_http::Request) + Send + Sync + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_worker = Arc::clone(&stop);
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = thread::spawn(move || {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("mock bind");
            let port = super::server_port(&server).expect("mock port");
            let _ = tx.send(port);
            loop {
                if stop_worker.load(Ordering::SeqCst) {
                    break;
                }
                match server.recv_timeout(Duration::from_millis(100)) {
                    Ok(Some(request)) => {
                        if stop_worker.load(Ordering::SeqCst) {
                            break;
                        }
                        handler(request);
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });
        let port = rx.recv_timeout(Duration::from_secs(2)).expect("mock port");
        Self {
            base: format!("http://127.0.0.1:{port}"),
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(port) = self
            .base
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse().ok())
        {
            super::pages::poke(port);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn respond_json(request: tiny_http::Request, status: u16, body: &str) {
    let content_type =
        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
            .expect("header");
    let response = tiny_http::Response::from_string(body.to_owned())
        .with_status_code(tiny_http::StatusCode(status))
        .with_header(content_type);
    let _ = request.respond(response);
}

fn read_body(request: &mut tiny_http::Request) -> Vec<u8> {
    let mut body = Vec::new();
    let _ = request.as_reader().read_to_end(&mut body);
    body
}

fn token_json(access: &str, refresh: &str, expires_at: u64) -> String {
    format!(
        concat!(
            r#"{{"token_type":"Bearer","expires_at":{expires_at},"expires_in":21600,"#,
            r#""refresh_token":"{refresh}","access_token":"{access}","#,
            r#""athlete":{{"id":7,"firstname":"Ada","lastname":"Lovelace"}},"#,
            r#""scope":"activity:write read"}}"#
        ),
        expires_at = expires_at,
        refresh = refresh,
        access = access,
    )
}

fn upload_ready(activity_id: u64) -> String {
    format!(
        r#"{{"id":77,"id_str":"77","external_id":"ride.fit","error":null,"status":"Your activity is ready.","activity_id":{activity_id}}}"#
    )
}

fn upload_pending() -> &'static str {
    r#"{"id":77,"id_str":"77","external_id":"ride.fit","error":null,"status":"Your activity is still being processed.","activity_id":null}"#
}

fn far_expiry() -> u64 {
    super::api::now_unix().saturating_add(86_400)
}

struct Seen {
    path: String,
    body: Vec<u8>,
}

fn record(
    seen: &Arc<Mutex<Vec<Seen>>>,
    request: &mut tiny_http::Request,
) -> (String, String, Vec<u8>) {
    let method = request.method().as_str().to_owned();
    let path = request.url().to_owned();
    let body = read_body(request);
    seen.lock().unwrap().push(Seen {
        path: path.clone(),
        body: body.clone(),
    });
    (method, path, body)
}

fn until(
    link: &StravaLink,
    timeout: Duration,
    mut done: impl FnMut(&StravaState, &[StravaEvent]) -> bool,
) -> (StravaState, Vec<StravaEvent>) {
    let started = Instant::now();
    let mut events = Vec::new();
    loop {
        events.extend(link.drain_events());
        let state = link.state();
        if done(&state, &events) || started.elapsed() > timeout {
            return (state, events);
        }
        thread::sleep(Duration::from_millis(15));
    }
}

fn exchange(addr: &str, request: &str) -> String {
    let started = Instant::now();
    loop {
        match TcpStream::connect(addr) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream.write_all(request.as_bytes()).unwrap();
                return read_response(&mut stream);
            }
            Err(_) if started.elapsed() < Duration::from_secs(2) => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(err) => panic!("connect {addr}: {err}"),
        }
    }
}

fn read_response(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 2048];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if response_complete(&buf) {
                    break;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn response_complete(buf: &[u8]) -> bool {
    let Some(split) = find_subsequence(buf, b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&buf[..split]);
    let Some(length) = content_length(&headers) else {
        return false;
    };
    buf.len() >= split + 4 + length
}

fn content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn status_of(response: &str) -> u16 {
    response
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0)
}

fn header_of(response: &str, name: &str) -> Option<String> {
    let headers = response.split("\r\n\r\n").next()?;
    for line in headers.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.eq_ignore_ascii_case(name) {
            return Some(value.trim().to_owned());
        }
    }
    None
}

fn query(url: &str) -> Vec<(String, String)> {
    let query = url.split_once('?').map(|(_, query)| query).unwrap_or("");
    form_urlencoded::parse(query.as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

fn query_get<'a>(pairs: &'a [(String, String)], key: &str) -> &'a str {
    pairs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
        .unwrap_or("")
}

fn port_of_url(url: &str) -> u16 {
    url.trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .unwrap_or(0)
}

fn redirect_port(authorize_url: &str) -> u16 {
    let pairs = query(authorize_url);
    port_of_url(query_get(&pairs, "redirect_uri"))
}

fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn meta() -> UploadMeta {
    UploadMeta {
        name: "Penguin Pass".to_owned(),
        description: "Indoor ride".to_owned(),
    }
}

fn write_ride(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn authorize_url_has_the_documented_query() {
    let dir = TempDir::new();
    let link = StravaLink::open(dir.path(), Endpoints::strava());
    link.set_keys(Keys {
        client_id: "424242".to_owned(),
        client_secret: SECRET.to_owned(),
    });
    let url = link.begin_connect().expect("url");
    assert!(url.starts_with("https://www.strava.com/oauth/authorize?"));
    assert!(!url.contains(SECRET));
    let pairs = query(&url);
    assert_eq!(query_get(&pairs, "client_id"), "424242");
    assert_eq!(query_get(&pairs, "response_type"), "code");
    assert_eq!(query_get(&pairs, "approval_prompt"), "auto");
    assert_eq!(query_get(&pairs, "scope"), "activity:write,read");
    let state = query_get(&pairs, "state");
    assert_eq!(state.len(), 32);
    assert!(state.chars().all(|c| c.is_ascii_hexdigit()));
    let redirect = query_get(&pairs, "redirect_uri");
    assert!(redirect.starts_with("http://127.0.0.1:"));
    assert!(redirect.ends_with("/callback"));
    drop(link);
}

#[test]
fn callback_accepts_the_matching_state_and_stores_tokens() {
    let dir = TempDir::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls_handler = Arc::clone(&calls);
    let expiry = far_expiry();
    let mock = Mock::spawn(move |mut request| {
        let (_method, path, body) = record(&calls_handler, &mut request);
        assert!(path.starts_with("/oauth/token"), "{path}");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("grant_type=authorization_code"));
        assert!(text.contains("code=from-strava"));
        assert!(text.contains("client_id=424242"));
        assert!(text.contains(&format!("client_secret={SECRET}")));
        respond_json(
            request,
            200,
            &token_json("access-one", "refresh-one", expiry),
        );
    });
    let link = StravaLink::open(dir.path(), endpoints_of(&mock.base));
    tune(&link);
    link.set_keys(Keys {
        client_id: "424242".to_owned(),
        client_secret: SECRET.to_owned(),
    });
    let url = link.begin_connect().expect("authorize");
    let pairs = query(&url);
    let state = query_get(&pairs, "state").to_owned();
    let port = redirect_port(&url);
    let wrong = exchange(
        &format!("127.0.0.1:{port}"),
        "GET /callback?code=from-strava&state=not-the-state HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(status_of(&wrong), 400);
    assert!(wrong.contains("did not match"));
    assert!(matches!(link.state(), StravaState::Connecting { .. }));
    let right = exchange(
        &format!("127.0.0.1:{port}"),
        &format!(
            "GET /callback?code=from-strava&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ),
    );
    assert_eq!(status_of(&right), 200);
    assert!(right.contains("close this tab"));
    let (state, events) = until(&link, Duration::from_secs(3), |state, events| {
        events
            .iter()
            .any(|event| matches!(event, StravaEvent::Connected { .. }))
            || matches!(state, StravaState::Error(_))
    });
    assert!(
        matches!(state, StravaState::Connected { ref athlete } if athlete == "Ada Lovelace"),
        "{state:?} {events:?}"
    );
    let text = fs::read_to_string(dir.path()).unwrap();
    assert!(text.contains("access-one"));
    assert!(text.contains("refresh-one"));
    assert!(text.contains(SECRET));
    assert_eq!(mode_of(&dir.path()), 0o600);
    assert!(calls.lock().unwrap().len() == 1);
}

#[test]
fn access_denied_stops_the_flow() {
    let dir = TempDir::new();
    let link = StravaLink::open(dir.path(), endpoints_of("http://127.0.0.1:9"));
    link.set_keys(Keys {
        client_id: "424242".to_owned(),
        client_secret: SECRET.to_owned(),
    });
    let url = link.begin_connect().expect("authorize");
    let pairs = query(&url);
    let state = query_get(&pairs, "state");
    let port = redirect_port(&url);
    let page = exchange(
        &format!("127.0.0.1:{port}"),
        &format!(
            "GET /callback?error=access_denied&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(page.contains("declined"));
    let (state, events) = until(&link, Duration::from_secs(2), |_state, events| {
        events
            .iter()
            .any(|event| matches!(event, StravaEvent::ConnectFailed { .. }))
    });
    assert!(matches!(state, StravaState::NotConnected), "{state:?}");
    assert!(events.iter().any(|event| matches!(
        event,
        StravaEvent::ConnectFailed { reason } if reason.contains("declined")
    )));
    let text = fs::read_to_string(dir.path()).unwrap();
    assert!(!text.contains("access_token = \"a"));
}

#[test]
fn key_form_round_trip() {
    let dir = TempDir::new();
    let link = StravaLink::open(dir.path(), endpoints_of("http://127.0.0.1:9"));
    let url = link.begin_connect().expect("form");
    assert!(url.starts_with("http://127.0.0.1:"));
    assert!(url.ends_with('/'));
    let port = port_of_url(&url);
    let page = exchange(
        &format!("127.0.0.1:{port}"),
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(page.contains("https://www.strava.com/settings/api"));
    assert!(page.contains("127.0.0.1"));
    assert!(page.contains("client_id"));
    assert!(page.contains("client_secret"));
    let body = form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", "424242")
        .append_pair("client_secret", "s3cret&more=1")
        .finish();
    let posted = exchange(
        &format!("127.0.0.1:{port}"),
        &format!(
            "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    assert_eq!(status_of(&posted), 303);
    let location = header_of(&posted, "location").expect("location");
    assert!(location.starts_with("http://127.0.0.1:9/oauth/authorize?"));
    assert!(!location.contains("s3cret"));
    let pairs = query(&location);
    assert_eq!(query_get(&pairs, "client_id"), "424242");
    assert_eq!(query_get(&pairs, "response_type"), "code");
    assert_eq!(query_get(&pairs, "approval_prompt"), "auto");
    assert_eq!(query_get(&pairs, "scope"), "activity:write,read");
    assert!(!query_get(&pairs, "state").is_empty());
    assert!(query_get(&pairs, "redirect_uri").contains("/callback"));
    let (state, _events) = until(&link, Duration::from_secs(2), |_state, events| {
        events
            .iter()
            .any(|event| matches!(event, StravaEvent::KeysSaved))
    });
    assert!(matches!(state, StravaState::Connecting { .. }), "{state:?}");
    let text = fs::read_to_string(dir.path()).unwrap();
    assert!(text.contains("s3cret&more=1"));
    assert!(text.contains("424242"));
    assert_eq!(mode_of(&dir.path()), 0o600);
}

#[test]
fn refresh_before_upload_stores_the_rotated_refresh_token() {
    let dir = TempDir::new();
    fs::write(
        dir.path(),
        format!(
            "client_id = \"424242\"\nclient_secret = \"{SECRET}\"\naccess_token = \"old-access\"\nrefresh_token = \"old-refresh\"\nexpires_at = 1\nathlete = \"Ada Lovelace\"\n"
        ),
    )
    .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls_handler = Arc::clone(&calls);
    let mock = Mock::spawn(move |mut request| {
        let (_method, path, body) = record(&calls_handler, &mut request);
        if path.starts_with("/oauth/token") {
            let text = String::from_utf8_lossy(&body);
            assert!(text.contains("grant_type=refresh_token"));
            assert!(text.contains("refresh_token=old-refresh"));
            respond_json(
                request,
                200,
                &token_json("new-access", "new-refresh", far_expiry()),
            );
        } else {
            respond_json(request, 201, &upload_ready(55));
        }
    });
    let link = StravaLink::open(dir.path(), endpoints_of(&mock.base));
    tune(&link);
    assert!(matches!(
        link.state(),
        StravaState::Connected { athlete } if athlete == "Ada Lovelace"
    ));
    let file = write_ride(&dir.0, "morning.fit", b"fit-bytes");
    link.upload(file.clone(), meta());
    let (state, events) = until(&link, Duration::from_secs(3), |_state, events| {
        events.iter().any(|event| {
            matches!(
                event,
                StravaEvent::Uploaded { .. } | StravaEvent::UploadFailed { .. }
            )
        })
    });
    assert!(
        matches!(state, StravaState::Connected { .. }),
        "{state:?} {events:?}"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        StravaEvent::Uploaded {
            activity_id: 55,
            ..
        }
    )));
    let text = fs::read_to_string(dir.path()).unwrap();
    assert!(text.contains("new-refresh"));
    assert!(text.contains("new-access"));
    assert!(!text.contains("old-refresh"));
    let seen = calls.lock().unwrap();
    assert!(
        seen[0].path.starts_with("/oauth/token"),
        "{:?}",
        seen[0].path
    );
    assert!(
        seen[1].path.starts_with("/api/v3/uploads"),
        "{:?}",
        seen[1].path
    );
}

#[test]
fn multipart_body_contains_the_file_and_fields() {
    let dir = TempDir::new();
    let bytes = b"\x0eFIT-BYTES-\x00\xff-end";
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls_handler = Arc::clone(&calls);
    let mock = Mock::spawn(move |mut request| {
        let (_method, path, _body) = record(&calls_handler, &mut request);
        if path.starts_with("/oauth/token") {
            respond_json(request, 200, &token_json("access", "refresh", far_expiry()));
        } else {
            respond_json(request, 201, &upload_ready(9));
        }
    });
    let link = connected(&dir, &mock.base);
    let file = write_ride(&dir.0, "morning.fit", bytes);
    link.upload(
        file,
        UploadMeta {
            name: "Penguin Pass".to_owned(),
            description: "Indoor ride".to_owned(),
        },
    );
    let (_state, events) = until(&link, Duration::from_secs(3), |_state, events| {
        events
            .iter()
            .any(|event| matches!(event, StravaEvent::Uploaded { .. }))
    });
    assert!(events.iter().any(|event| matches!(
        event,
        StravaEvent::Uploaded { activity_id: 9, url, .. } if url == "https://www.strava.com/activities/9"
    )));
    let seen = calls.lock().unwrap();
    let upload = seen
        .iter()
        .find(|hit| hit.path.starts_with("/api/v3/uploads"))
        .expect("upload");
    assert!(
        upload
            .body
            .windows(bytes.len())
            .any(|window| window == bytes)
    );
    let text = String::from_utf8_lossy(&upload.body);
    assert!(text.contains("name=\"name\""));
    assert!(text.contains("Penguin Pass"));
    assert!(text.contains("name=\"description\""));
    assert!(text.contains("Indoor ride"));
    assert!(text.contains("name=\"data_type\""));
    assert!(text.contains("fit"));
    assert!(text.contains("name=\"external_id\""));
    assert!(text.contains("morning.fit"));
    assert!(text.contains("name=\"sport_type\""));
    assert!(text.contains("VirtualRide"));
    assert!(text.contains("name=\"trainer\""));
    assert!(text.contains('1'));
    assert!(seen.iter().all(|hit| !hit.path.starts_with("/oauth/token")));
}

#[test]
fn polling_reaches_an_activity_id() {
    let dir = TempDir::new();
    let polls = Arc::new(AtomicU64::new(0));
    let polls_handler = Arc::clone(&polls);
    let mock = Mock::spawn(move |mut request| {
        let path = request.url().to_owned();
        let _ = read_body(&mut request);
        if path.starts_with("/api/v3/uploads/") {
            let n = polls_handler.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                respond_json(request, 200, upload_pending());
            } else {
                respond_json(request, 200, &upload_ready(153_243_126));
            }
        } else if path.starts_with("/api/v3/uploads") {
            respond_json(request, 201, upload_pending());
        } else {
            respond_json(request, 404, "{}");
        }
    });
    let link = connected(&dir, &mock.base);
    let file = write_ride(&dir.0, "lap.fit", b"abc");
    link.upload(file.clone(), meta());
    let (_state, events) = until(&link, Duration::from_secs(3), |_state, events| {
        events.iter().any(|event| {
            matches!(
                event,
                StravaEvent::Uploaded { .. } | StravaEvent::UploadFailed { .. }
            )
        })
    });
    assert!(events.iter().any(|event| matches!(
        event,
        StravaEvent::Uploaded {
            activity_id: 153_243_126,
            url,
            ..
        } if url == "https://www.strava.com/activities/153243126" && event_file(event) == file
    )));
    assert!(polls.load(Ordering::SeqCst) >= 2);
}

fn event_file(event: &StravaEvent) -> &Path {
    match event {
        StravaEvent::Uploaded { file, .. } | StravaEvent::UploadFailed { file, .. } => file,
        _ => Path::new(""),
    }
}

#[test]
fn duplicate_is_recorded_as_uploaded() {
    let dir = TempDir::new();
    let mock = Mock::spawn(|mut request| {
        let path = request.url().to_owned();
        let _ = read_body(&mut request);
        if path.starts_with("/api/v3/uploads/") {
            respond_json(
                request,
                200,
                r#"{"id":77,"error":"ride.fit duplicate of activity 21234316","status":"There was an error processing your activity.","activity_id":null}"#,
            );
        } else {
            respond_json(request, 201, upload_pending());
        }
    });
    let link = connected(&dir, &mock.base);
    let file = write_ride(&dir.0, "ride.fit", b"abc");
    link.upload(file.clone(), meta());
    let (_state, events) = until(&link, Duration::from_secs(3), |_state, events| {
        events
            .iter()
            .any(|event| matches!(event, StravaEvent::UploadFailed { .. }))
    });
    assert!(events.iter().any(|event| matches!(
        event,
        StravaEvent::UploadFailed { duplicate: true, reason, .. }
            if reason.contains("21234316")
    )));
    assert!(link.was_uploaded(&file));
    let text = fs::read_to_string(dir.path()).unwrap();
    assert!(text.contains("21234316"));
}

#[test]
fn unauthorized_after_refresh_disconnects() {
    let dir = TempDir::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls_handler = Arc::clone(&calls);
    let mock = Mock::spawn(move |mut request| {
        let (_method, path, _body) = record(&calls_handler, &mut request);
        if path.starts_with("/oauth/token") {
            respond_json(
                request,
                200,
                &token_json("second-access", "second-refresh", far_expiry()),
            );
        } else {
            respond_json(request, 401, r#"{"message":"Authorization Error"}"#);
        }
    });
    let link = connected(&dir, &mock.base);
    let file = write_ride(&dir.0, "ride.fit", b"abc");
    link.upload(file, meta());
    let (state, events) = until(&link, Duration::from_secs(3), |state, events| {
        matches!(state, StravaState::NotConnected)
            && events
                .iter()
                .any(|event| matches!(event, StravaEvent::UploadFailed { .. }))
    });
    assert!(
        matches!(state, StravaState::NotConnected),
        "{state:?} {events:?}"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        StravaEvent::UploadFailed { duplicate: false, reason, .. } if reason.contains("Connect again")
    )));
    let text = fs::read_to_string(dir.path()).unwrap();
    assert!(!text.contains("second-access"));
    assert!(text.contains("424242"));
    let seen = calls.lock().unwrap();
    assert_eq!(
        seen.len(),
        3,
        "{:?}",
        seen.iter().map(|hit| &hit.path).collect::<Vec<_>>()
    );
    assert!(seen[0].path.starts_with("/api/v3/uploads"));
    assert!(seen[1].path.starts_with("/oauth/token"));
    assert!(seen[2].path.starts_with("/api/v3/uploads"));
}

#[test]
fn rate_limit_is_reported_once() {
    let dir = TempDir::new();
    let calls = Arc::new(AtomicU64::new(0));
    let calls_handler = Arc::clone(&calls);
    let mock = Mock::spawn(move |mut request| {
        calls_handler.fetch_add(1, Ordering::SeqCst);
        let _ = read_body(&mut request);
        respond_json(request, 429, r#"{"message":"Rate Limit Exceeded"}"#);
    });
    let link = connected(&dir, &mock.base);
    let file = write_ride(&dir.0, "ride.fit", b"abc");
    link.upload(file.clone(), meta());
    let (state, events) = until(&link, Duration::from_secs(3), |_state, events| {
        events
            .iter()
            .any(|event| matches!(event, StravaEvent::UploadFailed { .. }))
    });
    assert!(matches!(state, StravaState::Connected { .. }), "{state:?}");
    assert!(events.iter().any(|event| matches!(
        event,
        StravaEvent::UploadFailed { duplicate: false, reason, .. }
            if reason.contains("429") && !reason.contains("Rate Limit Exceeded")
    )));
    assert!(!link.was_uploaded(&file));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn request_timeout_is_reported() {
    let dir = TempDir::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let thread = thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            thread::sleep(Duration::from_millis(800));
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
        }
    });
    seed_connected(&dir.path(), far_expiry());
    let link = StravaLink::open(
        dir.path(),
        endpoints_of(&format!("http://127.0.0.1:{port}")),
    );
    tune(&link);
    link.set_http_timeout(Duration::from_millis(200));
    let file = write_ride(&dir.0, "ride.fit", b"abc");
    link.upload(file, meta());
    let (_state, events) = until(&link, Duration::from_secs(3), |_state, events| {
        events
            .iter()
            .any(|event| matches!(event, StravaEvent::UploadFailed { .. }))
    });
    assert!(events.iter().any(|event| matches!(
        event,
        StravaEvent::UploadFailed { reason, duplicate: false, .. } if reason.contains("did not respond")
    )));
    drop(link);
    let _ = thread.join();
}

#[test]
fn store_file_is_owner_only() {
    let dir = TempDir::new();
    let link = StravaLink::open(dir.path(), Endpoints::strava());
    link.set_keys(Keys {
        client_id: "424242".to_owned(),
        client_secret: SECRET.to_owned(),
    });
    assert_eq!(mode_of(&dir.path()), 0o600);
}

#[test]
fn corrupt_store_is_no_keys() {
    let dir = TempDir::new();
    fs::write(dir.path(), format!("this is not toml {SECRET} [[[")).unwrap();
    let link = StravaLink::open(dir.path(), Endpoints::strava());
    assert!(matches!(link.state(), StravaState::NoKeys));
    let warnings = link.warnings().join("\n");
    assert!(warnings.contains("could not be read"));
    assert!(!warnings.contains(SECRET));
    let text = fs::read_to_string(dir.path()).unwrap();
    assert!(text.contains(SECRET));
}

#[test]
fn secrets_do_not_appear_in_debug_or_events() {
    let dir = TempDir::new();
    let mock = Mock::spawn(|mut request| {
        let path = request.url().to_owned();
        let _ = read_body(&mut request);
        if path.starts_with("/oauth/token") {
            respond_json(request, 200, &token_json(ACCESS, REFRESH, far_expiry()));
        } else {
            respond_json(
                request,
                400,
                &format!(r#"{{"message":"{SECRET} {ACCESS} {REFRESH}"}}"#),
            );
        }
    });
    let link = StravaLink::open(dir.path(), endpoints_of(&mock.base));
    tune(&link);
    let keys = Keys {
        client_id: "424242".to_owned(),
        client_secret: SECRET.to_owned(),
    };
    link.set_keys(keys.clone());
    let url = link.begin_connect().expect("url");
    assert!(!url.contains(SECRET));
    let state = query_get(&query(&url), "state").to_owned();
    let port = redirect_port(&url);
    let _ = exchange(
        &format!("127.0.0.1:{port}"),
        &format!(
            "GET /callback?code=once&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ),
    );
    let (connected, events) = until(&link, Duration::from_secs(3), |state, events| {
        matches!(state, StravaState::Connected { .. })
            || events
                .iter()
                .any(|event| matches!(event, StravaEvent::ConnectFailed { .. }))
    });
    assert!(
        matches!(connected, StravaState::Connected { .. }),
        "{connected:?} {events:?}"
    );
    let file = write_ride(&dir.0, "ride.fit", b"abc");
    link.upload(file, meta());
    let (state, more) = until(&link, Duration::from_secs(3), |_state, events| {
        events
            .iter()
            .any(|event| matches!(event, StravaEvent::UploadFailed { .. }))
    });
    let mut all = events;
    all.extend(more);
    let blob = format!("{link:?} {state:?} {all:?} {keys:?} {:?}", link.warnings());
    assert!(!blob.contains(SECRET), "client secret leaked");
    assert!(!blob.contains(ACCESS), "access token leaked");
    assert!(!blob.contains(REFRESH), "refresh token leaked");
}

#[test]
fn drop_stops_the_listener() {
    // Hold the preferred ports so this flow gets an ephemeral port that the
    // other tests are not racing for.
    let _held: Vec<TcpListener> = [53_682_u16, 53_683, 53_684, 53_685, 53_686]
        .into_iter()
        .filter_map(|port| TcpListener::bind(("127.0.0.1", port)).ok())
        .collect();
    let dir = TempDir::new();
    let link = StravaLink::open(dir.path(), Endpoints::strava());
    let url = link.begin_connect().expect("form");
    let port = port_of_url(&url);
    let started = Instant::now();
    drop(link);
    assert!(started.elapsed() < Duration::from_secs(2));
    let mut refused = false;
    for _ in 0..5 {
        let attempt = TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_millis(100),
        );
        if attempt.is_err() {
            refused = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        refused,
        "listener on {port} was still accepting connections"
    );
}

#[test]
fn was_uploaded_survives_reopening() {
    let dir = TempDir::new();
    let mock = Mock::spawn(|mut request| {
        let _ = read_body(&mut request);
        respond_json(request, 201, &upload_ready(44));
    });
    let path = dir.path();
    let base = mock.base.clone();
    let file = write_ride(&dir.0, "kept.fit", b"abc");
    {
        let link = connected(&dir, &base);
        link.upload(file.clone(), meta());
        let (_state, events) = until(&link, Duration::from_secs(3), |_state, events| {
            events
                .iter()
                .any(|event| matches!(event, StravaEvent::Uploaded { .. }))
        });
        assert!(events.iter().any(|event| matches!(
            event,
            StravaEvent::Uploaded {
                activity_id: 44,
                ..
            }
        )));
        assert!(link.was_uploaded(&file));
    }
    let link = StravaLink::open(path, endpoints_of(&base));
    assert!(link.was_uploaded(&file));
    let elsewhere = PathBuf::from("/tmp/other-dir").join("kept.fit");
    assert!(link.was_uploaded(&elsewhere));
    assert!(matches!(
        link.state(),
        StravaState::Connected { athlete } if athlete == "Ada Lovelace"
    ));
}

fn connected(dir: &TempDir, base: &str) -> StravaLink {
    seed_connected(&dir.path(), far_expiry());
    let link = StravaLink::open(dir.path(), endpoints_of(base));
    tune(&link);
    link
}

fn seed_connected(path: &Path, expires_at: u64) {
    fs::write(
        path,
        format!(
            "client_id = \"424242\"\nclient_secret = \"{SECRET}\"\naccess_token = \"live-access\"\nrefresh_token = \"live-refresh\"\nexpires_at = {expires_at}\nathlete = \"Ada Lovelace\"\n"
        ),
    )
    .unwrap();
}
