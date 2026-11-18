#![no_main]

use libfuzzer_sys::fuzz_target;
use pjs_fuzz::reparsed;
use pjson_rs::infrastructure::{
    StreamOptions, WsMessage,
    websocket::{
        Framing, MaxFrameSize,
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

fn check_merge_root(messages: &[WsMessage], expected: &Value) {
    if expected.is_object() {
        assert_eq!(
            &replay_merge(messages),
            expected,
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
    assert_eq!(payload, expected, "whole payload drifted");
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
    let Ok(max_frame_size) = MaxFrameSize::new(frame_size(head)) else {
        return;
    };
    let mut options = StreamOptions::default();
    options.max_frame_size = max_frame_size;
    options.framing = framing;

    let Ok(texts) = plan_messages(document.clone(), &options) else {
        return;
    };
    let messages: Vec<WsMessage> = texts
        .iter()
        .map(|text| serde_json::from_str(text).expect("planner output decodes"))
        .collect();
    check_bookkeeping(&messages);

    let planned_bytes: usize = texts.iter().map(String::len).sum();
    let document_bytes = serde_json::to_vec(&document)
        .expect("a Value serializes")
        .len();
    assert!(
        planned_bytes >= document_bytes,
        "plan holds {planned_bytes} bytes, fewer than the {document_bytes}-byte document"
    );

    for (text, message) in texts.iter().zip(&messages) {
        assert!(
            text.len() <= MAX_MESSAGE_BYTES,
            "message over the queue budget"
        );
        assert!(
            !is_splittable(message) || text.len() <= max_frame_size.get(),
            "{} > {}",
            text.len(),
            max_frame_size.get()
        );
    }

    let expected = reparsed(&document);
    match framing {
        Framing::Patch => assert_eq!(replay_patches(&messages), expected, "round trip drifted"),
        Framing::Merge => check_merge_root(&messages, &expected),
        _ => unreachable!("the target picks Merge or Patch"),
    }
});
