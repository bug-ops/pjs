//! Acceptance test for #741 item 2 (track F2, after D and F1): `/metrics`
//! exports `pjs_active_sessions` and `pjs_active_streams`, refreshed at scrape
//! time from the same query as `GET /pjs/stats`.
//!
//! The gauges are process-global and each `PjsAppState` throttles its own
//! refresh, so this binary holds one test and scrapes through a fresh state
//! (same repository) each time it needs current values.

#![cfg_attr(
    all(feature = "http-server", feature = "metrics"),
    feature(impl_trait_in_assoc_type)
)]
#![cfg(all(feature = "http-server", feature = "metrics"))]

mod common;
mod http_metrics_support;

use std::sync::Arc;

use axum::Router;
use http_metrics_support::{scrape, send_with_body, value_of};
use pjson_rs::infrastructure::http::axum_adapter::{PjsAppState, create_pjs_router};
use serde_json::Value;

fn app_over(repository: &Arc<common::MockRepository>) -> Router {
    create_pjs_router().with_state(PjsAppState::new(
        repository.clone(),
        Arc::new(common::MockEventPublisher::new()),
        Arc::new(common::MockStreamStore::new()),
    ))
}

async fn create_session(app: &Router) -> String {
    let (status, body) = send_with_body(app, "POST", "/pjs/sessions", "{}").await;
    assert_eq!(status.as_u16(), 200, "{body}");
    let json: Value = serde_json::from_str(&body).expect("json");
    json["session_id"].as_str().expect("session_id").to_owned()
}

#[tokio::test]
async fn scrape_reports_the_active_session_and_stream_counts_of_the_system_stats() {
    let repository = Arc::new(common::MockRepository::new());
    let app = app_over(&repository);
    let first = create_session(&app).await;
    create_session(&app).await;
    let (status, body) = send_with_body(
        &app,
        "POST",
        &format!("/pjs/sessions/{first}/streams"),
        r#"{"data":{"a":1}}"#,
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{body}");

    let (_, stats) = send_with_body(&app, "GET", "/pjs/stats", "").await;
    let stats: Value = serde_json::from_str(&stats).expect("stats json");
    let sessions = stats["active_sessions"].as_u64().expect("active_sessions");
    let streams = stats["active_streams"].as_u64().expect("active_streams");
    assert_eq!(sessions, 2, "{stats}");
    assert!(streams >= 1, "{stats}");

    let scraped = scrape(&app_over(&repository)).await;

    assert_eq!(
        value_of(&scraped, "pjs_active_sessions", &[]),
        Some(sessions as f64),
        "{scraped}"
    );
    assert_eq!(
        value_of(&scraped, "pjs_active_streams", &[]),
        Some(streams as f64),
        "{scraped}"
    );

    create_session(&app).await;
    let rescraped = scrape(&app_over(&repository)).await;
    assert_eq!(
        value_of(&rescraped, "pjs_active_sessions", &[]),
        Some(3.0),
        "a fresh state refreshes the gauge on its first scrape:\n{rescraped}"
    );
}
