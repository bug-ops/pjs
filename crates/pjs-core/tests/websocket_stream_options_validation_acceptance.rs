//! Acceptance tests for #700: `StreamOptions` is valid by construction and by
//! decode. `max_frame_size < 1024` and `client_fps: 0` fail the whole
//! `WsMessage` decode, so the server answers a fixed 400 `"malformed message"`
//! without a session id, and the client sends options verbatim.
#![cfg(all(feature = "websocket-server", feature = "websocket-client"))]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::{accept_async, connect_async, tungstenite::Message};

use pjson_rs::infrastructure::websocket::{
    AxumWebSocketTransport, Framing, MaxFrameSize, PjsWebSocketClient, StreamOptions, WsMessage,
    server::create_websocket_router,
};

const SESSION: &str = "123e4567-e89b-12d3-a456-426614174000";

fn stream_init_text(options: Value) -> String {
    json!({
        "type": "StreamInit",
        "data": {"session_id": SESSION, "data": {"k": "v"}, "options": options}
    })
    .to_string()
}

fn options_with(field: &str, value: Value) -> Value {
    let mut options =
        json!({"max_frame_size": 65536, "client_fps": null, "priority_mapping": null});
    options[field] = value;
    options
}

#[test]
fn ws_message_decode_rejects_max_frame_size_below_1024() {
    for size in [0, 1, 512, 1023] {
        let text = stream_init_text(options_with("max_frame_size", json!(size)));
        assert!(
            serde_json::from_str::<WsMessage>(&text).is_err(),
            "max_frame_size {size} must fail decode"
        );
    }
    for size in [1024, 65536] {
        let text = stream_init_text(options_with("max_frame_size", json!(size)));
        assert!(
            serde_json::from_str::<WsMessage>(&text).is_ok(),
            "max_frame_size {size} must decode"
        );
    }
}

#[test]
fn ws_message_decode_rejects_zero_client_fps() {
    let zero = stream_init_text(options_with("client_fps", json!(0)));
    let one = stream_init_text(options_with("client_fps", json!(1)));

    assert!(serde_json::from_str::<WsMessage>(&zero).is_err());
    assert!(serde_json::from_str::<WsMessage>(&one).is_ok());
}

#[test]
fn max_frame_size_new_accepts_exactly_the_documented_minimum_and_above() {
    assert!(MaxFrameSize::new(0).is_err());
    assert!(MaxFrameSize::new(1023).is_err());
    assert_eq!(
        MaxFrameSize::new(1024).expect("minimum is valid").get(),
        1024
    );
    assert_eq!(
        MaxFrameSize::try_from(1_000_000usize).expect("valid").get(),
        1_000_000
    );
    assert_eq!(MaxFrameSize::default().get(), 64 * 1024);
}

#[test]
fn stream_options_patch_differs_from_default_only_in_framing() {
    let patch = StreamOptions::patch();

    assert_eq!(patch.framing, Framing::Patch);
    assert_eq!(
        patch,
        StreamOptions {
            framing: Framing::Patch,
            ..StreamOptions::default()
        }
    );
    assert_eq!(StreamOptions::default().framing, Framing::Merge);
}

async fn spawn_server() -> SocketAddr {
    let transport = Arc::new(AxumWebSocketTransport::new());
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

async fn next_text(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    wait: Duration,
) -> Option<Value> {
    loop {
        match timeout(wait, ws.next()).await {
            Err(_) | Ok(None) => return None,
            Ok(Some(Ok(Message::Text(text)))) => {
                return Some(serde_json::from_str(&text).expect("server sends JSON"));
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(error))) => panic!("socket error {error}"),
        }
    }
}

#[tokio::test]
async fn server_answers_invalid_stream_options_with_fixed_malformed_400_and_no_session() {
    let addr = spawn_server().await;
    for options in [
        options_with("max_frame_size", json!(1023)),
        options_with("client_fps", json!(0)),
    ] {
        let (mut ws, _) = connect_async(format!("ws://{addr}/ws"))
            .await
            .expect("handshake");
        ws.send(Message::Text(stream_init_text(options).into()))
            .await
            .expect("send");

        let reply = next_text(&mut ws, Duration::from_secs(5))
            .await
            .expect("an error reply");

        assert_eq!(reply["type"], "Error", "{reply}");
        assert_eq!(reply["data"]["code"], 400, "{reply}");
        assert_eq!(reply["data"]["error"], "malformed message", "{reply}");
        assert!(reply["data"]["session_id"].is_null(), "{reply}");
        assert!(
            next_text(&mut ws, Duration::from_millis(300))
                .await
                .is_none()
        );
    }
}

async fn spawn_spy_server() -> (SocketAddr, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let Ok((tcp, _)) = listener.accept().await else {
            return;
        };
        let Ok(mut ws) = accept_async(tcp).await else {
            return;
        };
        while let Some(Ok(message)) = ws.next().await {
            if let Message::Text(text) = message {
                let _ = tx.send(text.to_string());
            }
        }
    });
    (addr, rx)
}

#[tokio::test]
async fn request_stream_sends_the_given_options_verbatim() {
    let (addr, mut sent) = spawn_spy_server().await;
    let client = Arc::new(PjsWebSocketClient::new(format!("ws://{addr}/ws")).expect("url"));
    let connect = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.connect().await }
    });

    client
        .request_stream(json!({"a": 1}), StreamOptions::patch())
        .await
        .expect("patch request");
    client
        .request_stream(json!({"a": 2}), StreamOptions::default())
        .await
        .expect("default request");

    let mut next_options = async || -> Value {
        let text = timeout(Duration::from_secs(5), sent.recv())
            .await
            .expect("client sends a StreamInit")
            .expect("spy open");
        serde_json::from_str::<Value>(&text).expect("json")["data"]["options"].clone()
    };
    let patch = next_options().await;
    let default = next_options().await;

    assert_eq!(patch["framing"], "patch");
    assert!(default.get("framing").is_none(), "{default}");
    connect.abort();
}
