//! Acceptance tests for #741 item 2 (track F2, after D and F1):
//! `pjs_rate_limited_total{transport="http"}` counts requests the HTTP rate
//! limiter rejects, and `pjs_bytes_sent_total{transport="http"}` counts the
//! bytes `BatchFrameStream` emits (not the JSON frame endpoints).
//!
//! Counters only grow, so each test compares against a baseline scrape.

#![cfg_attr(
    all(feature = "http-server", feature = "metrics"),
    feature(impl_trait_in_assoc_type)
)]
#![cfg(all(feature = "http-server", feature = "metrics"))]

mod common;
mod http_metrics_support;

use std::time::Duration;

use axum::Router;
use futures::StreamExt;
use http_metrics_support::{scrape, send, value_of};
use pjson_rs::domain::entities::Frame;
use pjson_rs::domain::value_objects::StreamId;
use pjson_rs::infrastructure::http::RateLimitMiddleware;
use pjson_rs::infrastructure::http::axum_adapter::{
    create_pjs_router, create_pjs_router_with_rate_limit,
};
use pjson_rs::infrastructure::http::streaming::{BatchFrameStream, StreamFormat};
use pjson_rs::security::RateLimitConfig;

fn scrape_app() -> Router {
    create_pjs_router().with_state(common::create_test_app_state())
}

async fn counter(name: &str, transport: &str) -> f64 {
    value_of(
        &scrape(&scrape_app()).await,
        name,
        &[("transport", transport)],
    )
    .unwrap_or(0.0)
}

#[tokio::test]
async fn an_http_rate_limit_rejection_increments_the_rate_limited_counter() {
    let before = counter("pjs_rate_limited_total", "http").await;
    let limited = create_pjs_router_with_rate_limit(RateLimitMiddleware::new(RateLimitConfig {
        max_requests_per_window: 1,
        window_duration: Duration::from_secs(60),
        ..RateLimitConfig::default()
    }))
    .with_state(common::create_test_app_state());

    let first = send(&limited, "GET", "/pjs/health").await;
    let second = send(&limited, "GET", "/pjs/health").await;
    let third = send(&limited, "GET", "/pjs/health").await;

    assert_eq!(first.as_u16(), 200);
    assert_eq!((second.as_u16(), third.as_u16()), (429, 429));
    assert_eq!(
        counter("pjs_rate_limited_total", "http").await - before,
        2.0
    );
}

#[tokio::test]
async fn batch_frame_stream_adds_the_emitted_bytes_to_the_http_bytes_counter() {
    let before = counter("pjs_bytes_sent_total", "http").await;
    let stream_id = StreamId::new();
    let frames = vec![
        Frame::complete(stream_id, 1, None),
        Frame::complete(stream_id, 2, Some("checksum".to_owned())),
        Frame::complete(stream_id, 3, None),
    ];

    let chunks: Vec<Vec<u8>> =
        BatchFrameStream::new(futures::stream::iter(frames), StreamFormat::NdJson, 2)
            .into_stream()
            .map(|chunk| chunk.expect("serializes"))
            .collect()
            .await;
    let emitted: usize = chunks.iter().map(Vec::len).sum();

    assert_eq!(chunks.len(), 2, "batches of two frames, then the remainder");
    assert_eq!(
        counter("pjs_bytes_sent_total", "http").await - before,
        emitted as f64
    );
}
