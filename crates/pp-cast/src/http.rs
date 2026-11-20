//! HTTP routes for the dashboard, the JSON API, and the telemetry stream.

use crate::CourseProfile;
use axum::extract::State;
use axum::http::header;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::stream::{self, Stream};
use pp_core::Telemetry;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// How often one browser is sent a new snapshot.
///
/// Five hertz sits in the product range of 4–10 updates per second. Publishing
/// faster than this still only forwards the latest value on the next tick.
const PUSH_INTERVAL: Duration = Duration::from_millis(200);

const DASHBOARD_HTML: &str = include_str!("dashboard.html");

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) telemetry: watch::Sender<Telemetry>,
    pub(crate) course: Arc<CourseProfile>,
    pub(crate) shutdown: watch::Sender<bool>,
}

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/api/telemetry", get(telemetry))
        .route("/api/course", get(course))
        .route("/events", get(events))
        .route("/healthz", get(health))
        .with_state(state)
}

fn no_store(response: impl IntoResponse) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], response)
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

async fn events(State(state): State<AppState>) -> impl IntoResponse {
    let stream = telemetry_stream(state.telemetry.subscribe(), state.shutdown.subscribe());
    let body = Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    );
    no_store(body)
}

fn telemetry_stream(
    telemetry: watch::Receiver<Telemetry>,
    shutdown: watch::Receiver<bool>,
) -> impl Stream<Item = Result<Event, Infallible>> + Send {
    let mut ticker = tokio::time::interval(PUSH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    stream::unfold(
        (telemetry, shutdown, ticker, None::<Telemetry>),
        |(telemetry, mut shutdown, mut ticker, mut last)| async move {
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            return None;
                        }
                    }
                    _ = ticker.tick() => {
                        let current = *telemetry.borrow();
                        if last == Some(current) {
                            continue;
                        }
                        last = Some(current);
                        let event = match Event::default().json_data(current) {
                            Ok(event) => event,
                            Err(_) => continue,
                        };
                        return Some((Ok(event), (telemetry, shutdown, ticker, last)));
                    }
                }
            }
        },
    )
}
