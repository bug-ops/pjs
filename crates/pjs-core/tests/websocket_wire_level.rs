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
use pjson_rs::security::RateLimitConfig;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Spawn the WebSocket router on a random localhost port.
///
/// The TCP listener is bound before `tokio::spawn` returns, so callers may
/// immediately issue `connect_async` — the kernel queues the SYN until the
/// accept loop runs.
async fn spawn_ws_test_server() -> (SocketAddr, Arc<AxumWebSocketTransport>) {
    spawn_ws_test_server_with(AxumWebSocketTransport::new()).await
}

/// Spawn a WebSocket router with a caller-provided transport.
async fn spawn_ws_test_server_with(
    transport: AxumWebSocketTransport,
) -> (SocketAddr, Arc<AxumWebSocketTransport>) {
    let transport = Arc::new(transport);
    let app = create_websocket_router().with_state(transport.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind random port");
    let addr = listener.local_addr().expect("local_addr");

    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
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

/// Verify that inbound application frames are subject to per-connection rate
/// limiting once the token bucket is exhausted.
///
/// The server is configured with a tiny token budget (1 message + 0 burst)
/// so the second text frame must trigger a policy-violation close.
#[tokio::test]
async fn test_wire_inbound_messages_rate_limited() {
    let config = RateLimitConfig {
        max_requests_per_window: 100,
        max_connections_per_ip: 10,
        max_frame_size: 1024 * 1024,
        max_messages_per_second: 1,
        burst_allowance: 1,
        ..Default::default()
    };
    let (addr, _transport) =
        spawn_ws_test_server_with(AxumWebSocketTransport::with_rate_limit_config(config)).await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    // First inbound frame consumes the only token. Use a benign payload that
    // will not parse as a WsMessage so no streaming side-effects occur.
    ws.send(Message::Text("{}".into()))
        .await
        .expect("send first frame");

    // Second inbound frame must be rejected. The server closes the socket with
    // code 1008 (Policy Violation).
    ws.send(Message::Text("{}".into()))
        .await
        .expect("send second frame");

    let close_seen = timeout(Duration::from_secs(5), async {
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(Message::Close(Some(frame))) => return Some(frame),
                Ok(Message::Close(None)) => return None,
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
        None
    })
    .await
    .expect("timed out waiting for rate-limit close");

    if let Some(frame) = close_seen {
        assert_eq!(
            u16::from(frame.code),
            1008,
            "expected policy-violation close code"
        );
    }
}

/// Verify that `upgrade_handler` configures axum/tungstenite's
/// transport-level `max_message_size`/`max_frame_size` from the
/// transport's `RateLimitConfig`, rather than relying solely on the
/// application-level `check_message` call (which already existed prior to
/// this fix and independently rejects oversized frames with a `1008`
/// policy-violation close).
///
/// This is only observable at the wire level via the close *code*: the
/// app-level path always closes with `1008`. If the transport-level cap is
/// not wired up, tungstenite still forwards the oversized frame to
/// `check_message`, which then closes with `1008` — so a `1008` close here
/// would indicate the fix regressed, not that it works.
#[tokio::test]
async fn test_wire_upgrade_enforces_transport_level_max_frame_size() {
    let config = RateLimitConfig {
        max_frame_size: 64,
        ..Default::default()
    };
    let (addr, _transport) =
        spawn_ws_test_server_with(AxumWebSocketTransport::with_rate_limit_config(config)).await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    // Comfortably over the 64-byte cap, but small enough that this would
    // NOT have been rejected by axum/tungstenite's own pre-fix defaults
    // (64 MiB message / 16 MiB frame).
    let oversized = "x".repeat(1024);
    ws.send(Message::Text(oversized.into()))
        .await
        .expect("send oversized frame");

    let outcome = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out waiting for the connection to close");

    match outcome {
        None => {}         // stream ended: connection closed
        Some(Err(_)) => {} // protocol error surfaced as a read error
        Some(Ok(Message::Close(Some(frame)))) => {
            assert_ne!(
                u16::from(frame.code),
                1008,
                "close code 1008 indicates the application-level check_message \
                 rejected this after axum/tungstenite already buffered the full \
                 frame — the transport-level max_frame_size cap did not fire"
            );
        }
        Some(Ok(Message::Close(None))) => {}
        Some(Ok(other)) => panic!(
            "expected the connection to be closed for exceeding max_frame_size, got {:?}",
            other
        ),
    }
}

/// Verify the transport-level cap is keyed on `max_frame_size` itself, not
/// some off-by-one approximation of it: a frame of exactly `max_frame_size`
/// bytes must be accepted (invalid JSON, but that's an application-level
/// concern — see `test_wire_invalid_json_does_not_crash`), while a frame one
/// byte larger must be rejected at the transport level.
#[tokio::test]
async fn test_wire_max_frame_size_boundary_is_exact() {
    const MAX_FRAME_SIZE: usize = 1024;
    let config = RateLimitConfig {
        max_frame_size: MAX_FRAME_SIZE,
        ..Default::default()
    };
    let (addr, _transport) =
        spawn_ws_test_server_with(AxumWebSocketTransport::with_rate_limit_config(config)).await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    ws.send(Message::Text("x".repeat(MAX_FRAME_SIZE).into()))
        .await
        .expect("send frame at exactly max_frame_size");

    // Probe with a protocol-level ping — a Pong confirms the connection
    // survived the exactly-at-limit frame instead of being torn down.
    ws.send(Message::Ping(b"probe".to_vec().into()))
        .await
        .expect("send probe ping");

    let frame = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out — a frame at exactly max_frame_size must not close the connection")
        .expect("stream ended unexpectedly")
        .expect("WebSocket error");
    match frame {
        Message::Pong(payload) => assert_eq!(payload.as_ref(), b"probe"),
        other => panic!("expected Pong, got {:?}", other),
    }
}

/// Companion to the boundary-success test above: a frame one byte over
/// `max_frame_size` must be rejected at the transport level (not the
/// application-level `check_message` 1008 close — see the discrimination
/// rationale on `test_wire_upgrade_enforces_transport_level_max_frame_size`).
#[tokio::test]
async fn test_wire_max_frame_size_boundary_plus_one_byte_rejected_at_transport_level() {
    const MAX_FRAME_SIZE: usize = 1024;
    let config = RateLimitConfig {
        max_frame_size: MAX_FRAME_SIZE,
        ..Default::default()
    };
    let (addr, _transport) =
        spawn_ws_test_server_with(AxumWebSocketTransport::with_rate_limit_config(config)).await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    ws.send(Message::Text("x".repeat(MAX_FRAME_SIZE + 1).into()))
        .await
        .expect("send frame one byte over max_frame_size");

    let outcome = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out waiting for the connection to close");

    match outcome {
        None => {}
        Some(Err(_)) => {}
        Some(Ok(Message::Close(Some(frame)))) => {
            assert_ne!(
                u16::from(frame.code),
                1008,
                "a frame one byte over max_frame_size must be rejected at the \
                 transport level, not fall through to the application-level check"
            );
        }
        Some(Ok(Message::Close(None))) => {}
        Some(Ok(other)) => panic!(
            "expected the connection to be closed for exceeding max_frame_size, got {:?}",
            other
        ),
    }
}

/// Verify `RateLimitConfig::low_resource()`'s smaller 256 KiB frame cap is
/// actually what gets wired into the transport by `upgrade_handler`, not
/// just the 1 MiB default preset exercised by the other tests in this file.
#[tokio::test]
async fn test_wire_low_resource_preset_enforces_smaller_frame_cap() {
    let config = RateLimitConfig::low_resource();
    assert_eq!(
        config.max_frame_size,
        256 * 1024,
        "test assumes low_resource()'s documented 256 KiB frame cap"
    );
    let (addr, _transport) =
        spawn_ws_test_server_with(AxumWebSocketTransport::with_rate_limit_config(config)).await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    // Well under the 1 MiB *default* cap, but over low_resource()'s 256 KiB
    // cap — proves the smaller preset value is what's actually enforced.
    let oversized = "x".repeat(512 * 1024);
    let send_result = ws.send(Message::Text(oversized.into())).await;

    // A large-enough oversized frame can trigger the server's rejection
    // (and connection teardown) before the client finishes writing it,
    // surfacing as a write error here rather than a later read — that is
    // just as valid a signal of transport-level rejection as a close frame.
    if send_result.is_err() {
        return;
    }

    let outcome = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("timed out waiting for the connection to close");

    match outcome {
        None => {}
        Some(Err(_)) => {}
        Some(Ok(Message::Close(Some(frame)))) => {
            assert_ne!(
                u16::from(frame.code),
                1008,
                "expected transport-level rejection under the low_resource preset"
            );
        }
        Some(Ok(Message::Close(None))) => {}
        Some(Ok(other)) => panic!(
            "expected the connection to be closed for exceeding the low_resource \
             frame cap, got {:?}",
            other
        ),
    }
}

/// Verify that a real stalled outbound write (not a mocked one) times out
/// and the connection is torn down, instead of wedging the connection's
/// task — and the `Arc<RateLimitGuard>` it holds — forever.
///
/// Uses `RateLimitConfig::write_timeout` (the testability seam added
/// alongside this fix) to keep the test fast: a short deadline lets it
/// observe a real stall without waiting out the 10s production default.
/// The client requests one large stream and then never reads again, so
/// the server's single ~32 MiB outbound frame cannot be delivered —
/// cumulative undelivered bytes exceed any realistic OS socket-buffer
/// size, so the write genuinely blocks rather than merely simulating a
/// stall the way the unit tests in `websocket::mod`'s `write_timeout_tests`
/// do against a mock sink.
#[tokio::test]
async fn test_wire_stalled_write_times_out_and_closes_connection() {
    let config = RateLimitConfig {
        max_frame_size: 64 * 1024 * 1024,
        write_timeout: Duration::from_millis(150),
        ..Default::default()
    };
    let (addr, transport) =
        spawn_ws_test_server_with(AxumWebSocketTransport::with_rate_limit_config(config)).await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    let big_data = "x".repeat(32 * 1024 * 1024);
    let init = json!({
        "type": "StreamInit",
        "data": {
            "session_id": "ignored",
            "data": big_data,
            "options": {
                "max_frame_size": 64 * 1024 * 1024,
                "client_fps": null,
                "compression": false,
                "priority_mapping": null
            }
        }
    });

    ws.send(Message::Text(init.to_string().into()))
        .await
        .expect("send StreamInit");

    // Never read from `ws` again, and keep it alive (not dropped): a
    // dropped/closed socket would make the server's next write fail fast
    // with a connection error, which isn't the scenario under test. Held
    // open but undrained, the server's ~32 MiB outbound frame cannot be
    // delivered, so the write genuinely blocks rather than completing
    // instantly into a generously-sized OS buffer.
    let _ws = ws;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if transport.active_connection_count().await == 0 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "connection must be closed once its stalled write exceeds write_timeout, \
             not left wedged forever"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Negative control for [`test_wire_stalled_write_times_out_and_closes_connection`].
///
/// Reuses that test's exact stall setup (large frame, non-reading client)
/// but with a `write_timeout` far longer than the 150ms one the sibling
/// test uses. Asserts both halves of the causal claim: the connection
/// stays open through an 800ms observation window (proving a closure
/// for some unrelated reason, e.g. rejecting the large inbound frame,
/// isn't what's happening), and it does eventually close, but not
/// before this test's own `write_timeout` elapses (proving the stalled
/// write was actually reached rather than the observation window simply
/// expiring before the write phase began, and that closure is genuinely
/// gated on `write_timeout`).
#[tokio::test]
async fn test_wire_stalled_write_stays_open_before_write_timeout() {
    let write_timeout = Duration::from_secs(5);
    let config = RateLimitConfig {
        max_frame_size: 64 * 1024 * 1024,
        write_timeout,
        ..Default::default()
    };
    let (addr, transport) =
        spawn_ws_test_server_with(AxumWebSocketTransport::with_rate_limit_config(config)).await;

    let (mut ws, _) = connect_async(ws_url(addr))
        .await
        .expect("WebSocket handshake failed");

    let big_data = "x".repeat(32 * 1024 * 1024);
    let init = json!({
        "type": "StreamInit",
        "data": {
            "session_id": "ignored",
            "data": big_data,
            "options": {
                "max_frame_size": 64 * 1024 * 1024,
                "client_fps": null,
                "compression": false,
                "priority_mapping": null
            }
        }
    });

    let write_start = tokio::time::Instant::now();
    ws.send(Message::Text(init.to_string().into()))
        .await
        .expect("send StreamInit");

    // Never read from `ws` again, and keep it alive — same rationale as the
    // sibling test: an undrained connection keeps the server's outbound
    // write genuinely stalled.
    let _ws = ws;

    // Wait for the server to register the connection before asserting it
    // stays registered.
    let established_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while transport.active_connection_count().await == 0 {
        assert!(
            tokio::time::Instant::now() < established_deadline,
            "timed out waiting for the server to register the connection"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Poll for a window well short of the 5s write_timeout above (and well
    // above the sibling test's 150ms write_timeout), asserting the
    // connection never closes during it.
    let patience = Duration::from_millis(800);
    let observation_deadline = tokio::time::Instant::now() + patience;
    while tokio::time::Instant::now() < observation_deadline {
        assert_eq!(
            transport.active_connection_count().await,
            1,
            "connection must stay open while the stalled write is still within write_timeout"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Second half of the causal claim: the connection does eventually
    // close (proving the stalled-write phase was actually reached, not
    // skipped past by a too-short observation window above), and it does
    // not close before `write_timeout` elapses (proving that closure,
    // once it happens, is caused by the write timeout and not some
    // unrelated path). A 15s deadline is generous relative to the 5s
    // write_timeout so a hang here fails loudly instead of silently.
    let close_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while transport.active_connection_count().await != 0 {
        assert!(
            tokio::time::Instant::now() < close_deadline,
            "connection with a {:?} write_timeout must eventually close once truly stalled",
            write_timeout
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let elapsed = write_start.elapsed();
    assert!(
        elapsed >= write_timeout,
        "connection closed after {:?}, before its {:?} write_timeout could have elapsed — \
         closure must not happen for a reason unrelated to the write timeout",
        elapsed,
        write_timeout
    );
}
