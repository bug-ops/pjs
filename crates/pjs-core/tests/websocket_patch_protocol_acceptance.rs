//! Acceptance tests for #624 (wire format): `Framing` negotiation in
//! `StreamOptions` and the `StreamPatch` message with its four-op patch
//! grammar `["set"|"merge"|"append"|"append_str", path, arg]`.
//!
//! Patch content is asserted on the JSON wire form, so these tests pin the
//! frozen grammar rather than the Rust field layout of `WirePatch`.
#![cfg(any(feature = "websocket-client", feature = "websocket-server"))]

use pjson_rs::domain::Priority;
use pjson_rs::infrastructure::websocket::{Framing, StreamOptions, WsMessage};
use serde_json::{Value, json};

const SESSION: &str = "00000000-0000-4000-8000-000000000001";

fn stream_patch(patches: Value) -> String {
    json!({
        "type": "StreamPatch",
        "data": {
            "session_id": SESSION,
            "frame_id": 3,
            "priority": 80,
            "patches": patches,
            "is_complete": false
        }
    })
    .to_string()
}

fn default_options_json() -> Value {
    json!({
        "max_frame_size": 65536,
        "client_fps": null,
        "priority_mapping": null
    })
}

#[test]
fn test_framing_default_is_merge() {
    assert_eq!(Framing::default(), Framing::Merge);
    assert_eq!(StreamOptions::default().framing, Framing::Merge);
}

#[test]
fn test_stream_options_merge_framing_wire_omits_framing_key() {
    let encoded = serde_json::to_value(StreamOptions::default()).unwrap();

    assert_eq!(encoded, default_options_json());
    assert!(encoded.get("framing").is_none());
}

#[test]
fn test_stream_options_patch_framing_serializes_as_lowercase_string() {
    let options = StreamOptions {
        framing: Framing::Patch,
        ..StreamOptions::default()
    };

    let encoded = serde_json::to_value(&options).unwrap();

    assert_eq!(encoded["framing"], json!("patch"));
    let decoded: StreamOptions = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, options);
}

#[test]
fn test_stream_init_without_framing_decodes_as_merge() {
    let text = json!({
        "type": "StreamInit",
        "data": {"session_id": SESSION, "data": {"k": 1}, "options": default_options_json()}
    })
    .to_string();

    let message: WsMessage = serde_json::from_str(&text).unwrap();

    let WsMessage::StreamInit { options, .. } = message else {
        panic!("expected StreamInit");
    };
    assert_eq!(options.framing, Framing::Merge);
}

#[test]
fn test_stream_init_with_patch_framing_round_trips() {
    let mut options = default_options_json();
    options["framing"] = json!("patch");
    let text = json!({
        "type": "StreamInit",
        "data": {"session_id": SESSION, "data": [1, 2], "options": options}
    })
    .to_string();

    let message: WsMessage = serde_json::from_str(&text).unwrap();

    let WsMessage::StreamInit { options, .. } = &message else {
        panic!("expected StreamInit");
    };
    assert_eq!(options.framing, Framing::Patch);
    assert_eq!(
        serde_json::to_value(&message).unwrap(),
        serde_json::from_str::<Value>(&text).unwrap()
    );
}

#[test]
fn test_stream_options_unknown_framing_is_rejected() {
    let mut options = default_options_json();
    options["framing"] = json!("zip");

    assert!(serde_json::from_value::<StreamOptions>(options).is_err());
}

#[test]
fn test_stream_patch_decodes_all_four_ops_and_round_trips_unchanged() {
    let patches = json!([
        ["set", ["a.b", 0, "[x]", "", "c]"], {"k": 1}],
        ["merge", [], {"x": 1, "y": [2]}],
        ["append", ["list"], [1, "two", null]],
        ["append_str", ["s"], "chunk \"q\" \u{e9}\n"]
    ]);
    let text = stream_patch(patches.clone());

    let message: WsMessage = serde_json::from_str(&text).unwrap();

    let WsMessage::StreamPatch {
        frame_id,
        priority,
        patches: decoded,
        is_complete,
        ..
    } = &message
    else {
        panic!("expected StreamPatch, got {message:?}");
    };
    assert_eq!(*frame_id, 3);
    assert_eq!(*priority, Priority::HIGH);
    assert_eq!(decoded.len(), 4);
    assert!(!*is_complete);
    assert_eq!(
        serde_json::to_value(&message).unwrap()["data"]["patches"],
        patches
    );
}

#[test]
fn test_stream_patch_distinguishes_numeric_string_key_from_index() {
    let patches = json!([["set", ["0", 0], 1]]);

    let message: WsMessage = serde_json::from_str(&stream_patch(patches.clone())).unwrap();

    assert_eq!(
        serde_json::to_value(&message).unwrap()["data"]["patches"],
        patches,
        "the string \"0\" must stay a key and the integer 0 an index"
    );
}

#[test]
fn test_stream_patch_round_trips_root_set_of_empty_object() {
    let patches = json!([["set", [], {}]]);

    let message: WsMessage = serde_json::from_str(&stream_patch(patches.clone())).unwrap();

    assert_eq!(
        serde_json::to_value(&message).unwrap()["data"]["patches"],
        patches
    );
}

#[test]
fn test_stream_patch_rejects_malformed_patches() {
    let malformed = [
        json!([["set", []]]),
        json!([["set", [], 1, 2]]),
        json!([["bogus", [], 1]]),
        json!([["set", "path", 1]]),
        json!([["set", [true], 1]]),
        json!([["set", [-1], 1]]),
        json!([["set", [1.5], 1]]),
        json!([["set", [null], 1]]),
        json!([["set", [["nested"]], 1]]),
        json!([["merge", [], [1]]]),
        json!([["merge", [], 1]]),
        json!([["append", [], "x"]]),
        json!([["append_str", [], 1]]),
        json!([{"op": "set"}]),
        json!(["set"]),
        json!([[]]),
        json!([[1, [], 2]]),
        json!("not an array"),
    ];

    let control = stream_patch(json!([["set", [], 1]]));
    assert!(
        serde_json::from_str::<WsMessage>(&control).is_ok(),
        "control: a well-formed StreamPatch must decode"
    );
    for patches in malformed {
        let text = stream_patch(patches.clone());
        assert!(
            serde_json::from_str::<WsMessage>(&text).is_err(),
            "must reject patches {patches}"
        );
    }
}

#[test]
fn test_stream_patch_rejects_invalid_priority_like_stream_frame() {
    let mut value: Value = serde_json::from_str(&stream_patch(json!([["set", [], 1]]))).unwrap();
    assert!(
        serde_json::from_value::<WsMessage>(value.clone()).is_ok(),
        "control: priority 80 must decode"
    );
    value["data"]["priority"] = json!(0);

    assert!(serde_json::from_value::<WsMessage>(value).is_err());
}
