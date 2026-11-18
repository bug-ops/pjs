//! Acceptance tests for #706: the controller and transport methods that never
//! awaited are plain `fn`s, and `send_frame` reports what happened to the frame
//! as a `SendOutcome` instead of an always-`Ok` `Result`.
#![cfg(feature = "websocket-server")]

use pjson_rs::domain::SessionId;
use pjson_rs::infrastructure::byte_bounded_channel;
use pjson_rs::infrastructure::websocket::{
    AdaptiveStreamController, AxumWebSocketTransport, ConnectionId, DropReason, SendOutcome,
    StreamOptions, WsMessage,
};
use serde_json::json;
use std::time::Duration;

fn pong() -> WsMessage {
    WsMessage::Pong { timestamp: 7 }
}

#[test]
fn send_frame_without_a_connection_is_dropped_synchronously() {
    let transport = AxumWebSocketTransport::new();

    let outcome = transport.send_frame(ConnectionId::new(), pong());

    assert_eq!(outcome, SendOutcome::Dropped(DropReason::NoConnection));
}

#[tokio::test]
async fn send_frame_reports_queued_over_budget_and_unavailable_channel() {
    let transport = AxumWebSocketTransport::new();
    let controller = transport.controller();

    let queued = ConnectionId::new();
    let (tx, mut rx) = byte_bounded_channel::<String>(8, 1024 * 1024);
    controller.attach_connection(queued, tx);
    assert_eq!(transport.send_frame(queued, pong()), SendOutcome::Queued);
    let text = rx.recv().await.expect("frame queued").into_inner();
    assert!(matches!(
        serde_json::from_str::<WsMessage>(&text),
        Ok(WsMessage::Pong { timestamp: 7 })
    ));

    let tiny = ConnectionId::new();
    let (tx, _rx_tiny) = byte_bounded_channel::<String>(8, 4);
    controller.attach_connection(tiny, tx);
    assert_eq!(
        transport.send_frame(tiny, pong()),
        SendOutcome::Dropped(DropReason::OverBudget)
    );

    let closed = ConnectionId::new();
    let (tx, rx_closed) = byte_bounded_channel::<String>(8, 1024 * 1024);
    controller.attach_connection(closed, tx);
    drop(rx_closed);
    assert_eq!(
        transport.send_frame(closed, pong()),
        SendOutcome::Dropped(DropReason::ChannelUnavailable)
    );
}

#[tokio::test]
async fn controller_methods_that_never_awaited_are_synchronous() {
    let controller = AdaptiveStreamController::new();
    let owner = ConnectionId::new();
    let session = SessionId::new();
    controller
        .create_session(session, owner, json!({"k": "v"}), StreamOptions::default())
        .await
        .expect("session created");

    controller.start_streaming(owner, session).expect("starts");
    controller
        .handle_frame_ack(owner, session, 0, 10)
        .expect("ack for a live session");
    controller.cleanup_expired_sessions(Duration::from_secs(3600));
    assert!(controller.remove_session(owner, session));
    assert!(!controller.remove_session(owner, session));
    assert!(controller.handle_frame_ack(owner, session, 1, 10).is_err());
}

#[test]
fn transport_active_connection_count_is_synchronous() {
    let transport = AxumWebSocketTransport::new();

    assert_eq!(transport.active_connection_count(), 0);
}
