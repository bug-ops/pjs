//! Acceptance tests for #643: the domain `Stream` must not lose values stored
//! under object keys that `JsonPath` cannot encode (`.`, `[`, `]`, empty), at
//! any depth. Frames are replayed through the real `JsonReconstructor`.

use std::collections::HashMap;

use pjson_rs::domain::entities::frame::FrameType;
use pjson_rs::domain::entities::stream::StreamConfig;
use pjson_rs::domain::value_objects::{JsonData, JsonPath, Priority, SessionId};
use pjson_rs::domain::{Frame, Stream};
use pjson_rs::stream::priority::{JsonPatch, PatchOperation as CorePatchOp, PriorityStreamFrame};
use pjson_rs::stream::reconstruction::JsonReconstructor;
use serde_json::{Value, json};

fn unencodable_docs() -> Vec<Value> {
    vec![
        json!({"a.b": 1, "ok": 2}),
        json!({"x": {"a[0]": "v"}}),
        json!({"": 1, "ok": [1, 2]}),
        json!({"a.b": {"c": [1, 2, {"d.e": true}]}, "id": 7}),
        json!({"k]": [1, 2, 3], "list": [4, 5], "n": {"x": [1]}}),
        json!({"outer": {"weird.key": [1, 2, 3], "fine": "y"}}),
        json!({"a": {"b": {"c[": {"d": null}}}, "z": 0.5}),
    ]
}

fn streaming(data: &Value, config: StreamConfig) -> Stream {
    let mut stream = Stream::new(SessionId::new(), JsonData::from(data.clone()), config);
    stream.start_streaming().expect("fresh stream starts");
    stream
}

fn payload_value(frame: &Frame) -> Value {
    serde_json::to_value(frame.payload()).expect("JsonData serializes")
}

fn to_priority_frame(frame: &Frame) -> PriorityStreamFrame {
    let priority = frame.priority();
    let payload = payload_value(frame);
    match frame.frame_type() {
        FrameType::Skeleton => PriorityStreamFrame::Skeleton {
            data: payload,
            priority,
            complete: false,
        },
        FrameType::Patch => {
            let patches = payload["patches"]
                .as_array()
                .expect("patch frame carries a patches array")
                .iter()
                .map(|p| {
                    let path = JsonPath::new(p["path"].as_str().expect("path is a string"))
                        .expect("emitted path parses");
                    let value = p["value"].clone();
                    let operation = match p["operation"].as_str().expect("operation") {
                        "set" => CorePatchOp::Set { value },
                        "append" => CorePatchOp::Append {
                            values: value.as_array().cloned().unwrap_or_else(|| vec![value]),
                        },
                        other => panic!("unexpected patch operation {other}"),
                    };
                    JsonPatch {
                        path,
                        operation,
                        priority,
                    }
                })
                .collect();
            PriorityStreamFrame::Patch { patches, priority }
        }
        other => panic!("unexpected frame type {other:?}"),
    }
}

/// Skeleton frame followed by every patch frame, applied in emission order.
fn reconstruct(data: &Value, config: StreamConfig, max_frames: usize) -> Value {
    let mut stream = streaming(data, config);
    let skeleton = stream.create_skeleton_frame().expect("skeleton");
    let frames = stream
        .create_patch_frames(Priority::BACKGROUND, max_frames)
        .expect("patch frames");

    let mut reconstructor = JsonReconstructor::new();
    reconstructor.add_frame(to_priority_frame(&skeleton));
    for frame in &frames {
        reconstructor.add_frame(to_priority_frame(frame));
    }
    reconstructor
        .process_all_frames()
        .expect("every emitted frame applies");
    reconstructor.current_state().clone()
}

#[test]
fn test_create_patch_frames_round_trips_unencodable_keys_at_any_depth() {
    for data in unencodable_docs() {
        for max_frames in [1, 2, 1000] {
            assert_eq!(
                reconstruct(&data, StreamConfig::default(), max_frames),
                data,
                "max_frames = {max_frames}"
            );
        }
    }
}

#[test]
fn test_create_patch_frames_round_trips_issue_repro_documents() {
    let top = json!({"a.b": 1, "ok": 2});
    let nested = json!({"x": {"a[0]": "v"}});

    assert_eq!(reconstruct(&top, StreamConfig::default(), 10), top);
    assert_eq!(reconstruct(&nested, StreamConfig::default(), 10), nested);
}

#[test]
fn test_create_patch_frames_round_trips_documents_without_unencodable_keys() {
    let data = json!({"a": 1, "b": [1, 2, 3], "c": {"d": "x", "e": {"f": [true]}}});

    assert_eq!(reconstruct(&data, StreamConfig::default(), 1000), data);
}

#[test]
fn test_create_patch_frames_round_trips_with_priority_rules() {
    let data = json!({"a.b": {"c": 1}, "ok": 2, "list": [1, 2, 3]});
    let rules = HashMap::from([
        ("ok".to_owned(), Priority::CRITICAL),
        ("list".to_owned(), Priority::BACKGROUND),
    ]);
    let config = StreamConfig {
        priority_rules: rules,
        ..StreamConfig::default()
    };

    assert_eq!(reconstruct(&data, config, 3), data);
}

#[test]
fn test_create_patch_frames_round_trips_generated_key_alphabet_documents() {
    const KEYS: [&str; 8] = ["a", "a.b", "[0]", "x]", "", "k", "n.", "[z"];
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % bound as u64) as usize
    };

    fn build(next: &mut impl FnMut(usize) -> usize, depth: usize) -> Value {
        match (depth, next(4)) {
            (0, _) | (_, 0) => json!(next(100)),
            (_, 1) => Value::Array((0..next(3)).map(|_| build(next, depth - 1)).collect()),
            _ => Value::Object(
                (0..=next(3))
                    .map(|_| (KEYS[next(KEYS.len())].to_owned(), build(next, depth - 1)))
                    .collect(),
            ),
        }
    }

    for _ in 0..200 {
        let data = Value::Object(
            (0..=next(3))
                .map(|_| (KEYS[next(KEYS.len())].to_owned(), build(&mut next, 3)))
                .collect(),
        );
        assert_eq!(
            reconstruct(&data, StreamConfig::default(), 1000),
            data,
            "generated document {data}"
        );
    }
}

#[test]
fn test_extract_prioritized_patches_addresses_unencodable_keys_without_a_root_set() {
    let data = json!({"a.b": {"c": 1}, "ok": 2, "list": [1, 2]});
    let stream = streaming(&data, StreamConfig::default());

    let patches = stream
        .extract_prioritized_patches(Priority::BACKGROUND)
        .expect("patches");

    assert!(
        patches.iter().all(|(p, _)| !p.path.segments().is_empty()),
        "no patch targets the root"
    );
    assert!(
        patches
            .iter()
            .any(|(p, _)| p.path.to_string() == r#"$["a.b"].c"#),
        "the unencodable key is addressed by a quoted segment"
    );
}

#[test]
fn test_extract_prioritized_patches_has_no_root_set_without_unencodable_keys() {
    let data = json!({"a": 1, "b": [1, 2], "c": {"d": "x"}});
    let stream = streaming(&data, StreamConfig::default());

    let patches = stream
        .extract_prioritized_patches(Priority::BACKGROUND)
        .expect("patches");

    assert!(!patches.is_empty());
    assert!(
        patches.iter().all(|(p, _)| !p.path.segments().is_empty()),
        "documents without unencodable keys must not emit a root patch"
    );
}

#[test]
fn test_create_skeleton_frame_does_not_ship_unencodable_subtrees_verbatim() {
    let data = json!({"a.b": {"c": [1, 2]}, "x": {"a[0]": [3, 4]}, "ok": 5});
    let mut stream = streaming(&data, StreamConfig::default());

    let skeleton = payload_value(&stream.create_skeleton_frame().expect("skeleton"));

    assert_eq!(skeleton["a.b"], json!({"c": []}));
    assert_eq!(skeleton["x"]["a[0]"], json!([]));
}
