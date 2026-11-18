//! Acceptance tests for #741 item 2 (track F2, after D and F1):
//! `pjs_rate_limited_total{transport="websocket"}` counts messages the
//! WebSocket budget rejects and `pjs_bytes_sent_total{transport="websocket"}`
//! counts the bytes of every text message the server writes.
//!
//! Counters only grow, so assertions are lower bounds on what each test did.

#![cfg_attr(
    all(
        feature = "http-server",
        feature = "metrics",
        feature = "websocket-server",
        feature = "websocket-client"
    ),
    feature(impl_trait_in_assoc_type)
)]
#![cfg(all(
    feature = "http-server",
    feature = "metrics",
    feature = "websocket-server",
    feature = "websocket-client"
))]

mod common;
mod http_metrics_support;

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use http_metrics_support::{scrape_until, spawn_ws_server, value_of};
use pjson_rs::infrastructure::http::axum_adapter::create_pjs_router;
use pjson_rs::infrastructure::websocket::AxumWebSocketTransport;
use pjson_rs::security::RateLimitConfig;
use serde_json::{Value, json};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn counter(body: &str, name: &str) -> f64 {
    value_of(body, name, &[("transport", "websocket")]).unwrap_or(0.0)
}

#[tokio::test]
async fn a_websocket_budget_rejection_increments_the_rate_limited_counter() {
    let metrics = create_pjs_router().with_state(common::create_test_app_state());
    let addr = spawn_ws_server(AxumWebSocketTransport::with_rate_limit_config(
        RateLimitConfig {
            max_messages_per_second: 1,
            burst_allowance: 1,
            max_requests_per_window: 100,
            max_connections_per_ip: 10,
            max_frame_size: 1024 * 1024,
            ..RateLimitConfig::default()
        },
    ))
    .await;
    let (mut ws, _) = connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");

    for _ in 0..3 {
        let _ = ws.send(Message::Text("{}".into())).await;
    }
    let body = scrape_until(&metrics, |body| {
        counter(body, "pjs_rate_limited_total") >= 1.0
    })
    .await;

    assert!(counter(&body, "pjs_rate_limited_total") >= 1.0, "{body}");
}

#[tokio::test]
async fn the_bytes_counter_covers_every_text_message_written_to_a_client() {
    let metrics = create_pjs_router().with_state(common::create_test_app_state());
    let addr = spawn_ws_server(AxumWebSocketTransport::new()).await;
    let (mut ws, _) = connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    let init = json!({"type": "StreamInit", "data": {
        "session_id": "123e4567-e89b-12d3-a456-426614174777",
        "data": {"id": 1, "name": "ada", "tags": ["a", "b", "c"]},
        "options": {"max_frame_size": 65536, "client_fps": null, "priority_mapping": null}
    }});
    ws.send(Message::Text(init.to_string().into()))
        .await
        .expect("send");

    let mut received = 0usize;
    timeout(Duration::from_secs(5), async {
        while let Some(Ok(message)) = ws.next().await {
            if let Message::Text(text) = message {
                received += text.len();
                let value: Value = serde_json::from_str(&text).expect("json");
                if value["type"] == "StreamComplete" {
                    break;
                }
            }
        }
    })
    .await
    .expect("the stream completes");

    let body = scrape_until(&metrics, |body| {
        counter(body, "pjs_bytes_sent_total") >= received as f64
    })
    .await;
    assert!(received > 0);
    assert!(
        counter(&body, "pjs_bytes_sent_total") >= received as f64,
        "counted {} of {received} bytes\n{body}",
        counter(&body, "pjs_bytes_sent_total")
    );
}
