//! HTTP routes for the dashboard, the JSON API, and the telemetry stream.
//!
//! The three Barlow faces and their licence are compiled in from
//! `godot/assets/fonts/barlow-semi-condensed/`. They stay in that folder,
//! which the designer owns; the binary only embeds a copy at build time.

use crate::CourseProfile;
use crate::status::RideStatus;
use crate::zones::{grade_bands, power_zones};
use axum::extract::State;
use axum::http::header;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::stream::{self, Stream};
use pp_core::Telemetry;
use serde::Serialize;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// How often one browser is sent a new telemetry snapshot.
///
/// Four hertz matches the interface design (digits stay readable) and still
/// sits in the original 4–10 updates per second. Faster `publish` calls keep
/// only the latest value until the next tick.
const PUSH_INTERVAL: Duration = Duration::from_millis(250);

const DASHBOARD_HTML: &str = include_str!("dashboard.html");

const FONT_MEDIUM: &[u8] = include_bytes!(
    "../../../godot/assets/fonts/barlow-semi-condensed/BarlowSemiCondensed-Medium.ttf"
);
const FONT_SEMIBOLD: &[u8] = include_bytes!(
    "../../../godot/assets/fonts/barlow-semi-condensed/BarlowSemiCondensed-SemiBold.ttf"
);
const FONT_BOLD: &[u8] = include_bytes!(
    "../../../godot/assets/fonts/barlow-semi-condensed/BarlowSemiCondensed-Bold.ttf"
);
const FONT_LICENSE: &str =
    include_str!("../../../godot/assets/fonts/barlow-semi-condensed/OFL.txt");
const LOGO_SVG: &str = include_str!("../../../docs/img/logo.svg");

const FONT_CACHE: &str = "public, max-age=31536000";

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) telemetry: watch::Sender<Telemetry>,
    pub(crate) status: watch::Sender<RideStatus>,
    pub(crate) course: Arc<CourseProfile>,
    pub(crate) shutdown: watch::Sender<bool>,
}

#[derive(Serialize)]
struct ZoneFile {
    power: &'static [crate::ZoneStep],
    gradient: &'static [crate::GradeBand],
}

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/api/telemetry", get(telemetry))
        .route("/api/course", get(course))
        .route("/api/status", get(status))
        .route("/api/zones", get(zones))
        .route("/events", get(events))
        .route("/healthz", get(health))
        .route("/fonts/BarlowSemiCondensed-Medium.ttf", get(font_medium))
        .route(
            "/fonts/BarlowSemiCondensed-SemiBold.ttf",
            get(font_semibold),
        )
        .route("/fonts/BarlowSemiCondensed-Bold.ttf", get(font_bold))
        .route("/fonts/OFL.txt", get(license))
        .route("/logo.svg", get(logo))
        .with_state(state)
}

fn no_store(response: impl IntoResponse) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], response)
}

fn font_bytes(bytes: &'static [u8]) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "font/ttf"),
            (header::CACHE_CONTROL, FONT_CACHE),
        ],
        bytes,
    )
}

async fn font_medium() -> impl IntoResponse {
    font_bytes(FONT_MEDIUM)
}

async fn font_semibold() -> impl IntoResponse {
    font_bytes(FONT_SEMIBOLD)
}

async fn font_bold() -> impl IntoResponse {
    font_bytes(FONT_BOLD)
}

async fn license() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, FONT_CACHE),
        ],
        FONT_LICENSE,
    )
}

async fn logo() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, FONT_CACHE),
        ],
        LOGO_SVG,
    )
}

async fn dashboard() -> impl IntoResponse {
    no_store(Html(DASHBOARD_HTML))
}

async fn health() -> impl IntoResponse {
    (axum::http::StatusCode::OK, "ok")
}

async fn telemetry(State(state): State<AppState>) -> impl IntoResponse {
    let snapshot = *state.telemetry.borrow();
    no_store(Json(snapshot))
}

async fn course(State(state): State<AppState>) -> impl IntoResponse {
    no_store(Json(state.course.as_ref().clone()))
}

async fn status(State(state): State<AppState>) -> impl IntoResponse {
    no_store(Json(state.status.borrow().clone()))
}

async fn zones() -> impl IntoResponse {
    no_store(Json(ZoneFile {
        power: power_zones(),
        gradient: grade_bands(),
    }))
}

async fn events(State(state): State<AppState>) -> impl IntoResponse {
    let stream = live_stream(
        state.telemetry.subscribe(),
        state.status.subscribe(),
        state.shutdown.subscribe(),
    );
    let body = Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    );
    no_store(body)
}

fn live_stream(
    telemetry: watch::Receiver<Telemetry>,
    status: watch::Receiver<RideStatus>,
    shutdown: watch::Receiver<bool>,
) -> impl Stream<Item = Result<Event, Infallible>> + Send {
    let mut ticker = tokio::time::interval(PUSH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    stream::unfold(
        (telemetry, status, shutdown, ticker, None::<Telemetry>, true),
        |(telemetry, mut status, mut shutdown, mut ticker, mut last, mut send_status)| async move {
            loop {
                if send_status {
                    send_status = false;
                    let snapshot = status.borrow().clone();
                    if let Ok(event) = Event::default().event("status").json_data(&snapshot) {
                        return Some((
                            Ok(event),
                            (telemetry, status, shutdown, ticker, last, send_status),
                        ));
                    }
                }
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            return None;
                        }
                    }
                    changed = status.changed() => {
                        if changed.is_ok() {
                            send_status = true;
                        }
                    }
                    _ = ticker.tick() => {
                        let current = *telemetry.borrow();
                        if last == Some(current) {
                            continue;
                        }
                        last = Some(current);
                        let Ok(event) = Event::default().json_data(current) else {
                            continue;
                        };
                        return Some((
                            Ok(event),
                            (telemetry, status, shutdown, ticker, last, send_status),
                        ));
                    }
                }
            }
        },
    )
}
