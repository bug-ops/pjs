//! Acceptance test for #741 (track F1): building a router that mounts
//! `/metrics` installs the recorder, so requests served before the first
//! scrape are counted. Own binary: the recorder is process-global and this
//! test must make the first scrape of the process.

#![cfg_attr(
    all(feature = "http-server", feature = "metrics"),
    feature(impl_trait_in_assoc_type)
)]
#![cfg(all(feature = "http-server", feature = "metrics"))]

mod common;
mod http_metrics_support;

use http_metrics_support::{scrape, send, value_of};
use pjson_rs::infrastructure::http::axum_adapter::create_pjs_router;

#[tokio::test]
async fn requests_served_before_the_first_scrape_are_counted() {
    let app = create_pjs_router().with_state(common::create_test_app_state());

    send(&app, "GET", "/pjs/health").await;
    send(&app, "GET", "/pjs/health").await;
    let body = scrape(&app).await;

    let count = value_of(
        &body,
        "http_requests_total",
        &[
            ("method", "GET"),
            ("route", "/pjs/health"),
            ("status", "200"),
        ],
    );
    assert!(
        count.is_some_and(|count| count >= 2.0),
        "http_requests_total for /pjs/health: {count:?}\n{body}"
    );
}
