//! Tests closing the mutation survivors and cheap gaps found while validating
//! #750-#753: heuristic boundaries, per-stream priority rules, `Hash for
//! Frame`, frame decoding extras, and `chunk_patches_for_commit` at `usize::MAX`.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use pjson_rs_domain::Frame;
use pjson_rs_domain::Stream;
use pjson_rs_domain::entities::frame::FramePatch;
use pjson_rs_domain::entities::stream::StreamConfig;
use pjson_rs_domain::services::{PriorityHeuristicConfig, ValueShape, compute_priority};
use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority, SessionId, StreamId};
use serde_json::json;

const STREAM: &str = "22222222-2222-4222-8222-222222222222";

fn path(text: &str) -> JsonPath {
    JsonPath::new(text).expect("valid path")
}

fn hash_of(frame: &Frame) -> u64 {
    let mut hasher = DefaultHasher::new();
    frame.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn compute_priority_thresholds_are_exclusive_at_the_boundary() {
    let cfg = PriorityHeuristicConfig::default();
    let rows = [
        (ValueShape::Array { len: 100 }, 50 + 20 - 15),
        (ValueShape::Array { len: 101 }, 50 + 20 - 40),
        (ValueShape::Object { len: 10 }, 50 + 20),
        (ValueShape::Object { len: 11 }, 50 + 20 - 10),
    ];
    for (shape, expected) in rows {
        assert_eq!(
            compute_priority(&cfg, &path("$.unknown"), shape).value(),
            expected,
            "{shape:?}"
        );
    }
}

#[test]
fn extract_prioritized_patches_applies_the_stream_priority_rules() {
    let config = StreamConfig {
        priority_rules: HashMap::from([("name".to_owned(), Priority::BACKGROUND)]),
    };
    let mut stream = Stream::new(
        SessionId::new(),
        JsonData::from(json!({"name": "x", "id": 1})),
        config,
    );
    stream.start_streaming().unwrap();

    let patches = stream
        .extract_prioritized_patches(Priority::BACKGROUND)
        .unwrap();

    let priority_of = |target: &str| {
        patches
            .iter()
            .find(|(patch, _)| *patch.path() == path(target))
            .map(|(_, priority)| *priority)
    };
    assert_eq!(priority_of("$.name"), Some(Priority::BACKGROUND));
    assert_eq!(priority_of("$.id"), Some(Priority::CRITICAL));
}

#[test]
fn frame_hash_follows_equality_and_sees_every_field() {
    let id = StreamId::from_string(STREAM).unwrap();
    let frame = Frame::complete(id, 4, Some("abc".to_owned()));
    let same = frame.clone();
    assert_eq!(hash_of(&frame), hash_of(&same));

    let with_metadata = frame.clone().with_metadata("k".to_owned(), "v".to_owned());
    assert_ne!(hash_of(&frame), hash_of(&with_metadata));
    let other_checksum = Frame::complete(id, 4, Some("abd".to_owned()));
    assert_ne!(hash_of(&frame), hash_of(&other_checksum));
}

#[test]
fn frame_round_trips_several_metadata_entries() {
    let id = StreamId::from_string(STREAM).unwrap();
    let frame = Frame::complete(id, 4, None)
        .with_metadata("a".to_owned(), "1".to_owned())
        .with_metadata("b".to_owned(), "2".to_owned());
    let decoded: Frame = serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();
    assert_eq!(decoded, frame);
}

#[test]
fn frame_deserialize_rejects_an_unknown_frame_type() {
    let value = json!({
        "stream_id": STREAM,
        "frame_type": "Heartbeat",
        "priority": 100,
        "sequence": 1,
        "timestamp": "2024-01-02T03:04:05Z",
        "payload": {},
        "metadata": {}
    });
    assert!(serde_json::from_value::<Frame>(value).is_err());
}

#[test]
fn chunk_patches_for_commit_with_usize_max_frames_does_not_panic_and_chunks_by_ceil() {
    let patches: Vec<(FramePatch, Priority)> = (0..5)
        .map(|i| {
            (
                FramePatch::set(path(&format!("$.f{i}")), JsonData::Integer(i)),
                Priority::new(10 + i as u8).unwrap(),
            )
        })
        .collect();

    let chunks = Stream::chunk_patches_for_commit(patches, usize::MAX);

    assert_eq!(
        chunks.len(),
        5,
        "one patch per frame when frames outnumber patches"
    );
}
