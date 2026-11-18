//! Acceptance tests for #761 (track B): a `JsonReconstructor` rejects every
//! frame after a processed `Complete`, `ReconstructionStats` is a read-only view
//! with `u64` counters and one consistent timing source, and
//! `ProcessResult::PatchesApplied` no longer carries a redundant `count`.

use pjson_rs::Error;
use pjson_rs::domain::value_objects::{JsonPath, Priority};
use pjson_rs::error::FrameSequenceError;
use pjson_rs::stream::JsonReconstructor;
use pjson_rs::stream::patch::WirePatch;
use pjson_rs::stream::priority::{JsonPatch, PriorityStreamFrame};
use pjson_rs::stream::reconstruction::ProcessResult;
use serde_json::{Value, json};

fn skeleton(data: Value) -> PriorityStreamFrame {
    PriorityStreamFrame::Skeleton {
        data,
        priority: Priority::CRITICAL,
        complete: false,
    }
}

fn set_patch(path: &str, value: Value) -> PriorityStreamFrame {
    PriorityStreamFrame::Patch {
        patches: vec![JsonPatch {
            patch: WirePatch::Set {
                path: JsonPath::new(path).expect("valid path"),
                value,
            },
            priority: Priority::HIGH,
        }],
        priority: Priority::HIGH,
    }
}

fn complete() -> PriorityStreamFrame {
    PriorityStreamFrame::Complete { checksum: None }
}

fn completed_reconstructor() -> JsonReconstructor {
    let mut reconstructor = JsonReconstructor::new();
    reconstructor.add_frame(skeleton(json!({"a": 0})));
    reconstructor.add_frame(set_patch("$.a", json!(1)));
    reconstructor.add_frame(complete());
    reconstructor.process_all_frames().expect("stream applies");
    assert!(reconstructor.is_complete());
    reconstructor
}

fn assert_after_complete(result: Result<impl std::fmt::Debug, Error>) {
    assert!(
        matches!(
            result,
            Err(Error::FrameSequence(FrameSequenceError::AfterComplete))
        ),
        "expected FrameSequence(AfterComplete), got {result:?}"
    );
}

#[test]
fn process_all_frames_rejects_a_skeleton_after_complete() {
    let mut reconstructor = completed_reconstructor();
    reconstructor.add_frame(skeleton(json!({"late": 1})));

    assert_after_complete(reconstructor.process_all_frames());

    assert_eq!(reconstructor.current_state(), &json!({"a": 1}));
}

#[test]
fn process_next_frame_rejects_a_patch_after_complete() {
    let mut reconstructor = completed_reconstructor();
    reconstructor.add_frame(set_patch("$.a", json!(99)));

    assert_after_complete(reconstructor.process_next_frame());

    assert_eq!(reconstructor.current_state(), &json!({"a": 1}));
}

#[test]
fn a_rejected_frame_leaves_the_stats_unchanged() {
    let mut reconstructor = completed_reconstructor();
    let frames_before = reconstructor.stats().frames_processed();
    let patches_before = reconstructor.stats().patches_applied();
    let duration_before = reconstructor.stats().duration();
    reconstructor.add_frame(set_patch("$.a", json!(99)));

    assert_after_complete(reconstructor.process_next_frame());

    let stats = reconstructor.stats();
    assert_eq!(stats.frames_processed(), frames_before);
    assert_eq!(stats.patches_applied(), patches_before);
    assert_eq!(stats.duration(), duration_before);
    assert!(reconstructor.is_complete());
}

#[test]
fn process_all_frames_stops_at_the_first_rejection_and_keeps_later_frames_queued() {
    let mut reconstructor = completed_reconstructor();
    reconstructor.add_frame(skeleton(json!({"late": 1})));
    reconstructor.add_frame(set_patch("$.a", json!(99)));

    assert_after_complete(reconstructor.process_all_frames());
    // The rejected skeleton was consumed; the patch behind it is rejected the same way.
    assert_after_complete(reconstructor.process_next_frame());
    assert!(matches!(
        reconstructor.process_next_frame(),
        Ok(ProcessResult::NoFrames)
    ));
    assert_eq!(reconstructor.current_state(), &json!({"a": 1}));
}

#[test]
fn a_second_complete_after_complete_is_rejected() {
    let mut reconstructor = completed_reconstructor();
    reconstructor.add_frame(complete());

    assert_after_complete(reconstructor.process_next_frame());
}

#[test]
fn reset_accepts_a_new_stream_after_complete() {
    let mut reconstructor = completed_reconstructor();

    reconstructor.reset();
    reconstructor.add_frame(skeleton(json!({"fresh": true})));
    let result = reconstructor.process_all_frames();

    assert!(result.is_ok(), "{result:?}");
    assert!(!reconstructor.is_complete());
    assert_eq!(reconstructor.current_state(), &json!({"fresh": true}));
    assert_eq!(reconstructor.stats().frames_processed(), 1);
}

#[test]
fn stats_counters_are_u64_getters_that_count_each_frame_kind() {
    let reconstructor = completed_reconstructor();

    let stats = reconstructor.stats();
    let counts: [u64; 4] = [
        stats.frames_processed(),
        stats.patches_applied(),
        stats.skeleton_frames(),
        stats.patch_frames(),
    ];
    assert_eq!(counts, [3, 1, 1, 1]);
}

#[test]
fn duration_is_none_until_complete_and_some_after() {
    let mut reconstructor = JsonReconstructor::new();
    assert_eq!(reconstructor.stats().duration(), None);

    reconstructor.add_frame(skeleton(json!({"a": 0})));
    reconstructor.process_all_frames().expect("skeleton");
    assert_eq!(reconstructor.stats().duration(), None);
    assert_eq!(reconstructor.stats().frames_per_second(), None);
    assert_eq!(reconstructor.stats().patches_per_second(), None);

    reconstructor.add_frame(complete());
    reconstructor.process_all_frames().expect("complete");
    assert!(reconstructor.stats().duration().is_some());
}

#[test]
fn reconstructor_duration_matches_the_stats_duration() {
    let reconstructor = completed_reconstructor();

    assert_eq!(reconstructor.duration(), reconstructor.stats().duration());
    assert!(reconstructor.duration().is_some());
}

#[test]
fn patches_applied_reports_priority_and_the_modified_paths() {
    let mut reconstructor = JsonReconstructor::new();
    reconstructor.add_frame(skeleton(json!({"a": 0, "b": 0})));
    reconstructor.add_frame(PriorityStreamFrame::Patch {
        patches: ["$.a", "$.b"]
            .map(|path| JsonPatch {
                patch: WirePatch::Set {
                    path: JsonPath::new(path).expect("valid path"),
                    value: json!(1),
                },
                priority: Priority::HIGH,
            })
            .to_vec(),
        priority: Priority::HIGH,
    });

    let results = reconstructor.process_all_frames().expect("frames apply");

    // No `count` field: the pattern is exhaustive without `..`.
    let ProcessResult::PatchesApplied { priority, paths } = &results[1] else {
        panic!("expected PatchesApplied, got {:?}", results[1]);
    };
    assert_eq!(*priority, Priority::HIGH);
    assert_eq!(
        paths,
        &vec![
            JsonPath::new("$.a").expect("valid path"),
            JsonPath::new("$.b").expect("valid path"),
        ]
    );
}
