//! Integration tests for `websocket-streaming-server`'s `/ws/{session_id}` upgrade route.
//!
//! Regression coverage for #382: the route must bind to the session created via
//! `POST /stream`, reject unknown/malformed session ids, and reject a second
//! concurrent connection attempt for an already-bound session instead of
//! silently fanning the stream out to both.

use std::net::SocketAddr;

use futures::StreamExt;
use pjs_demo::servers::websocket_streaming::{AppState, app};
use pjson_rs::domain::value_objects::SessionId;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

/// Start the demo router on an OS-assigned port and return the state used to
/// seed sessions plus the address to connect to.
async fn spawn_server() -> (AppState, SocketAddr) {
    let state = AppState::new();
    let router = app(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    (state, addr)
}

#[tokio::test]
async fn valid_session_upgrades_and_streams() {
    let (state, addr) = spawn_server().await;
    let session_id = SessionId::new();
    state.add_session(session_id);

    let url = format!("ws://{addr}/ws/{session_id}");
    let (mut ws_stream, response) = connect_async(url)
        .await
        .expect("a known, unbound session should upgrade successfully");
    assert_eq!(response.status(), 101);

    let frame = ws_stream
        .next()
        .await
        .expect("connection closed before any frame arrived")
        .expect("frame read error");
    match frame {
        WsMessage::Text(text) => assert!(text.contains("pjs_frame"), "unexpected payload: {text}"),
        other => panic!("expected a text frame, got {other:?}"),
    }
}

#[tokio::test]
async fn unknown_session_is_rejected_with_404() {
    let (_state, addr) = spawn_server().await;
    let session_id = SessionId::new();

    let url = format!("ws://{addr}/ws/{session_id}");
    let err = connect_async(url)
        .await
        .expect_err("a session never created via POST /stream must be rejected");
    match err {
        WsError::Http(response) => assert_eq!(response.status(), 404),
        other => panic!("expected an HTTP rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn malformed_session_id_is_rejected_with_400() {
    let (_state, addr) = spawn_server().await;

    let url = format!("ws://{addr}/ws/not-a-valid-uuid");
    let err = connect_async(url)
        .await
        .expect_err("a malformed session id must be rejected");
    match err {
        WsError::Http(response) => assert_eq!(response.status(), 400),
        other => panic!("expected an HTTP rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn concurrent_connect_to_same_session_is_rejected_with_409() {
    let (state, addr) = spawn_server().await;
    let session_id = SessionId::new();
    state.add_session(session_id);

    let url = format!("ws://{addr}/ws/{session_id}");
    let (result_a, result_b) = tokio::join!(connect_async(url.clone()), connect_async(url));

    fn status_of(
        result: Result<
            (
                impl Sized,
                tokio_tungstenite::tungstenite::handshake::client::Response,
            ),
            WsError,
        >,
    ) -> Option<u16> {
        match result {
            Ok((_, response)) => Some(response.status().as_u16()),
            Err(WsError::Http(response)) => Some(response.status().as_u16()),
            Err(_) => None,
        }
    }
    let statuses = [status_of(result_a), status_of(result_b)];

    let successes = statuses.iter().filter(|s| **s == Some(101)).count();
    let conflicts = statuses.iter().filter(|s| **s == Some(409)).count();
    assert_eq!(
        successes, 1,
        "exactly one concurrent connect should win the bind: {statuses:?}"
    );
    assert_eq!(
        conflicts, 1,
        "exactly one concurrent connect should be rejected as a conflict: {statuses:?}"
    );
}
