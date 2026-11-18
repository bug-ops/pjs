//! Prometheus recorder management.
//!
//! The recorder is a process-global singleton. This module installs it, renders
//! it for the `/metrics` endpoint, and provides the request-latency middleware
//! that every pjs router builder applies as its outermost layer.

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    extract::{MatchedPath, Request, State},
    http::Method,
    middleware::Next,
    response::Response,
};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

use crate::{
    domain::ports::{EventPublisherGat, StreamRepositoryGat, StreamStoreGat},
    infrastructure::http::{
        axum_adapter::{PjsAppState, PjsError},
        handlers::health::query_system_stats,
    },
};

static RECORDER: OnceLock<PrometheusHandle> = OnceLock::new();

const REQUESTS_TOTAL: &str = "http_requests_total";
const REQUEST_DURATION: &str = "http_request_duration_seconds";
const UNMATCHED_ROUTE: &str = "unmatched";
const DURATION_BUCKETS: [f64; 11] = [
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 10.0,
];
const UPKEEP_INTERVAL: Duration = Duration::from_secs(5);
const GAUGE_REFRESH_INTERVAL_MS: u64 = 1_000;
const RATE_LIMITED_TOTAL: &str = "pjs_rate_limited_total";
const BYTES_SENT_TOTAL: &str = "pjs_bytes_sent_total";
#[cfg(feature = "websocket-server")]
const WEBSOCKET_CONNECTIONS: &str = "pjs_websocket_connections";
const ACTIVE_SESSIONS: &str = "pjs_active_sessions";
const ACTIVE_STREAMS: &str = "pjs_active_streams";

/// Closed set of `transport` label values for the rate-limit and byte counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    Http,
    #[cfg(feature = "websocket-server")]
    WebSocket,
}

impl Transport {
    fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            #[cfg(feature = "websocket-server")]
            Self::WebSocket => "websocket",
        }
    }
}

/// Count one request or message rejected by a rate limiter.
pub(crate) fn record_rate_limited(transport: Transport) {
    metrics::counter!(RATE_LIMITED_TOTAL, "transport" => transport.as_str()).increment(1);
}

/// Add `len` bytes to the bytes-sent counter of `transport`.
pub(crate) fn record_bytes_sent(transport: Transport, len: usize) {
    let len = u64::try_from(len).unwrap_or(u64::MAX);
    metrics::counter!(BYTES_SENT_TOTAL, "transport" => transport.as_str()).increment(len);
}

/// Keeps `pjs_websocket_connections` equal to the number of live guards:
/// incremented on creation, decremented on drop.
#[cfg(feature = "websocket-server")]
#[derive(Debug)]
pub(crate) struct WebSocketConnectionGauge;

#[cfg(feature = "websocket-server")]
impl WebSocketConnectionGauge {
    pub(crate) fn open() -> Self {
        metrics::gauge!(WEBSOCKET_CONNECTIONS).increment(1.0);
        Self
    }
}

#[cfg(feature = "websocket-server")]
impl Drop for WebSocketConnectionGauge {
    fn drop(&mut self) {
        metrics::gauge!(WEBSOCKET_CONNECTIONS).decrement(1.0);
    }
}

/// Lets at most one scrape per second refresh the session and stream gauges,
/// so the unauthenticated `/metrics` cannot amplify session scans. One
/// instance lives in each [`PjsAppState`] and is shared by its clones.
#[derive(Debug)]
pub(crate) struct GaugeRefreshThrottle {
    epoch: Instant,
    /// Milliseconds since `epoch` of the last refresh plus one; 0 = never.
    last_refresh: AtomicU64,
}

impl GaugeRefreshThrottle {
    pub(crate) fn new() -> Self {
        Self {
            epoch: Instant::now(),
            last_refresh: AtomicU64::new(0),
        }
    }

    /// Returns `true` when the caller may refresh now; a caller that loses a
    /// race with a concurrent refresh gets `false`.
    fn try_acquire(&self) -> bool {
        let now = u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX - 1) + 1;
        let last = self.last_refresh.load(Ordering::Relaxed);
        if last != 0 && now.saturating_sub(last) < GAUGE_REFRESH_INTERVAL_MS {
            return false;
        }
        self.last_refresh
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    }

    /// Gives the slot back after a failed refresh so the next scrape retries.
    fn release(&self) {
        self.last_refresh.store(0, Ordering::Relaxed);
    }
}

/// Closed set of `method` label values, so an unknown extension method cannot
/// inflate label cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MethodLabel {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Other,
}

impl MethodLabel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
            Self::Other => "other",
        }
    }
}

impl From<&Method> for MethodLabel {
    fn from(method: &Method) -> Self {
        match *method {
            Method::GET => Self::Get,
            Method::HEAD => Self::Head,
            Method::POST => Self::Post,
            Method::PUT => Self::Put,
            Method::PATCH => Self::Patch,
            Method::DELETE => Self::Delete,
            Method::OPTIONS => Self::Options,
            _ => Self::Other,
        }
    }
}

/// Install the global Prometheus recorder, or return the handle from a prior
/// successful install.
///
/// The recorder exports `http_request_duration_seconds` as a true histogram
/// with explicit buckets. When a Tokio runtime is current, a background task
/// also runs the recorder's upkeep every few seconds so histograms do not grow
/// unboundedly. Outside a runtime (e.g. a router built in a synchronous
/// `main`) the task starts on the first call made inside one, at the latest
/// the first scrape; until then samples are buffered by the exporter.
/// [`metrics_handler`] additionally runs upkeep before rendering.
///
/// Every pjs router builder calls this while building, so building a router
/// with the `metrics` feature claims the process-global recorder: an embedder
/// that wants its own recorder must install it before building the router.
///
/// This function is idempotent on success: the first successful call wins, and
/// every subsequent call returns a clone of that same handle.
///
/// On failure, **nothing is cached** — the `OnceLock` remains empty and the
/// next call will retry the install. A transient failure (e.g. a competing
/// recorder installed by another library that is later removed) does not
/// permanently disable the `/metrics` endpoint.
///
/// # Errors
///
/// Returns [`PjsError::HttpError`] if `PrometheusBuilder::install_recorder()`
/// fails. The underlying error's `Display` text is logged server-side only,
/// once per process — callers that surface this error in an HTTP response
/// (e.g. [`metrics_handler`]) must not leak internal details to the client.
/// The once-per-process log guard matters because, on failure, the
/// `OnceLock` stays empty and this closure re-runs on every call; without
/// the guard, a sustained failure would log at ERROR on every call, and
/// `metrics_handler` calls this on every unauthenticated, unrate-limited
/// request to `/metrics`. When multiple callers race through the first
/// install, only one performs the actual install; on error all racing
/// callers observe the error and the cell stays empty.
pub fn install_global_recorder() -> Result<PrometheusHandle, PjsError> {
    RECORDER
        .get_or_try_init(|| {
            let handle = PrometheusBuilder::new()
                .set_buckets_for_metric(
                    Matcher::Full(REQUEST_DURATION.to_owned()),
                    &DURATION_BUCKETS,
                )
                .and_then(PrometheusBuilder::install_recorder)
                .map_err(|e| {
                    log_recorder_build_failure_once(&e);
                    PjsError::HttpError("failed to install metrics recorder".to_string())
                })?;
            Ok(handle)
        })
        .cloned()
        .inspect(spawn_upkeep_once)
}

/// Starts the upkeep task the first time a Tokio runtime is current. A router
/// built outside a runtime therefore starts it at the latest on the first
/// scrape, which always runs inside one.
fn spawn_upkeep_once(handle: &PrometheusHandle) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    if SPAWNED.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(runtime) = tokio::runtime::Handle::try_current()
        && !SPAWNED.swap(true, Ordering::Relaxed)
    {
        let handle = handle.clone();
        runtime.spawn(async move {
            let mut ticker = tokio::time::interval(UPKEEP_INTERVAL);
            loop {
                ticker.tick().await;
                handle.run_upkeep();
            }
        });
    }
}

/// Install the recorder while a router is being built, so requests served
/// before the first scrape are already counted.
///
/// A failure is logged once by [`install_global_recorder`] and otherwise
/// ignored: [`metrics_handler`] keeps retrying on every scrape.
pub(crate) fn install_for_router() {
    if let Err(error) = install_global_recorder() {
        tracing::debug!(%error, "metrics recorder unavailable while building router");
    }
}

/// Middleware recording `http_requests_total{method,route,status}` and
/// `http_request_duration_seconds{method,route}` for every request.
///
/// `route` is the matched route template (or `unmatched`), never the raw URI,
/// and `method` is limited to the [`MethodLabel`] set, so label cardinality is
/// bounded by the route table.
pub(crate) async fn request_metrics_middleware(request: Request, next: Next) -> Response {
    let method = MethodLabel::from(request.method()).as_str();
    let route = request.extensions().get::<MatchedPath>().map_or_else(
        || UNMATCHED_ROUTE.to_owned(),
        |path| path.as_str().to_owned(),
    );
    let started = std::time::Instant::now();

    let response = next.run(request).await;

    let elapsed = started.elapsed().as_secs_f64();
    metrics::counter!(
        REQUESTS_TOTAL,
        "method" => method,
        "route" => route.clone(),
        "status" => response.status().as_u16().to_string(),
    )
    .increment(1);
    metrics::histogram!(REQUEST_DURATION, "method" => method, "route" => route).record(elapsed);
    response
}

/// Log once (per process) the raw error from `PrometheusBuilder::install_recorder()`,
/// so a sustained failure doesn't flood logs at ERROR on every retry — see
/// [`install_global_recorder`]'s `# Errors` section for why the guard is needed.
fn log_recorder_build_failure_once(e: &impl std::fmt::Display) {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| {
        tracing::error!(error = %e, "failed to install Prometheus recorder (logged once)");
    });
}

/// Axum handler that renders current Prometheus metrics.
///
/// The recorder is normally installed when the router is built; the handler
/// installs it if that failed earlier, refreshes the `pjs_active_sessions` and
/// `pjs_active_streams` gauges from the system-stats query (at most once per
/// second per [`PjsAppState`]), runs upkeep, and renders. It takes the
/// application state so it mounts on any `Router<PjsAppState<R, P, S>>`.
///
/// `/metrics` is unauthenticated, so on installation failure the response
/// body must never expose the underlying error's `Display` text — the
/// error is logged server-side only and the client gets a generic message.
pub async fn metrics_handler<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
) -> impl axum::response::IntoResponse
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    match install_global_recorder() {
        Ok(handle) => {
            if state.metrics_throttle.try_acquire() && !refresh_state_gauges(&state).await {
                state.metrics_throttle.release();
            }
            handle.run_upkeep();
            let body = handle.render();
            axum::response::Response::builder()
                .status(axum::http::StatusCode::OK)
                .header(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; version=0.0.4; charset=utf-8",
                )
                .body(axum::body::Body::from(body))
                .unwrap_or_else(|_| {
                    axum::response::Response::builder()
                        .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                        .body(axum::body::Body::empty())
                        .expect("infallible empty response")
                })
        }
        Err(_) => axum::response::Response::builder()
            .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            .body(axum::body::Body::from("metrics recorder unavailable"))
            .expect("infallible error response"),
    }
}

/// Returns whether the gauges were refreshed.
async fn refresh_state_gauges<R, P, S>(state: &PjsAppState<R, P, S>) -> bool
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    match query_system_stats(state).await {
        Ok(stats) => {
            metrics::gauge!(ACTIVE_SESSIONS).set(stats.active_sessions as f64);
            metrics::gauge!(ACTIVE_STREAMS).set(stats.active_streams as f64);
            true
        }
        Err(error) => {
            tracing::warn!(%error, "failed to refresh state gauges");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent() {
        let h1 = install_global_recorder().expect("first install");
        let h2 = install_global_recorder().expect("second install");
        // The recorder is global and other tests record concurrently, so compare
        // a uniquely named counter instead of the whole output.
        metrics::counter!("pjs_test_install_idempotent_total").increment(1);
        let line = "pjs_test_install_idempotent_total 1";
        assert!(h1.render().contains(line));
        assert!(h2.render().contains(line));
    }

    #[test]
    fn throttle_admits_one_refresh_per_interval() {
        let throttle = GaugeRefreshThrottle::new();
        assert!(throttle.try_acquire());
        assert!(!throttle.try_acquire());
    }

    #[test]
    fn released_slot_is_available_again() {
        let throttle = GaugeRefreshThrottle::new();
        assert!(throttle.try_acquire());
        throttle.release();
        assert!(throttle.try_acquire());
    }

    #[test]
    fn unknown_methods_map_to_other() {
        let custom = Method::from_bytes(b"XCUSTOM").expect("valid token");
        assert_eq!(MethodLabel::from(&custom).as_str(), "other");
        assert_eq!(MethodLabel::from(&Method::PATCH).as_str(), "PATCH");
    }
}
