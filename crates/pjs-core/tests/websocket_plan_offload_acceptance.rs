//! Acceptance tests for #658: `create_session` plans a large document off the
//! async worker thread, with an unchanged plan and unchanged errors, and plans
//! inline when no Tokio runtime is present.
#![cfg(feature = "websocket-server")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use core::assert_matches;
use pjson_rs::domain::SessionId;
use pjson_rs::error::PlanError;
use pjson_rs::infrastructure::bounded_channel::{Envelope, byte_bounded_channel};
use pjson_rs::infrastructure::websocket::{
    AdaptiveStreamController, ConnectionId, CreateSessionError, Framing, StreamOptions,
};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc::Receiver;
use tokio::time::timeout;

mod wire_patch;
use wire_patch::apply_wire_patch;

const MIB: usize = 1024 * 1024;

/// About 4.4 MB of JSON: far above any plausible inline-planning threshold and
/// long enough (tens of milliseconds) to starve a single-threaded runtime.
fn large_document() -> Value {
    let entries: Map<String, Value> = (0..20_000)
        .map(|i| (format!("key{i:05}"), Value::String(format!("{i:0>200}"))))
        .collect();
    Value::Object(entries)
}

fn patch_options() -> StreamOptions {
    StreamOptions {
        framing: Framing::Patch,
        client_fps: Some(1000),
        ..StreamOptions::default()
    }
}

fn attached_controller() -> (
    AdaptiveStreamController,
    ConnectionId,
    Receiver<Envelope<String>>,
) {
    let controller = AdaptiveStreamController::new();
    let owner = ConnectionId::new();
    let (tx, rx) = byte_bounded_channel::<String>(1000, 32 * MIB);
    controller.attach_connection(owner, tx);
    (controller, owner, rx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_session_large_document_streams_the_complete_document() {
    let (controller, owner, mut rx) = attached_controller();
    let id = SessionId::new();
    let document = large_document();

    controller
        .create_session(id, owner, document.clone(), patch_options())
        .await
        .expect("large document is accepted");
    controller
        .start_streaming(owner, id)
        .await
        .expect("session exists");

    let mut state = json!({});
    let mut frame_ids = Vec::new();
    loop {
        let envelope = timeout(Duration::from_secs(60), rx.recv())
            .await
            .expect("stream stalled")
            .expect("channel closed");
        let message: Value = serde_json::from_str(&envelope.into_inner()).expect("JSON message");
        match message["type"].as_str().expect("type tag") {
            "StreamPatch" => {
                frame_ids.push(message["data"]["frame_id"].as_u64().expect("frame_id"));
                for op in message["data"]["patches"].as_array().expect("patches") {
                    apply_wire_patch(&mut state, op);
                }
            }
            "StreamComplete" => break,
            other => panic!("unexpected message type {other}"),
        }
    }

    assert_eq!(state, document);
    assert!(
        frame_ids.len() > 1,
        "a 4 MB document splits into several frames"
    );
    assert_eq!(
        frame_ids,
        (0..frame_ids.len() as u64).collect::<Vec<_>>(),
        "frame ids are sequential"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_create_session_large_document_lets_other_tasks_run_while_planning() {
    let (controller, owner, _rx) = attached_controller();
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
        .create_session(SessionId::new(), owner, large_document(), patch_options())
        .await
        .expect("large document is accepted");
    let ticks_during_planning = ticks.load(Ordering::Relaxed);
    ticker.abort();

    assert!(
        ticks_during_planning > 0,
        "the only worker thread was blocked for the whole plan"
    );
}

#[test]
fn test_create_session_large_document_plans_inline_without_a_tokio_runtime() {
    let controller = AdaptiveStreamController::new();

    let result = futures::executor::block_on(controller.create_session(
        SessionId::new(),
        ConnectionId::new(),
        large_document(),
        patch_options(),
    ));

    assert_matches!(result, Ok(()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_session_large_document_reports_plan_rejection_unchanged() {
    let controller = AdaptiveStreamController::new();
    let owner = ConnectionId::new();
    let mut deep = json!(0);
    for _ in 0..300 {
        deep = json!({ "a": deep });
    }
    let document = json!({ "blob": "x".repeat(MIB), "deep": deep });

    let result = controller
        .create_session(SessionId::new(), owner, document, patch_options())
        .await;

    assert_matches!(
        result,
        Err(CreateSessionError::PlanRejected(
            PlanError::PathTooDeep { .. }
        ))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_session_large_document_duplicate_id_is_rejected() {
    let controller = AdaptiveStreamController::new();
    let owner = ConnectionId::new();
    let id = SessionId::new();
    controller
        .create_session(id, owner, large_document(), patch_options())
        .await
        .expect("first session is accepted");

    let second = controller
        .create_session(id, owner, large_document(), patch_options())
        .await;

    assert_matches!(second, Err(CreateSessionError::AlreadyExists(existing)) if existing == id);
}
