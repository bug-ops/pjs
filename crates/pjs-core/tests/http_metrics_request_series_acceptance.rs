//! Acceptance tests for #741 (track F1): `/metrics` exports
//! `http_requests_total{method,route,status}` and the
//! `http_request_duration_seconds` histogram, with bounded label values
//! (spec 065): the matched route template, a closed set of methods, never the
//! raw URI.
//!
//! One recorder per process: every test here uses request shapes no other test
//! in this file produces.

#![cfg_attr(
    all(feature = "http-server", feature = "metrics"),
    feature(impl_trait_in_assoc_type)
)]
#![cfg(all(feature = "http-server", feature = "metrics"))]

mod common;
mod http_metrics_support;

use axum::Router;
use http_metrics_support::{label_values, lines, scrape, send, value_of};
use pjson_rs::infrastructure::http::axum_adapter::create_pjs_router;

const METHODS: [&str; 8] = [
    "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "other",
];

fn app() -> Router {
    create_pjs_router().with_state(common::create_test_app_state())
}

#[tokio::test]
async fn a_matched_route_is_labelled_with_its_template_not_the_raw_uri() {
    let app = app();
    let session = "123e4567-e89b-12d3-a456-426614174999";

    let status = send(&app, "GET", &format!("/pjs/sessions/{session}")).await;
    let body = scrape(&app).await;

    assert_eq!(status.as_u16(), 404);
    assert_eq!(
        value_of(
            &body,
            "http_requests_total",
            &[
                ("method", "GET"),
                ("route", "/pjs/sessions/{session_id}"),
                ("status", "404"),
            ],
        ),
        Some(1.0),
        "{body}"
    );
    assert!(!body.contains(session), "the raw id leaked into the labels");
}

#[tokio::test]
async fn an_unmatched_request_is_labelled_unmatched_without_its_uri() {
    let app = app();

    send(&app, "GET", "/no-such-route-zq81/secret-token-7f3").await;
    let body = scrape(&app).await;

    assert!(
        value_of(
            &body,
            "http_requests_total",
            &[("route", "unmatched"), ("status", "404")],
        )
        .is_some_and(|count| count >= 1.0),
        "{body}"
    );
    assert!(!body.contains("secret-token-7f3"));
    assert!(!body.contains("no-such-route-zq81"));
}

#[tokio::test]
async fn an_unknown_method_is_recorded_as_other_and_never_by_name() {
    let app = app();

    send(&app, "GET", "/pjs/health").await;
    send(&app, "XCUSTOMMETHODQ", "/pjs/health").await;
    send(&app, "POST", "/pjs/health").await;
    let body = scrape(&app).await;

    assert!(
        value_of(&body, "http_requests_total", &[("method", "other")]).is_some(),
        "{body}"
    );
    assert!(!body.contains("XCUSTOMMETHODQ"));
    for method in label_values(&body, "http_requests_total", "method") {
        assert!(
            METHODS.contains(&method.as_str()),
            "unexpected method {method}"
        );
    }
}

#[tokio::test]
async fn the_request_duration_is_a_histogram_with_buckets_not_a_summary() {
    let app = app();

    send(&app, "OPTIONS", "/pjs/health").await;
    let body = scrape(&app).await;

    let buckets = lines(&body, "http_request_duration_seconds_bucket");
    assert!(
        buckets.iter().any(|line| line.contains("le=")),
        "no histogram buckets:\n{body}"
    );
    assert!(
        lines(&body, "http_request_duration_seconds_count")
            .iter()
            .any(|line| line.contains(r#"method="OPTIONS""#)),
        "{body}"
    );
    assert!(
        lines(&body, "http_request_duration_seconds")
            .iter()
            .all(|line| !line.contains("quantile=")),
        "the duration is rendered as a summary"
    );
}

#[tokio::test]
async fn the_duration_histogram_carries_no_status_label() {
    let app = app();

    send(&app, "PUT", "/pjs/health").await;
    let body = scrape(&app).await;

    let counts = lines(&body, "http_request_duration_seconds_count");
    assert!(!counts.is_empty(), "{body}");
    assert!(
        counts.iter().all(|line| !line.contains("status=")),
        "{counts:?}"
    );
}
