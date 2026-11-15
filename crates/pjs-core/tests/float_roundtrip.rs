//! Acceptance tests for #575: the opt-in `float-roundtrip` feature.
//!
//! With the feature on, `serde_json` parses every `f64` to the correctly
//! rounded value, so a number survives a `WsMessage` decode/encode round trip
//! bit for bit. Without it the default parser may be 1 ULP off; those literals
//! are pinned below (found with a throwaway search against serde_json 1.0.154:
//! each one decodes 1 ULP away from `str::parse::<f64>` without the feature).
#![cfg(feature = "float-roundtrip")]

use pjson_rs::infrastructure::websocket::WsMessage;

/// Literals the default (best-effort) serde_json float parser misses by 1 ULP.
const DRIFTING_LITERALS: [&str; 4] = [
    "985.6906946328695",
    "212.91890726713459",
    "479.60756426982596",
    "92.42132512813595",
];

fn stream_frame_text(literal: &str) -> String {
    format!(
        r#"{{"type":"StreamFrame","data":{{"session_id":"00000000-0000-4000-8000-000000000001","frame_id":0,"priority":80,"payload":{{"x":{literal}}},"is_complete":true}}}}"#
    )
}

fn payload_x(message: &WsMessage) -> f64 {
    let WsMessage::StreamFrame { payload, .. } = message else {
        panic!("expected a StreamFrame, got {message:?}");
    };
    payload["x"].as_f64().expect("payload.x is a number")
}

#[test]
fn test_ws_message_decode_float_is_correctly_rounded() {
    for literal in DRIFTING_LITERALS {
        let expected: f64 = literal.parse().expect("valid f64 literal");

        let message: WsMessage =
            serde_json::from_str(&stream_frame_text(literal)).expect("frame decodes");

        assert_eq!(
            payload_x(&message).to_bits(),
            expected.to_bits(),
            "{literal} must decode to the correctly rounded f64"
        );
    }
}

#[test]
fn test_ws_message_encode_decode_float_is_bit_exact() {
    for literal in DRIFTING_LITERALS {
        let expected: f64 = literal.parse().expect("valid f64 literal");
        let first: WsMessage =
            serde_json::from_str(&stream_frame_text(literal)).expect("frame decodes");

        let reencoded = serde_json::to_string(&first).expect("frame encodes");
        let second: WsMessage = serde_json::from_str(&reencoded).expect("re-encoded frame decodes");

        assert_eq!(
            payload_x(&second).to_bits(),
            expected.to_bits(),
            "{literal} must survive decode -> encode -> decode"
        );
    }
}
