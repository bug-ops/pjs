//! Integration tests for the `#406` pause/resume client-command protocol on
//! `websocket-streaming-server`'s `/ws/{session_id}` route.
//!
//! `ClientCommand`/`StreamControl` are private to `websocket_streaming.rs`, so
//! this drives the behavior end-to-end through the public router instead of
//! unit-testing the parser directly.

use std::net::SocketAddr;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use pjs_demo::servers::websocket_streaming::{AppState, app};
use pjson_rs::domain::value_objects::SessionId;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
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

/// Read text frames until one containing `needle` arrives, or the timeout elapses.
async fn wait_for_text_containing(
    ws_stream: &mut (
             impl StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin
         ),
    needle: &str,
    duration: Duration,
) -> bool {
    let found = timeout(duration, async {
        while let Some(msg) = ws_stream.next().await {
            if let Ok(WsMessage::Text(text)) = msg
                && text.contains(needle)
            {
                return true;
            }
        }
        false
    })
    .await;

    matches!(found, Ok(true))
}

#[tokio::test]
async fn pause_command_blocks_stream_completion() {
    let (state, addr) = spawn_server().await;
    let session_id = SessionId::new();
    state.add_session(session_id);

    let url = format!("ws://{addr}/ws/{session_id}");
    let (mut ws_stream, _) = connect_async(url).await.expect("upgrade should succeed");

    ws_stream
        .send(WsMessage::Text(r#"{"command":"pause"}"#.into()))
        .await
        .expect("sending pause command should succeed");

    // The demo stream sends 6 frames at ~10ms apart (~60ms total), so a pause
    // sent right after the handshake reliably lands before completion.
    let completed_while_paused = wait_for_text_containing(
        &mut ws_stream,
        "stream_complete",
        Duration::from_millis(150),
    )
    .await;
    assert!(
        !completed_while_paused,
        "stream must not complete while paused"
    );

    ws_stream
        .send(WsMessage::Text(r#"{"command":"resume"}"#.into()))
        .await
        .expect("sending resume command should succeed");

    let completed_after_resume =
        wait_for_text_containing(&mut ws_stream, "stream_complete", Duration::from_secs(2)).await;
    assert!(completed_after_resume, "stream must complete after resume");
}

#[tokio::test]
async fn rapid_pause_then_resume_does_not_hang_stream() {
    let (state, addr) = spawn_server().await;
    let session_id = SessionId::new();
    state.add_session(session_id);

    let url = format!("ws://{addr}/ws/{session_id}");
    let (mut ws_stream, _) = connect_async(url).await.expect("upgrade should succeed");

    ws_stream
        .send(WsMessage::Text(r#"{"command":"pause"}"#.into()))
        .await
        .expect("sending pause command should succeed");
    ws_stream
        .send(WsMessage::Text(r#"{"command":"resume"}"#.into()))
        .await
        .expect("sending resume command should succeed");

    let completed =
        wait_for_text_containing(&mut ws_stream, "stream_complete", Duration::from_secs(2)).await;
    assert!(
        completed,
        "stream must still complete after a pause immediately followed by resume"
    );
}

#[tokio::test]
async fn resume_without_prior_pause_is_a_no_op() {
    let (state, addr) = spawn_server().await;
    let session_id = SessionId::new();
    state.add_session(session_id);

    let url = format!("ws://{addr}/ws/{session_id}");
    let (mut ws_stream, _) = connect_async(url).await.expect("upgrade should succeed");

    ws_stream
        .send(WsMessage::Text(r#"{"command":"resume"}"#.into()))
        .await
        .expect("sending resume command should succeed");

    let completed =
        wait_for_text_containing(&mut ws_stream, "stream_complete", Duration::from_secs(2)).await;
    assert!(
        completed,
        "an unsolicited resume command must not disrupt normal streaming"
    );
}

/// Regression test for #428: a valid `command` tag with an extra unknown
/// field must be rejected, not silently accepted as the tagged variant.
/// Before the fix, `{"command":"pause","extra":true}` deserialized
/// successfully as `ClientCommand::Pause` and paused the stream; now it must
/// take the same malformed-command path as invalid JSON, leaving the stream
/// unpaused.
#[tokio::test]
async fn pause_command_with_unknown_field_is_rejected() {
    let (state, addr) = spawn_server().await;
    let session_id = SessionId::new();
    state.add_session(session_id);

    let url = format!("ws://{addr}/ws/{session_id}");
    let (mut ws_stream, _) = connect_async(url).await.expect("upgrade should succeed");

    ws_stream
        .send(WsMessage::Text(
            r#"{"command":"pause","extra":true}"#.into(),
        ))
        .await
        .expect("sending command with unknown field should succeed at the transport level");

    // If the unknown field were silently ignored, the stream would stay
    // paused and never emit `stream_complete` within the timeout.
    let completed =
        wait_for_text_containing(&mut ws_stream, "stream_complete", Duration::from_secs(2)).await;
    assert!(
        completed,
        "a command with an unknown field must be rejected, not accepted as pause"
    );
}

#[tokio::test]
async fn malformed_command_does_not_disrupt_streaming() {
    let (state, addr) = spawn_server().await;
    let session_id = SessionId::new();
    state.add_session(session_id);

    let url = format!("ws://{addr}/ws/{session_id}");
    let (mut ws_stream, _) = connect_async(url).await.expect("upgrade should succeed");

    ws_stream
        .send(WsMessage::Text("not valid json".into()))
        .await
        .expect("sending malformed command should succeed at the transport level");

    let completed =
        wait_for_text_containing(&mut ws_stream, "stream_complete", Duration::from_secs(2)).await;
    assert!(
        completed,
        "a malformed command must be ignored, not stall or close the stream"
    );
}
