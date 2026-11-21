//! HTTP contract for the TV dashboard. Each test binds `127.0.0.1:0`.

use futures_util::StreamExt;
use pp_cast::{
    CastConfig, CastError, CastServer, CourseProfile, LinkPhase, ProfilePoint, ResistanceMode,
    RideSource, RideStatus,
};
use pp_core::Telemetry;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

fn start(course: CourseProfile) -> CastServer {
    CastServer::start(CastConfig {
        bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        course,
    })
    .expect("start test server")
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .no_proxy()
        .build()
        .expect("http client")
}

async fn send_get(client: &reqwest::Client, url: &str) -> reqwest::Response {
    let mut last_error = None;
    for _ in 0..25 {
        match client.get(url).send().await {
            Ok(response) => return response,
            Err(err) => {
                last_error = Some(err);
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }
    panic!("GET {url} failed: {last_error:?}");
}

fn sample(power_w: f32) -> Telemetry {
    Telemetry {
        power_w,
        cadence_rpm: Some(90.0),
        heart_rate_bpm: Some(150),
        speed_mps: 8.5,
        distance_m: 1200.0,
        lap_distance_m: 400.0,
        lap: 1,
        grade_pct: 4.0,
        elevation_m: 88.0,
        elapsed_s: 180.0,
    }
}

fn course() -> CourseProfile {
    CourseProfile {
        name: "Test Loop".to_owned(),
        length_m: 1000.0,
        points: vec![
            ProfilePoint {
                distance_m: 0.0,
                elevation_m: 10.0,
            },
            ProfilePoint {
                distance_m: 500.0,
                elevation_m: 40.0,
            },
            ProfilePoint {
                distance_m: 1000.0,
                elevation_m: 12.0,
            },
        ],
    }
}

fn drain_events(buf: &mut Vec<u8>) -> Vec<Telemetry> {
    let mut events = Vec::new();
    while let Some(split) = find_frame_end(buf) {
        let frame: Vec<u8> = buf.drain(..split).collect();
        if let Some(telemetry) = parse_frame(&frame) {
            events.push(telemetry);
        }
    }
    events
}

fn find_frame_end(buf: &[u8]) -> Option<usize> {
    if let Some(index) = buf.windows(4).position(|frame| frame == b"\r\n\r\n") {
        return Some(index + 4);
    }
    buf.windows(2)
        .position(|pair| pair == b"\n\n")
        .map(|index| index + 2)
}

fn parse_frame(frame: &[u8]) -> Option<Telemetry> {
    let text = std::str::from_utf8(frame).ok()?;
    let mut data = String::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("data:") else {
            continue;
        };
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(rest.trim_start());
    }
    if data.is_empty() {
        None
    } else {
        serde_json::from_str(&data).ok()
    }
}

fn spawn_reader<S, B, E>(mut stream: S) -> tokio::sync::mpsc::UnboundedReceiver<Telemetry>
where
    S: futures_util::Stream<Item = Result<B, E>> + Send + Unpin + 'static,
    B: AsRef<[u8]> + Send,
    E: Send,
{
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut buf = Vec::new();
        while let Some(item) = stream.next().await {
            let Ok(chunk) = item else {
                break;
            };
            buf.extend_from_slice(chunk.as_ref());
            for event in drain_events(&mut buf) {
                if tx.send(event).is_err() {
                    return;
                }
            }
        }
    });
    rx
}

async fn recv_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Telemetry>,
    wait: Duration,
) -> Option<Telemetry> {
    tokio::time::timeout(wait, rx.recv())
        .await
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_html_lists_the_tiles() {
    let server = start(course());
    let client = client();
    let response = send_get(&client, &format!("http://{}/", server.local_addr())).await;
    assert_eq!(response.status(), 200);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.starts_with("text/html"),
        "content-type {content_type}"
    );
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .map(|v| v.as_bytes()),
        Some(b"no-store".as_slice())
    );
    let html = response.text().await.expect("html body");
    for label in [
        "Power",
        "Speed",
        "Cadence",
        "Heart rate",
        "Grade",
        "Distance",
        "Lap",
        "Elapsed",
        "Elevation",
    ] {
        assert!(html.contains(label), "missing tile label {label}");
    }
    assert!(html.contains("reconnecting"));
    assert!(html.contains("EventSource"));
    assert!(!html.contains("https://"));
    assert!(!html.contains("http://"));
    assert!(!html.contains("<script src"));
    assert!(!html.contains("<link"));
    assert!(!html.contains("?."));
    assert!(!html.contains("??"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn course_round_trips() {
    let expected = course();
    let server = start(expected.clone());
    let client = client();
    let response = send_get(
        &client,
        &format!("http://{}/api/course", server.local_addr()),
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .map(|v| v.as_bytes()),
        Some(b"no-store".as_slice())
    );
    let got: CourseProfile = response.json().await.expect("course json");
    assert_eq!(got, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn telemetry_reflects_the_latest_publish() {
    let server = start(CourseProfile::default());
    let client = client();
    let url = format!("http://{}/api/telemetry", server.local_addr());

    server.publish(Telemetry {
        power_w: 10.0,
        cadence_rpm: None,
        heart_rate_bpm: None,
        ..Telemetry::default()
    });
    let missing = send_get(&client, &url).await;
    assert_eq!(missing.status(), 200);
    let missing_body = missing.text().await.expect("telemetry json");
    assert!(
        missing_body.contains("\"cadence_rpm\":null"),
        "{missing_body}"
    );
    assert!(
        missing_body.contains("\"heart_rate_bpm\":null"),
        "{missing_body}"
    );

    let expected = sample(240.0);
    server.publish(expected);
    let response = send_get(&client, &url).await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .map(|v| v.as_bytes()),
        Some(b"no-store".as_slice())
    );
    let got: Telemetry = response.json().await.expect("telemetry json");
    assert_eq!(got, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_deliver_a_snapshot_and_a_later_update() {
    let server = start(course());
    let first = sample(111.0);
    let second = Telemetry {
        power_w: 222.0,
        lap_distance_m: 750.0,
        heart_rate_bpm: Some(160),
        ..first
    };
    server.publish(first);

    let client = client();
    let response = send_get(&client, &format!("http://{}/events", server.local_addr())).await;
    assert_eq!(response.status(), 200);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.starts_with("text/event-stream"),
        "{content_type}"
    );
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .map(|v| v.as_bytes()),
        Some(b"no-store".as_slice())
    );

    let mut rx = spawn_reader(response.bytes_stream());
    let got_first = recv_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("first event");
    assert_eq!(got_first, first);

    server.publish(second);
    let got_second = recv_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("second event");
    assert_eq!(got_second, second);

    let addr = server.local_addr();
    server.shutdown();
    std::net::TcpListener::bind(addr).expect("port free after shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publishing_a_burst_is_throttled() {
    let server = start(CourseProfile::default());
    server.publish(Telemetry {
        power_w: 7.0,
        ..Telemetry::default()
    });

    let client = client();
    let response = send_get(&client, &format!("http://{}/events", server.local_addr())).await;
    assert_eq!(response.status(), 200);
    let mut rx = spawn_reader(response.bytes_stream());
    let initial = recv_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("initial event");
    assert_eq!(initial.power_w, 7.0);

    for i in 0..1000 {
        server.publish(Telemetry {
            power_w: 1000.0 + i as f32,
            elapsed_s: f64::from(i),
            ..Telemetry::default()
        });
    }

    let mut count = 0usize;
    let mut saw_final = false;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(event)) => {
                count += 1;
                if event.power_w == 1999.0 {
                    saw_final = true;
                    break;
                }
            }
            _ => break,
        }
    }
    assert!(
        saw_final,
        "coalesced snapshot was not delivered (events={count})"
    );
    assert!(
        count < 25,
        "expected far fewer than 1000 events, got {count}"
    );
}

#[test]
fn bind_in_use_port_returns_err() {
    let listener = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .expect("reserve a port");
    let addr = listener.local_addr().expect("reserved address");
    let error = CastServer::start(CastConfig {
        bind: addr,
        course: CourseProfile::default(),
    })
    .expect_err("in-use port must fail");
    assert!(matches!(error, CastError::Bind(_)), "{error}");
    drop(listener);
}

#[test]
fn drop_frees_the_port() {
    let addr = {
        let server = start(CourseProfile::default());
        let addr = server.local_addr();
        drop(server);
        addr
    };
    let rebound = std::net::TcpListener::bind(addr).expect("port free after drop");
    drop(rebound);
}

#[test]
fn urls_contain_the_bound_port() {
    let server = start(CourseProfile::default());
    let port = server.local_addr().port();
    assert_ne!(port, 0);
    assert_eq!(server.urls(), vec![format!("http://127.0.0.1:{port}/")]);
}

fn sample_status() -> RideStatus {
    RideStatus {
        ftp_w: Some(250),
        source: RideSource::Trainer,
        trainer_name: Some("Tacx Neo 2T".to_owned()),
        link: LinkPhase::Connected,
        resistance: Some(ResistanceMode::Controlled),
        power_3s_w: Some(345.0),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_starts_neutral_then_reflects_publish_status() {
    let server = start(CourseProfile::default());
    let client = client();
    let url = format!("http://{}/api/status", server.local_addr());
    let first: RideStatus = send_get(&client, &url).await.json().await.expect("status");
    assert_eq!(first, RideStatus::default());
    assert_eq!(first.source, RideSource::Unknown);
    assert!(first.ftp_w.is_none());

    let status = sample_status();
    server.publish_status(status.clone());
    let got: RideStatus = send_get(&client, &url).await.json().await.expect("status");
    assert_eq!(got, status);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_event_is_separate_from_telemetry_events() {
    let server = start(CourseProfile::default());
    let status = sample_status();
    server.publish_status(status.clone());
    server.publish(sample(180.0));

    let client = client();
    let response = send_get(&client, &format!("http://{}/events", server.local_addr())).await;
    assert_eq!(response.status(), 200);
    let mut stream = response.bytes_stream();
    let mut buf = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_status = false;
    let mut saw_telemetry = false;
    while Instant::now() < deadline && !(saw_status && saw_telemetry) {
        let left = deadline.saturating_duration_since(Instant::now());
        let Ok(Some(Ok(chunk))) = tokio::time::timeout(left, stream.next()).await else {
            break;
        };
        buf.extend_from_slice(&chunk);
        let text = String::from_utf8_lossy(&buf);
        if text.contains("event: status") {
            saw_status = true;
        }
        if text.contains("\"power_w\":180") || text.contains("\"power_w\":180.0") {
            saw_telemetry = true;
        }
    }
    assert!(
        saw_status,
        "missing status event: {}",
        String::from_utf8_lossy(&buf)
    );
    assert!(saw_telemetry, "telemetry event changed shape");
    let body = String::from_utf8_lossy(&buf);
    let status_json = body
        .split("event: status")
        .nth(1)
        .and_then(|rest| rest.split("data:").nth(1))
        .map(|data| data.split("\n\n").next().unwrap_or("").trim().to_owned())
        .expect("status payload");
    let parsed: RideStatus = serde_json::from_str(&status_json).expect("status json");
    assert_eq!(parsed, status);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn telemetry_json_keeps_the_original_fields() {
    let server = start(CourseProfile::default());
    server.publish_status(sample_status());
    server.publish(sample(215.0));
    let client = client();
    let body = send_get(
        &client,
        &format!("http://{}/api/telemetry", server.local_addr()),
    )
    .await
    .text()
    .await
    .expect("telemetry body");
    let value: serde_json::Value = serde_json::from_str(&body).expect("json");
    for key in [
        "power_w",
        "cadence_rpm",
        "heart_rate_bpm",
        "speed_mps",
        "distance_m",
        "lap_distance_m",
        "lap",
        "grade_pct",
        "elevation_m",
        "elapsed_s",
    ] {
        assert!(value.get(key).is_some(), "missing {key} in {body}");
    }
    assert!(value.get("ftp_w").is_none(), "status leaked into telemetry");
    assert!(
        value.get("power_3s_w").is_none(),
        "status leaked into telemetry"
    );
    let decoded: Telemetry = serde_json::from_str(&body).expect("telemetry");
    assert_eq!(decoded, sample(215.0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn font_and_licence_are_served_from_this_origin() {
    let server = start(CourseProfile::default());
    let client = client();
    let base = format!("http://{}", server.local_addr());
    for path in [
        "/fonts/BarlowSemiCondensed-Medium.ttf",
        "/fonts/BarlowSemiCondensed-SemiBold.ttf",
        "/fonts/BarlowSemiCondensed-Bold.ttf",
    ] {
        let response = send_get(&client, &format!("{base}{path}")).await;
        assert_eq!(response.status(), 200, "{path}");
        let kind = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(kind.contains("font/ttf"), "{path} content-type {kind}");
        let cache = response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(cache.contains("max-age"), "{path} cache {cache}");
        let bytes = response.bytes().await.expect("font bytes");
        assert!(bytes.len() > 1000, "{path} was empty");
    }
    let licence = send_get(&client, &format!("{base}/fonts/OFL.txt")).await;
    assert_eq!(licence.status(), 200);
    assert!(
        licence
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .contains("text/plain")
    );
    let text = licence.text().await.expect("licence");
    assert!(text.contains("SIL Open Font License"));
    assert!(!text.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_has_no_external_urls() {
    let server = start(CourseProfile::default());
    let client = client();
    let html = send_get(&client, &format!("http://{}/", server.local_addr()))
        .await
        .text()
        .await
        .expect("html");
    assert!(!html.contains("http://"), "{html}");
    assert!(!html.contains("https://"), "{html}");
    assert!(html.contains("/fonts/BarlowSemiCondensed-Bold.ttf"));
    assert!(html.contains("/logo.svg"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn served_zones_match_the_design_table() {
    let server = start(CourseProfile::default());
    let client = client();
    let body = send_get(
        &client,
        &format!("http://{}/api/zones", server.local_addr()),
    )
    .await
    .text()
    .await
    .expect("zones");
    let value: serde_json::Value = serde_json::from_str(&body).expect("json");
    let power = value["power"].as_array().expect("power zones");
    let expected = [
        (1, "Recovery", "Z1 RECOVERY", 0.0, "#9AA8BD"),
        (2, "Endurance", "Z2 ENDURANCE", 0.55, "#4FA8FF"),
        (3, "Tempo", "Z3 TEMPO", 0.75, "#3DD68C"),
        (4, "Threshold", "Z4 THRESHOLD", 0.90, "#FFD166"),
        (5, "VO2 max", "Z5 VO2 MAX", 1.05, "#FF9A3C"),
        (6, "Anaerobic", "Z6 ANAEROBIC", 1.20, "#FF5A52"),
        (7, "Sprint", "Z7 SPRINT", 1.50, "#D277FF"),
    ];
    assert_eq!(power.len(), expected.len());
    for (step, (zone, name, label, above, color)) in power.iter().zip(expected) {
        assert_eq!(step["zone"].as_u64(), Some(zone));
        assert_eq!(step["name"].as_str(), Some(name));
        assert_eq!(step["label"].as_str(), Some(label));
        let bound = step["above_ftp"].as_f64().expect("bound");
        assert!((bound - above).abs() < 1e-6, "{bound} vs {above}");
        assert_eq!(step["color"].as_str(), Some(color));
    }
    let gradient = value["gradient"].as_array().expect("gradient");
    let grades = [-1000.0, -1.0, 1.0, 3.0, 6.0, 9.0, 12.0];
    assert_eq!(gradient.len(), grades.len());
    for (band, from) in gradient.iter().zip(grades) {
        let got = band["from_pct"].as_f64().expect("from");
        assert!((got - from).abs() < 1e-6);
    }
}
