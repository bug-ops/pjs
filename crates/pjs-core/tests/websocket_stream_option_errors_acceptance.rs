//! Acceptance tests for #768 and #737 (track D): an invalid `StreamOptions`
//! field is answered with a fixed 400 text that names the field and the
//! violated constraint, never echoing the payload; a document too deep for the
//! decoder is answered `400 "malformed message"` (spec 066).
#![cfg(all(feature = "websocket-server", feature = "websocket-client"))]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use pjson_rs::infrastructure::websocket::{
    AxumWebSocketTransport, server::create_websocket_router,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const SESSION: &str = "123e4567-e89b-12d3-a456-426614174000";

async fn spawn_server() -> SocketAddr {
    let app = create_websocket_router().with_state(Arc::new(AxumWebSocketTransport::new()));
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

fn options_with(overrides: &[(&str, Value)]) -> Value {
    let mut options =
        json!({"max_frame_size": 65536, "client_fps": null, "priority_mapping": null});
    for (field, value) in overrides {
        options[*field] = value.clone();
    }
    options
}

fn stream_init(data_json: &str, options: &Value) -> Message {
    Message::Text(
        format!(
            r#"{{"type":"StreamInit","data":{{"session_id":"{SESSION}","data":{data_json},"options":{options}}}}}"#
        )
        .into(),
    )
}

/// Sends a `StreamInit` and returns the `(code, text, session_id)` of the next `Error`.
async fn error_reply(ws: &mut Socket, init: Message) -> (u64, String, Value) {
    ws.send(init).await.expect("send");
    timeout(Duration::from_secs(5), async {
        loop {
            let Message::Text(text) = ws.next().await.expect("open").expect("ok") else {
                continue;
            };
            let value: Value = serde_json::from_str(&text).expect("json");
            if value["type"] == "Error" {
                return (
                    value["data"]["code"].as_u64().expect("code"),
                    value["data"]["error"].as_str().expect("text").to_owned(),
                    value["data"]["session_id"].clone(),
                );
            }
        }
    })
    .await
    .expect("an Error reply")
}

async fn assert_connection_alive(ws: &mut Socket) {
    ws.send(Message::Ping(b"alive".to_vec().into()))
        .await
        .expect("send ping");
    timeout(Duration::from_secs(5), async {
        loop {
            if let Message::Pong(payload) = ws.next().await.expect("open").expect("ok") {
                assert_eq!(payload.as_ref(), b"alive");
                return;
            }
        }
    })
    .await
    .expect("the connection stays open after the rejected option");
}

fn invalid_option_rows() -> Vec<(&'static str, Value, &'static str)> {
    vec![
        (
            "max_frame_size",
            json!(1023),
            "invalid stream option: max_frame_size must be at least 1024",
        ),
        (
            "client_fps",
            json!(0),
            "invalid stream option: client_fps must be a positive integer",
        ),
        (
            "priority_mapping",
            json!({"field": 0}),
            "invalid stream option: priority_mapping values must be valid priorities",
        ),
        (
            "framing",
            json!("sideways"),
            r#"invalid stream option: framing must be "merge" or "patch""#,
        ),
    ]
}

#[tokio::test]
async fn each_invalid_option_is_answered_with_its_own_fixed_400_text() {
    let addr = spawn_server().await;
    for (field, value, expected) in invalid_option_rows() {
        let mut ws = connect(addr).await;

        let (code, text, session) = error_reply(
            &mut ws,
            stream_init(r#"{"k":"v"}"#, &options_with(&[(field, value)])),
        )
        .await;

        assert_eq!(code, 400, "{field}");
        assert_eq!(text, expected, "{field}");
        assert!(session.is_null(), "{field}: {session}");
    }
}

#[tokio::test]
async fn a_rejected_option_keeps_the_connection_open() {
    let addr = spawn_server().await;
    let mut ws = connect(addr).await;

    let _ = error_reply(
        &mut ws,
        stream_init(r#"{"k":"v"}"#, &options_with(&[("client_fps", json!(0))])),
    )
    .await;

    assert_connection_alive(&mut ws).await;
}

#[tokio::test]
async fn the_reply_never_echoes_the_rejected_value() {
    let addr = spawn_server().await;
    let mut ws = connect(addr).await;

    let (_, text, _) = error_reply(
        &mut ws,
        stream_init(
            r#"{"k":"v"}"#,
            &options_with(&[("framing", json!("sneaky-<script>"))]),
        ),
    )
    .await;

    assert!(!text.contains("sneaky"), "{text}");
}

#[tokio::test]
async fn with_several_invalid_options_the_first_field_in_declaration_order_is_reported() {
    let addr = spawn_server().await;
    let mut ws = connect(addr).await;

    let (_, text, _) = error_reply(
        &mut ws,
        stream_init(
            r#"{"k":"v"}"#,
            &options_with(&[
                ("framing", json!("x")),
                ("client_fps", json!(0)),
                ("max_frame_size", json!(1)),
            ]),
        ),
    )
    .await;

    assert_eq!(
        text,
        "invalid stream option: max_frame_size must be at least 1024"
    );
}

#[tokio::test]
async fn an_undecodable_message_that_is_not_an_option_error_stays_malformed() {
    let addr = spawn_server().await;
    let mut ws = connect(addr).await;

    let (code, text, session) = error_reply(
        &mut ws,
        Message::Text(
            r#"{"type":"StreamInit","data":{"session_id":"not-a-uuid","data":{},"options":{}}}"#
                .into(),
        ),
    )
    .await;

    assert_eq!((code, text.as_str()), (400, "malformed message"));
    assert!(session.is_null());
}

fn deep_document(levels: usize) -> String {
    format!("{}1{}", r#"{"a":"#.repeat(levels), "}".repeat(levels))
}

#[tokio::test]
async fn a_document_nested_beyond_the_decoder_limit_is_answered_malformed_not_413() {
    let addr = spawn_server().await;
    let mut ws = connect(addr).await;

    let (code, text, session) = error_reply(
        &mut ws,
        stream_init(&deep_document(130), &options_with(&[])),
    )
    .await;

    assert_eq!((code, text.as_str()), (400, "malformed message"));
    assert!(session.is_null());
}

#[tokio::test]
async fn a_too_deep_document_wins_over_an_invalid_option() {
    let addr = spawn_server().await;
    let mut ws = connect(addr).await;

    let (code, text, _) = error_reply(
        &mut ws,
        stream_init(
            &deep_document(130),
            &options_with(&[("max_frame_size", json!(1))]),
        ),
    )
    .await;

    assert_eq!((code, text.as_str()), (400, "malformed message"));
}
