//! Acceptance tests for #609 (per-stream config is applied, validated and defaulted) and
//! #610 (`/metrics` totals are lifetime counters) on `websocket-streaming-server`.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use futures::StreamExt;
use pjs_demo::servers::websocket_streaming::{AppState, app};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;

async fn spawn_server() -> SocketAddr {
    let router = app(AppState::new());
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    addr
}

async fn http(addr: SocketAddr, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let payload = body.map(Value::to_string).unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",
        payload.len()
    );
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    timeout(Duration::from_secs(5), stream.read_to_end(&mut raw))
        .await
        .expect("response in time")
        .expect("read");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text[9..12].parse().expect("status code");
    let body = text.split("\r\n\r\n").nth(1).unwrap_or_default();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

async fn create_stream(addr: SocketAddr, config: Value) -> (u16, Value) {
    http(
        addr,
        "POST",
        "/stream",
        Some(&json!({"data_type": "x", "config": config})),
    )
    .await
}

/// Drains a stream and returns the arrival instants of every `pjs_frame` message.
async fn collect_frames(addr: SocketAddr, session_id: &str) -> Vec<Instant> {
    let (mut ws, _) = connect_async(format!("ws://{addr}/ws/{session_id}"))
        .await
        .expect("upgrade");
    let mut frames = Vec::new();
    timeout(Duration::from_secs(20), async {
        while let Some(Ok(message)) = ws.next().await {
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(&text).expect("json message");
            match value["@type"].as_str() {
                Some("pjs_frame") => frames.push(Instant::now()),
                Some("stream_complete") => break,
                _ => {}
            }
        }
    })
    .await
    .expect("stream completes");
    frames
}

#[tokio::test]
async fn test_stream_config_max_frames_and_delay_applied() {
    let addr = spawn_server().await;
    let (status, created) =
        create_stream(addr, json!({"max_frames": 2, "frame_delay_ms": 500})).await;
    assert_eq!(status, 200, "{created}");

    let frames = collect_frames(addr, created["session_id"].as_str().expect("id")).await;
    assert_eq!(frames.len(), 2);
    assert!(frames[1] - frames[0] >= Duration::from_millis(400));
}

#[tokio::test]
async fn test_stream_config_omitted_fields_echo_defaults_not_null() {
    let addr = spawn_server().await;
    let (status, created) = create_stream(addr, json!({"max_frames": 2})).await;
    assert_eq!(status, 200, "{created}");

    for (field, value) in created["config"].as_object().expect("config object") {
        assert!(!value.is_null(), "{field} must echo a default, not null");
    }
}

#[tokio::test]
async fn test_stream_config_frame_delay_over_limit_rejected() {
    let addr = spawn_server().await;
    let (status, _) = create_stream(addr, json!({"frame_delay_ms": 5001})).await;
    assert!((400..500).contains(&status), "status {status}");
}

#[tokio::test]
async fn test_stream_config_zero_max_frames_rejected() {
    let addr = spawn_server().await;
    let (status, _) = create_stream(addr, json!({"max_frames": 0})).await;
    assert!((400..500).contains(&status), "status {status}");
}

#[tokio::test]
async fn test_stream_config_unknown_field_rejected() {
    let addr = spawn_server().await;
    let (status, _) = create_stream(addr, json!({"not_a_field": 1})).await;
    assert!((400..500).contains(&status), "status {status}");
}

#[tokio::test]
async fn test_metrics_totals_survive_session_end() {
    let addr = spawn_server().await;
    let (_, created) = create_stream(addr, json!({})).await;
    let frames = collect_frames(addr, created["session_id"].as_str().expect("id")).await;
    assert!(!frames.is_empty());
    tokio::time::sleep(Duration::from_millis(300)).await;

    let (_, metrics) = http(addr, "GET", "/metrics", None).await;
    assert_eq!(metrics["active_sessions"], json!(0));
    assert!(metrics["total_frames_sent"].as_u64().expect("frames") >= frames.len() as u64);
    assert!(metrics["total_bytes_sent"].as_u64().expect("bytes") > 0);
}
