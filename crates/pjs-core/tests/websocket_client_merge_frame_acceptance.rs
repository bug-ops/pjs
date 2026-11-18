//! Validation tests for #701: merge-framed `StreamFrame` payloads are applied by
//! value; the byte cap, key overwrite accounting and non-object replacement keep
//! their behaviour through a scripted server.
#![cfg(feature = "websocket-client")]

use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use pjson_rs::infrastructure::websocket::{
    PjsWebSocketClient, StreamOptions, WebSocketClientConfig,
};

async fn spawn_server(payloads: Vec<Value>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut ws = accept_async(tcp).await.expect("handshake");
        let Some(Ok(Message::Text(init))) = ws.next().await else {
            return;
        };
        let init: Value = serde_json::from_str(&init).expect("StreamInit");
        let id = init["data"]["session_id"].clone();
        for (frame_id, payload) in payloads.into_iter().enumerate() {
            let frame = json!({"type": "StreamFrame", "data": {
                "session_id": id, "frame_id": frame_id, "priority": 80,
                "payload": payload, "is_complete": false
            }});
            if ws
                .send(Message::Text(frame.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
        }
        while ws.next().await.is_some() {}
    });
    addr
}

/// Polls until the session's data satisfies `done`; `None` data means dropped.
async fn eventually(
    payloads: Vec<Value>,
    max_session_bytes: usize,
    done: impl Fn(Option<Value>) -> bool,
) -> bool {
    let addr = spawn_server(payloads).await;
    let mut config = WebSocketClientConfig::default();
    config.max_session_bytes = NonZeroUsize::new(max_session_bytes).expect("non-zero");
    let client = Arc::new(
        PjsWebSocketClient::new(format!("ws://{addr}"))
            .expect("url")
            .with_config(config),
    );
    let connect = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.connect().await }
    });
    let id = client
        .request_stream(json!({}), StreamOptions::default())
        .await
        .expect("request");
    let reached = timeout(Duration::from_secs(3), async {
        while !done(client.get_current_data(id).await.expect("lookup")) {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    connect.abort();
    reached
}

#[tokio::test]
async fn merge_frame_above_max_session_bytes_drops_the_session() {
    let payloads = vec![
        json!({"k": "x".repeat(100)}),
        json!({"big": "y".repeat(3000)}),
    ];

    assert!(eventually(payloads, 2000, |data| data.is_none()).await);
}

#[tokio::test]
async fn merge_frame_overwriting_a_key_is_accounted_by_size_delta() {
    let payloads: Vec<Value> = (0..20)
        .map(|i| json!({"k": "x".repeat(1000 + i)}))
        .collect();

    assert!(
        eventually(payloads, 2000, |data| {
            data.is_some_and(|d| d["k"].as_str().is_some_and(|s| s.len() == 1019))
        })
        .await
    );
}

#[tokio::test]
async fn merge_frame_merges_objects_and_non_object_payload_replaces_the_data() {
    let merged = vec![json!({"a": 1, "b": 1}), json!({"b": 2, "c": 3})];
    assert!(eventually(merged, 4096, |d| d == Some(json!({"a": 1, "b": 2, "c": 3}))).await);

    let replaced = vec![json!({"a": 1}), json!([1, 2, 3])];
    assert!(eventually(replaced, 4096, |d| d == Some(json!([1, 2, 3]))).await);
}
