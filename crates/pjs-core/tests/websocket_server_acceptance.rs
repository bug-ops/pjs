//! Wire-level acceptance tests for the WebSocket server track (#601, #602, #572.2,
//! #571 reuse, #574, #600-lite).
//!
//! Messages are raw JSON so the tests do not depend on the Rust shape of `WsMessage`.
//! Cases that cannot be driven deterministically from the wire (live-session cap,
//! Cancel/Init generation race, StreamCancel timing) are developer-owned `src/` unit tests.
#![cfg(all(feature = "http-server", feature = "websocket-server"))]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use pjson_rs::domain::SessionId;
use pjson_rs::infrastructure::websocket::{
    AxumWebSocketTransport, server::create_websocket_router,
};
use pjson_rs::security::RateLimitConfig;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn relaxed_limits() -> RateLimitConfig {
    RateLimitConfig {
        max_requests_per_window: 100_000,
        max_messages_per_second: 100_000,
        burst_allowance: 100_000,
        max_frame_size: 4 * 1024 * 1024,
        ..RateLimitConfig::default()
    }
}

async fn spawn_server() -> SocketAddr {
    let transport = Arc::new(AxumWebSocketTransport::with_rate_limit_config(
        relaxed_limits(),
    ));
    let app = create_websocket_router().with_state(transport);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    addr
}

async fn connect(addr: SocketAddr) -> Socket {
    connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("handshake")
        .0
}

fn default_options() -> Value {
    json!({"max_frame_size": 65536, "client_fps": null, "priority_mapping": null})
}

fn init_with(session_id: &str, options: Value) -> Message {
    Message::Text(
        json!({"type": "StreamInit", "data": {
            "session_id": session_id, "data": {"k": "v"}, "options": options
        }})
        .to_string()
        .into(),
    )
}

fn init(session_id: &str) -> Message {
    init_with(session_id, default_options())
}

async fn next_json(ws: &mut Socket) -> Value {
    loop {
        let message = timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timed out waiting for a message")
            .expect("stream ended")
            .expect("ws error");
        if let Message::Text(text) = message {
            return serde_json::from_str(&text).expect("server sent json");
        }
    }
}

fn kind(message: &Value) -> &str {
    message["type"].as_str().expect("type tag")
}

#[test]
fn test_transport_constructors_do_not_panic_outside_runtime() {
    let _ = AxumWebSocketTransport::new();
    let _ = AxumWebSocketTransport::default();
    let _ = AxumWebSocketTransport::with_rate_limit_config(RateLimitConfig::default());
}

#[tokio::test]
async fn test_ping_is_answered_with_pong_carrying_same_timestamp() {
    let mut ws = connect(spawn_server().await).await;
    ws.send(Message::Text(
        json!({"type": "Ping", "data": {"timestamp": 123456u64}})
            .to_string()
            .into(),
    ))
    .await
    .expect("send ping");

    let reply = next_json(&mut ws).await;
    assert_eq!(kind(&reply), "Pong", "{reply}");
    assert_eq!(reply["data"]["timestamp"], json!(123456u64));
}

#[tokio::test]
async fn test_stream_init_priority_mapping_value_zero_rejected() {
    let mut ws = connect(spawn_server().await).await;
    let mut options = default_options();
    options["priority_mapping"] = json!({"field": 0});
    ws.send(init_with(&SessionId::new().to_string(), options))
        .await
        .expect("send init");

    let reply = next_json(&mut ws).await;
    assert_eq!(kind(&reply), "Error", "{reply}");
}

#[tokio::test]
async fn test_stream_init_default_options_yields_complete_frame_then_stream_complete() {
    let mut ws = connect(spawn_server().await).await;
    let session = SessionId::new().to_string();
    ws.send(init(&session)).await.expect("send init");

    let frame = next_json(&mut ws).await;
    assert_eq!(kind(&frame), "StreamFrame", "{frame}");
    assert_eq!(frame["data"]["is_complete"], json!(true));
    assert_eq!(frame["data"]["session_id"], json!(session));

    let complete = next_json(&mut ws).await;
    assert_eq!(kind(&complete), "StreamComplete", "{complete}");
    assert_eq!(complete["data"]["session_id"], json!(session));
}

#[tokio::test]
async fn test_stream_complete_carries_no_checksum() {
    let mut ws = connect(spawn_server().await).await;
    ws.send(init(&SessionId::new().to_string()))
        .await
        .expect("send init");
    let _frame = next_json(&mut ws).await;
    let complete = next_json(&mut ws).await;
    assert_eq!(kind(&complete), "StreamComplete");
    assert!(complete["data"].get("checksum").is_none(), "{complete}");
}

#[tokio::test]
async fn test_stream_init_invalid_options_rejected_with_400_and_no_frames() {
    let mut ws = connect(spawn_server().await).await;
    let session = SessionId::new().to_string();
    let mut options = default_options();
    options["max_frame_size"] = json!(1023);
    ws.send(init_with(&session, options))
        .await
        .expect("send init");

    let reply = next_json(&mut ws).await;
    assert_eq!(kind(&reply), "Error", "{reply}");
    assert_eq!(reply["data"]["code"], json!(400));
    assert_eq!(reply["data"]["error"], json!("invalid stream options"));

    let followup = timeout(Duration::from_millis(300), ws.next()).await;
    assert!(
        !matches!(followup, Ok(Some(Ok(Message::Text(_))))),
        "no frames may follow a rejected init"
    );
}

#[tokio::test]
async fn test_same_session_id_on_two_connections_streams_independently() {
    let addr = spawn_server().await;
    let session = SessionId::new().to_string();
    let mut first = connect(addr).await;
    let mut second = connect(addr).await;

    first.send(init(&session)).await.expect("init first");
    second.send(init(&session)).await.expect("init second");

    for ws in [&mut first, &mut second] {
        let frame = next_json(ws).await;
        assert_eq!(kind(&frame), "StreamFrame", "{frame}");
        let complete = next_json(ws).await;
        assert_eq!(kind(&complete), "StreamComplete", "{complete}");
    }
}

#[tokio::test]
async fn test_session_id_reusable_immediately_after_stream_complete() {
    let mut ws = connect(spawn_server().await).await;
    let session = SessionId::new().to_string();

    ws.send(init(&session)).await.expect("first init");
    assert_eq!(kind(&next_json(&mut ws).await), "StreamFrame");
    assert_eq!(kind(&next_json(&mut ws).await), "StreamComplete");

    ws.send(init(&session)).await.expect("second init");
    let reply = next_json(&mut ws).await;
    assert_eq!(kind(&reply), "StreamFrame", "no 409 expected: {reply}");
}

#[tokio::test]
async fn test_many_sequential_streams_on_one_connection_all_succeed() {
    let mut ws = connect(spawn_server().await).await;
    for _ in 0..65 {
        ws.send(init(&SessionId::new().to_string()))
            .await
            .expect("send init");
        let frame = next_json(&mut ws).await;
        assert_eq!(kind(&frame), "StreamFrame", "{frame}");
        assert_eq!(kind(&next_json(&mut ws).await), "StreamComplete");
    }
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture_logs(max_level: tracing::Level) -> (LogBuffer, tracing::subscriber::DefaultGuard) {
    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(max_level)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    (buffer, tracing::subscriber::set_default(subscriber))
}

fn longest_line(buffer: &LogBuffer) -> usize {
    let bytes = buffer.0.lock().expect("log lock").clone();
    String::from_utf8_lossy(&bytes)
        .lines()
        .map(str::len)
        .max()
        .unwrap_or(0)
}

#[tokio::test]
async fn test_parse_failure_warn_line_is_bounded_for_oversized_unknown_tag() {
    let (logs, _guard) = capture_logs(tracing::Level::WARN);
    let mut ws = connect(spawn_server().await).await;
    let tag = "x".repeat(1024 * 1024);
    ws.send(Message::Text(
        json!({"type": tag, "data": {}}).to_string().into(),
    ))
    .await
    .expect("send");

    let reply = next_json(&mut ws).await;
    assert_eq!(kind(&reply), "Error", "{reply}");
    assert!(longest_line(&logs) > 0, "the parse failure must be logged");
    assert!(
        longest_line(&logs) < 256,
        "log line of {} bytes echoes untrusted input",
        longest_line(&logs)
    );
}

#[tokio::test]
async fn test_inbound_debug_log_does_not_echo_payload() {
    let (logs, _guard) = capture_logs(tracing::Level::DEBUG);
    let mut ws = connect(spawn_server().await).await;
    let big = "y".repeat(100 * 1024);
    ws.send(Message::Text(
        json!({"type": "StreamInit", "data": {
            "session_id": SessionId::new().to_string(),
            "data": {"blob": big},
            "options": default_options()
        }})
        .to_string()
        .into(),
    ))
    .await
    .expect("send");
    let _ = next_json(&mut ws).await;

    assert!(
        longest_line(&logs) < 4096,
        "a log line of {} bytes carries the payload",
        longest_line(&logs)
    );
}
