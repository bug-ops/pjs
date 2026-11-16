//! Boundary of the inline-planning threshold (`INLINE_PLAN_MAX_BYTES` = 32 KiB)
//! and number-dense documents (#658).
//!
//! On a current-thread runtime a ticker task only runs when `create_session`
//! yields, which it does exactly when planning goes to the blocking pool.
#![cfg(feature = "websocket-server")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pjson_rs::domain::SessionId;
use pjson_rs::infrastructure::websocket::{AdaptiveStreamController, ConnectionId, StreamOptions};
use serde_json::Value;

const THRESHOLD: usize = 32 * 1024;

/// Ticks the ticker task got while `create_session` planned `document`.
async fn ticks_during_create(document: Value) -> usize {
    let controller = AdaptiveStreamController::new();
    let ticks = Arc::new(AtomicUsize::new(0));
    let ticker = {
        let ticks = Arc::clone(&ticks);
        tokio::spawn(async move {
            loop {
                ticks.fetch_add(1, Ordering::Relaxed);
                tokio::task::yield_now().await;
            }
        })
    };

    controller
        .create_session(
            SessionId::new(),
            ConnectionId::new(),
            document,
            StreamOptions::default(),
        )
        .await
        .expect("document is accepted");
    let ticks = ticks.load(Ordering::Relaxed);
    ticker.abort();
    ticks
}

/// A string root serializes to `len + 2` bytes.
fn string_document(serialized_len: usize) -> Value {
    Value::String("x".repeat(serialized_len - 2))
}

#[tokio::test(flavor = "current_thread")]
async fn test_document_exactly_at_the_threshold_plans_inline() {
    assert_eq!(ticks_during_create(string_document(THRESHOLD)).await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn test_document_one_byte_over_the_threshold_plans_off_thread() {
    assert!(ticks_during_create(string_document(THRESHOLD + 1)).await > 0);
}

#[tokio::test(flavor = "current_thread")]
async fn test_number_dense_document_far_over_the_threshold_plans_off_thread() {
    let floats = Value::Array(
        (1..=32_000)
            .map(|i| Value::from(f64::from(i) * 0.123_456_789_012_345_6))
            .collect(),
    );
    assert!(serde_json::to_vec(&floats).expect("serializable").len() > 10 * THRESHOLD);

    assert!(ticks_during_create(floats).await > 0);
}
