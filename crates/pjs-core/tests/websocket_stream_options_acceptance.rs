//! Acceptance tests for #655: `StreamOptions` no longer carries `compression`.
//! A legacy `StreamInit` that still sends the key decodes and the key is
//! ignored; the key is never serialized.

#![cfg(any(feature = "websocket-server", feature = "websocket-client"))]

use pjson_rs::domain::SessionId;
use pjson_rs::infrastructure::websocket::{StreamOptions, WsMessage};
use serde_json::{Value, json};

const SESSION_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

fn stream_init(options: Value) -> String {
    json!({
        "type": "StreamInit",
        "data": {"session_id": SESSION_ID, "data": {"k": "v"}, "options": options}
    })
    .to_string()
}

fn decode_options(options: Value) -> StreamOptions {
    match serde_json::from_str(&stream_init(options)).expect("StreamInit decodes") {
        WsMessage::StreamInit { options, .. } => options,
        other => panic!("expected StreamInit, got {other:?}"),
    }
}

#[test]
fn test_stream_options_serialize_has_no_compression_key() {
    let value = serde_json::to_value(StreamOptions::default()).expect("serializable");

    assert!(value.get("compression").is_none(), "{value}");
    assert!(value.get("max_frame_size").is_some());
}

#[test]
fn test_stream_init_serialize_has_no_compression_key_anywhere() {
    let message = WsMessage::StreamInit {
        session_id: SessionId::new(),
        data: json!({"k": "v"}),
        options: StreamOptions::default(),
    };

    let text = serde_json::to_string(&message).expect("serializable");

    assert!(!text.contains("compression"), "{text}");
}

#[test]
fn test_stream_init_decode_accepts_options_without_compression() {
    let options = decode_options(json!({"max_frame_size": 4096}));

    assert_eq!(options.max_frame_size, 4096);
    assert_eq!(options.client_fps, None);
}

#[test]
fn test_stream_init_decode_ignores_a_legacy_compression_key() {
    let without = decode_options(json!({"max_frame_size": 4096, "client_fps": 30}));

    for legacy in [true, false] {
        let with = decode_options(json!({
            "max_frame_size": 4096, "client_fps": 30, "compression": legacy
        }));

        assert_eq!(with, without, "compression: {legacy}");
    }
}

#[test]
fn test_stream_init_decode_ignores_other_unknown_option_keys() {
    let options = decode_options(json!({"max_frame_size": 2048, "future_option": [1, 2]}));

    assert_eq!(options.max_frame_size, 2048);
}

#[test]
fn test_stream_init_decode_requires_max_frame_size() {
    let text = stream_init(json!({"client_fps": null, "compression": true}));

    assert!(serde_json::from_str::<WsMessage>(&text).is_err());
}

#[test]
fn test_stream_options_round_trip_through_serde_is_lossless() {
    let options = StreamOptions {
        max_frame_size: 8192,
        client_fps: Some(30),
        ..StreamOptions::default()
    };

    let decoded: StreamOptions =
        serde_json::from_value(serde_json::to_value(&options).expect("serializable"))
            .expect("decodes");

    assert_eq!(decoded, options);
}
