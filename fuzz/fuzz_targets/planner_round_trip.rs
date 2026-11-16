#![no_main]

use libfuzzer_sys::fuzz_target;
use pjs_fuzz::approx_eq;
use pjson_rs::infrastructure::{
    StreamOptions, WsMessage,
    websocket::{
        Framing,
        fuzzing::{ClientPatchHarness, plan_messages},
    },
};
use serde_json::{Map, Value};

const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const HEAD_BYTES: usize = 4;

fn replay_patches(messages: &[WsMessage]) -> Value {
    let mut harness = ClientPatchHarness::new();
    for message in messages {
        let WsMessage::StreamPatch { patches, .. } = message else {
            panic!("patch framing must plan only StreamPatch: {message:?}");
        };
        harness
            .apply_patches(patches.clone(), usize::MAX)
            .expect("planned patches apply");
    }
    harness.reconstructed().clone()
}

fn replay_merge(messages: &[WsMessage]) -> Value {
    let mut merged = Map::new();
    for message in messages {
        let WsMessage::StreamFrame {
            payload: Value::Object(entries),
            ..
        } = message
        else {
            panic!("merge framing must plan only object StreamFrame: {message:?}");
        };
        merged.extend(entries.clone());
    }
    Value::Object(merged)
}

fn check_bookkeeping(messages: &[WsMessage]) {
    for (index, message) in messages.iter().enumerate() {
        let (frame_id, is_complete) = match message {
            WsMessage::StreamPatch {
                frame_id,
                is_complete,
                ..
            }
            | WsMessage::StreamFrame {
                frame_id,
                is_complete,
                ..
            } => (*frame_id, *is_complete),
            other => panic!("unexpected planned message: {other:?}"),
        };
        assert_eq!(frame_id as usize, index, "frame ids are sequential");
        assert_eq!(
            is_complete,
            index + 1 == messages.len(),
            "only the last frame completes"
        );
    }
}

fn is_splittable(message: &WsMessage) -> bool {
    match message {
        WsMessage::StreamPatch { patches, .. } => patches.len() > 1,
        WsMessage::StreamFrame {
            payload: Value::Object(entries),
            ..
        } => entries.len() > 1,
        _ => false,
    }
}

/// 24-bit little-endian size: covers every value up to the planner's 16 MiB cap.
fn frame_size(head: &[u8; HEAD_BYTES]) -> usize {
    let size = u32::from_le_bytes([head[1], head[2], head[3], 0]);
    usize::try_from(size).unwrap_or(usize::MAX)
}

fn check_merge_root(messages: &[WsMessage], document: &Value) {
    if document.is_object() {
        assert!(
            approx_eq(&replay_merge(messages), document),
            "merge round trip drifted"
        );
        return;
    }
    let [
        WsMessage::StreamFrame {
            payload,
            is_complete: true,
            ..
        },
    ] = messages
    else {
        panic!("non-object merge root must be one complete frame: {messages:?}");
    };
    assert!(approx_eq(payload, document), "whole payload drifted");
}

fuzz_target!(|data: &[u8]| {
    let Some((head, body)) = data.split_first_chunk::<HEAD_BYTES>() else {
        return;
    };
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    let framing = if head[0] & 1 == 0 {
        Framing::Merge
    } else {
        Framing::Patch
    };
    let options = StreamOptions {
        max_frame_size: frame_size(head),
        framing,
        ..StreamOptions::default()
    };

    let Ok(texts) = plan_messages(document.clone(), &options) else {
        return;
    };
    let messages: Vec<WsMessage> = texts
        .iter()
        .map(|text| serde_json::from_str(text).expect("planner output decodes"))
        .collect();
    check_bookkeeping(&messages);

    for (text, message) in texts.iter().zip(&messages) {
        assert!(
            text.len() <= MAX_MESSAGE_BYTES,
            "message over the queue budget"
        );
        assert!(
            !is_splittable(message) || text.len() <= options.max_frame_size,
            "{} > {}",
            text.len(),
            options.max_frame_size
        );
    }

    match framing {
        Framing::Patch => assert!(
            approx_eq(&replay_patches(&messages), &document),
            "round trip drifted"
        ),
        Framing::Merge => check_merge_root(&messages, &document),
    }
});
