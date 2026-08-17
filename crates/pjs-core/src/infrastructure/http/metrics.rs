//! Prometheus recorder management.
//!
//! The recorder is a process-global singleton. This module exposes a single
//! function to install it and obtain a render handle that the `/metrics`
//! endpoint reads from.

use std::sync::OnceLock;

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

use crate::infrastructure::http::axum_adapter::PjsError;

static RECORDER: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the global Prometheus recorder, or return the handle from a prior
/// successful install.
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
            PrometheusBuilder::new().install_recorder().map_err(|e| {
                log_recorder_build_failure_once(&e);
                PjsError::HttpError("failed to install metrics recorder".to_string())
            })
        })
        .cloned()
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

/// Return an axum handler that renders current Prometheus metrics.
///
/// The handler installs the global recorder on the first call if not already
/// installed. Subsequent calls reuse the cached handle.
///
/// `/metrics` is unauthenticated, so on installation failure the response
/// body must never expose the underlying error's `Display` text — the
/// error is logged server-side only and the client gets a generic message.
pub async fn metrics_handler() -> impl axum::response::IntoResponse {
    match install_global_recorder() {
        Ok(handle) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent() {
        let h1 = install_global_recorder().expect("first install");
        let h2 = install_global_recorder().expect("second install");
        // PrometheusHandle does not implement PartialEq; assert observable
        // behavior instead: both handles render the same metrics text.
        assert_eq!(h1.render(), h2.render());
    }
}
