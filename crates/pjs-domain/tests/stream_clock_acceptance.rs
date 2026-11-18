//! Acceptance tests for #759 (track C): `Stream` takes every timestamp it
//! produces from an injected `pjson_rs_domain::clock::TimeProvider`, so a
//! fake clock drives `created_at`, `updated_at`, `completed_at`, `duration()`
//! and the timestamps of the frames the stream builds.

use std::sync::{Arc, Mutex};

use jiff::{SignedDuration, Timestamp};
use pjson_rs_domain::Stream;
use pjson_rs_domain::clock::TimeProvider;
use pjson_rs_domain::entities::stream::StreamConfig;
use pjson_rs_domain::value_objects::{JsonData, Priority, SessionId};
use serde_json::json;

struct ManualClock {
    now: Mutex<Timestamp>,
}

impl ManualClock {
    fn at(start: Timestamp) -> Arc<Self> {
        Arc::new(Self {
            now: Mutex::new(start),
        })
    }

    fn advance(&self, by: SignedDuration) -> Timestamp {
        let mut now = self.now.lock().unwrap();
        *now += by;
        *now
    }
}

impl TimeProvider for ManualClock {
    fn now(&self) -> Timestamp {
        *self.now.lock().unwrap()
    }
}

fn epoch_clock() -> Arc<ManualClock> {
    ManualClock::at(Timestamp::UNIX_EPOCH)
}

fn stream_with(clock: &Arc<ManualClock>) -> Stream {
    Stream::with_clock(
        SessionId::new(),
        JsonData::from(json!({"id": 1, "name": "n", "tags": [1, 2]})),
        StreamConfig::default(),
        clock.clone(),
    )
}

fn seconds(n: i64) -> SignedDuration {
    SignedDuration::from_secs(n)
}

#[test]
fn with_clock_stamps_creation_and_last_update_from_the_clock() {
    let clock = epoch_clock();

    let stream = stream_with(&clock);

    assert_eq!(stream.created_at(), Timestamp::UNIX_EPOCH);
    assert_eq!(stream.updated_at(), Timestamp::UNIX_EPOCH);
}

#[test]
fn start_streaming_stamps_updated_at_from_the_clock() {
    let clock = epoch_clock();
    let mut stream = stream_with(&clock);

    let later = clock.advance(seconds(5));
    stream.start_streaming().unwrap();

    assert_eq!(stream.updated_at(), later);
    assert_eq!(stream.created_at(), Timestamp::UNIX_EPOCH);
}

#[test]
fn complete_stamps_completed_at_and_duration_from_the_clock() {
    let clock = epoch_clock();
    let mut stream = stream_with(&clock);
    stream.start_streaming().unwrap();

    let end = clock.advance(seconds(90));
    stream.complete().unwrap();

    assert_eq!(stream.completed_at(), Some(end));
    assert_eq!(stream.updated_at(), end);
    assert_eq!(stream.duration(), Some(seconds(90)));
}

#[test]
fn fail_and_cancel_stamp_completed_at_from_the_clock() {
    let clock = epoch_clock();
    let mut failed = stream_with(&clock);
    let mut cancelled = stream_with(&clock);
    failed.start_streaming().unwrap();
    cancelled.start_streaming().unwrap();

    let end = clock.advance(seconds(7));
    failed.fail("boom".to_owned()).unwrap();
    cancelled.cancel().unwrap();

    assert_eq!(failed.completed_at(), Some(end));
    assert_eq!(cancelled.completed_at(), Some(end));
    assert_eq!(failed.updated_at(), end);
    assert_eq!(cancelled.updated_at(), end);
}

#[test]
fn add_metadata_stamps_updated_at_from_the_clock() {
    let clock = epoch_clock();
    let mut stream = stream_with(&clock);

    let later = clock.advance(seconds(3));
    stream.add_metadata("k".to_owned(), "v".to_owned());

    assert_eq!(stream.updated_at(), later);
}

#[test]
fn frames_built_by_the_stream_carry_the_clock_time() {
    let clock = epoch_clock();
    let mut stream = stream_with(&clock);
    stream.start_streaming().unwrap();

    let t_skeleton = clock.advance(seconds(1));
    let skeleton = stream.create_skeleton_frame().unwrap();
    let t_patch = clock.advance(seconds(1));
    let patches = stream
        .create_patch_frames(Priority::BACKGROUND, 10)
        .unwrap();
    let t_complete = clock.advance(seconds(1));
    let complete = stream.create_completion_frame(None).unwrap();

    assert_eq!(skeleton.timestamp(), t_skeleton);
    assert!(!patches.is_empty());
    assert!(patches.iter().all(|frame| frame.timestamp() == t_patch));
    assert_eq!(complete.timestamp(), t_complete);
    assert_eq!(stream.updated_at(), t_complete);
}

#[test]
fn new_keeps_using_the_system_clock() {
    let stream = Stream::new(
        SessionId::new(),
        JsonData::from(json!({"a": 1})),
        StreamConfig::default(),
    );

    let drift = Timestamp::now() - stream.created_at();
    assert!(drift.abs() < SignedDuration::from_secs(60), "{drift:?}");
}

#[test]
fn a_deserialized_stream_falls_back_to_the_system_clock() {
    let clock = epoch_clock();
    let mut original = stream_with(&clock);
    original.start_streaming().unwrap();
    let wire = serde_json::to_value(&original).unwrap();

    let mut restored: Stream = serde_json::from_value(wire).unwrap();
    restored.complete().unwrap();

    let completed = restored.completed_at().expect("completed");
    let drift = Timestamp::now() - completed;
    assert!(drift.abs() < SignedDuration::from_secs(60), "{drift:?}");
}
