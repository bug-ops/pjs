//! Boundary of the inline-planning threshold (`INLINE_PLAN_MAX_BYTES` = 32 KiB)
//! and number-dense documents (#658, #676).
//!
//! The only blocking thread is held by a blocker, so a plan sent to the
//! blocking pool cannot finish before `create_session` is polled the first
//! time: the first poll is `Pending` exactly when planning goes off-thread, and
//! `Ready` when it runs inline. No timing or tick counting is involved.
#![cfg(feature = "websocket-server")]

use std::{future::Future, pin::pin, sync::mpsc, task::Poll, time::Duration};

use pjson_rs::domain::SessionId;
use pjson_rs::infrastructure::websocket::{AdaptiveStreamController, ConnectionId, StreamOptions};
use serde_json::Value;
use tokio::runtime::{Builder, Runtime};

const THRESHOLD: usize = 32 * 1024;
const WAIT: Duration = Duration::from_secs(10);

/// Where `create_session` planned a document.
#[derive(Debug, PartialEq, Eq)]
enum Planned {
    Inline,
    OffThread,
}

fn runtime() -> Runtime {
    Builder::new_current_thread()
        .max_blocking_threads(1)
        .enable_time()
        .build()
        .expect("runtime")
}

/// Creates a session for `document` while the blocking pool is occupied and
/// reports where it was planned. The call must still complete once the pool is
/// free, and is bounded by a timeout so a regression fails instead of hanging.
fn planned(document: Value) -> Planned {
    let rt = runtime();
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    rt.spawn_blocking(move || {
        let _ = started_tx.send(());
        let _ = release_rx.recv_timeout(WAIT);
    });
    started_rx
        .recv_timeout(WAIT)
        .expect("the blocker occupies the only blocking thread");

    rt.block_on(async {
        let controller = AdaptiveStreamController::new();
        let mut create = pin!(tokio::time::timeout(
            WAIT,
            controller.create_session(
                SessionId::new(),
                ConnectionId::new(),
                document,
                StreamOptions::default(),
            ),
        ));
        let first = std::future::poll_fn(|cx| Poll::Ready(create.as_mut().poll(cx))).await;
        match first {
            Poll::Ready(done) => {
                done.expect("no hang").expect("document is accepted");
                Planned::Inline
            }
            Poll::Pending => {
                release_tx.send(()).expect("blocker still waiting");
                create
                    .await
                    .expect("no hang once the pool is free")
                    .expect("document is accepted");
                Planned::OffThread
            }
        }
    })
}

/// A string root serializes to `len + 2` bytes.
fn string_document(serialized_len: usize) -> Value {
    Value::String("x".repeat(serialized_len - 2))
}

#[test]
fn test_document_exactly_at_the_threshold_plans_inline() {
    assert_eq!(planned(string_document(THRESHOLD)), Planned::Inline);
}

#[test]
fn test_document_one_byte_over_the_threshold_plans_off_thread() {
    assert_eq!(planned(string_document(THRESHOLD + 1)), Planned::OffThread);
}

#[test]
fn test_number_dense_document_far_over_the_threshold_plans_off_thread() {
    let floats = Value::Array(
        (1..=32_000)
            .map(|i| Value::from(f64::from(i) * 0.123_456_789_012_345_6))
            .collect(),
    );
    assert!(serde_json::to_vec(&floats).expect("serializable").len() > 10 * THRESHOLD);

    assert_eq!(planned(floats), Planned::OffThread);
}
