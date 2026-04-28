//! Wire-level WebSocket integration tests.
//!
//! These tests bind a real TCP socket, perform an actual HTTP upgrade handshake,
//! and exchange real WebSocket frames with `AxumWebSocketTransport`. They cover
//! code paths (protocol upgrade, frame routing, connection cleanup) that
//! struct-level unit tests in `websocket_server_comprehensive.rs` cannot reach.
#![cfg(all(feature = "http-server", feature = "websocket-client"))]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use pjson_rs::infrastructure::websocket::{
    AxumWebSocketTransport, WsMessage, server::create_websocket_router,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Spawn the WebSocket router on a random localhost port.
///
/// The TCP listener is bound before `tokio::spawn` returns, so callers may
/// immediately issue `connect_async` — the kernel queues the SYN until the
/// accept loop runs.
async fn spawn_ws_test_server() -> (SocketAddr, Arc<AxumWebSocketTransport>) {
    let transport = Arc::new(AxumWebSocketTransport::new());
    let app = create_websocket_router().with_state(transport.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind random port");
    let addr = listener.local_addr().expect("local_addr");

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (addr, transport)
}

fn ws_url(addr: SocketAddr) -> String {
    format!("ws://{addr}/ws")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Verify that `create_websocket_router` responds to a real HTTP upgrade with
/// status 101 Switching Protocols.
#[tokio::test]
async fn test_wire_upgrade_handshake() {
    let (addr, transport) = spawn_ws_test_server().await;

    let (_, response) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    assert_eq!(
        response.status(),
        101,
        "expected HTTP 101 Switching Protocols"
    );

    // The test holds one Arc reference; the spawned task holds at least one more.
    assert!(
        Arc::strong_count(&transport) >= 2,
        "expected at least two Arc references"
    );
}

/// Verify that the server responds to a protocol-level Ping frame with a
/// matching Pong frame.
///
/// NOTE: `WsMessage::Ping` (application-level JSON) is intentionally NOT used
/// here. The server logs it at debug level but does not echo a `WsMessage::Pong`
/// back. Only the WebSocket protocol-level ping handler (server.rs:125-130)
/// sends a Pong.
#[tokio::test]
async fn test_wire_protocol_ping_pong() {
    let (addr, _transport) = spawn_ws_test_server().await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    ws.send(Message::Ping(b"hello".to_vec().into()))
        .await
        .expect("send ping");

    let frame = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out waiting for pong")
        .expect("stream ended")
        .expect("WebSocket error");

    match frame {
        Message::Pong(payload) => {
            assert_eq!(payload.as_ref(), b"hello", "pong payload must echo ping");
        }
        other => panic!("expected Pong, got {:?}", other),
    }
}

/// Verify that a `StreamInit` text message causes the server to stream at least
/// one `StreamFrame` back over the wire.
#[tokio::test]
async fn test_wire_stream_init_yields_frame() {
    let (addr, _transport) = spawn_ws_test_server().await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    // The server ignores the client-supplied session_id and generates a fresh one.
    let init = json!({
        "type": "StreamInit",
        "data": {
            "session_id": "client-supplied-ignored",
            "data": {
                "critical": {"id": 1},
                "metadata": {"ts": "2026-04-28"}
            },
            "options": {
                "max_frame_size": 65536,
                "client_fps": null,
                "compression": false,
                "priority_mapping": null
            }
        }
    });

    ws.send(Message::Text(init.to_string().into()))
        .await
        .expect("send StreamInit");

    // Collect messages until we find a StreamFrame or time out.
    let mut received_stream_frame = false;

    let result = timeout(Duration::from_secs(5), async {
        while let Some(msg) = ws.next().await {
            let msg = msg.expect("WebSocket error");
            if let Message::Text(text) = msg
                && let Ok(WsMessage::StreamFrame { session_id, .. }) =
                    serde_json::from_str::<WsMessage>(&text)
            {
                assert!(!session_id.is_empty(), "session_id must be non-empty");
                received_stream_frame = true;
                break;
            }
        }
    })
    .await;

    result.expect("timed out waiting for StreamFrame");
    assert!(received_stream_frame, "never received a StreamFrame");
}

/// Verify that after a client-initiated close the server cleans up the
/// connection record.
#[tokio::test]
async fn test_wire_clean_close() {
    let (addr, transport) = spawn_ws_test_server().await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    ws.send(Message::Close(None)).await.expect("send Close");

    // The server should echo a Close frame or close the stream.
    let frame = timeout(Duration::from_secs(5), ws.next()).await;

    match frame {
        Ok(None) | Ok(Some(Ok(Message::Close(_)))) => {} // expected
        Ok(Some(Ok(other))) => panic!("unexpected frame after close: {:?}", other),
        Ok(Some(Err(e))) => {
            // tungstenite may surface the peer close as an error — that is acceptable
            let msg = e.to_string();
            assert!(
                msg.contains("Connection reset")
                    || msg.contains("closed")
                    || msg.contains("eof")
                    || msg.contains("ConnectionClosed"),
                "unexpected error after close: {}",
                msg
            );
        }
        Err(_elapsed) => panic!("timed out waiting for close response"),
    }

    // Poll until the server task finishes cleanup (removes the connection from
    // active_connections after the inner websocket_task exits).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if transport.active_connection_count().await == 0 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for connection cleanup"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Verify that a malformed JSON text frame does not crash the server; it must
/// continue processing subsequent frames.
#[tokio::test]
async fn test_wire_invalid_json_does_not_crash() {
    let (addr, _transport) = spawn_ws_test_server().await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    // Send garbage JSON — server should log a warning and stay alive.
    ws.send(Message::Text("{ this is not valid json ::: }".into()))
        .await
        .expect("send invalid JSON");

    // Immediately probe with a protocol-level ping.
    ws.send(Message::Ping(b"probe".to_vec().into()))
        .await
        .expect("send probe ping");

    // Expect a Pong — confirms the server is still alive.
    let frame = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out — server may have crashed on invalid JSON")
        .expect("stream ended unexpectedly")
        .expect("WebSocket error");

    match frame {
        Message::Pong(payload) => {
            assert_eq!(payload.as_ref(), b"probe", "pong payload must echo ping");
        }
        other => panic!("expected Pong, got {:?}", other),
    }
}
