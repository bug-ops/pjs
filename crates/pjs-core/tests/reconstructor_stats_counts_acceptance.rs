//! Acceptance tests for #761 (track B, coverage gap): every
//! `ReconstructionStats` getter counts its own thing (distinct values per
//! counter), and the per-second rates divide by the stats duration.

use std::time::Duration;

use pjson_rs::domain::value_objects::{JsonPath, Priority};
use pjson_rs::stream::JsonReconstructor;
use pjson_rs::stream::patch::WirePatch;
use pjson_rs::stream::priority::{JsonPatch, PriorityStreamFrame};
use serde_json::json;

fn skeleton() -> PriorityStreamFrame {
    PriorityStreamFrame::Skeleton {
        data: json!({"a": 0, "b": 0}),
        priority: Priority::CRITICAL,
        complete: false,
    }
}

fn patch_frame(paths: &[&str]) -> PriorityStreamFrame {
    PriorityStreamFrame::Patch {
        patches: paths
            .iter()
            .map(|path| JsonPatch {
                patch: WirePatch::Set {
                    path: JsonPath::new(*path).expect("valid path"),
                    value: json!(1),
                },
                priority: Priority::HIGH,
            })
            .collect(),
        priority: Priority::HIGH,
    }
}

fn run(complete_after: Duration) -> JsonReconstructor {
    let mut reconstructor = JsonReconstructor::new();
    // 2 skeleton frames, 3 patch frames carrying 2 + 2 + 1 patches, 1 complete.
    for frame in [
        skeleton(),
        skeleton(),
        patch_frame(&["$.a", "$.b"]),
        patch_frame(&["$.a", "$.b"]),
        patch_frame(&["$.a"]),
    ] {
        reconstructor.add_frame(frame);
    }
    reconstructor.process_all_frames().expect("frames apply");
    std::thread::sleep(complete_after);
    reconstructor.add_frame(PriorityStreamFrame::Complete { checksum: None });
    reconstructor.process_all_frames().expect("complete");
    reconstructor
}

#[test]
fn each_counter_getter_reports_its_own_count() {
    let reconstructor = run(Duration::ZERO);

    let stats = reconstructor.stats();
    assert_eq!(stats.frames_processed(), 6);
    assert_eq!(stats.patches_applied(), 5);
    assert_eq!(stats.skeleton_frames(), 2);
    assert_eq!(stats.patch_frames(), 3);
}

#[test]
fn rates_divide_the_counts_by_the_stats_duration() {
    let reconstructor = run(Duration::from_millis(30));

    let stats = reconstructor.stats();
    let seconds = stats.duration().expect("complete").as_secs_f32();
    assert!(seconds >= 0.03, "{seconds}");
    let frames = stats.frames_per_second().expect("a rate after completion");
    let patches = stats.patches_per_second().expect("a rate after completion");
    assert!((frames - 6.0 / seconds).abs() <= 1e-3 * frames, "{frames}");
    assert!(
        (patches - 5.0 / seconds).abs() <= 1e-3 * patches,
        "{patches}"
    );
}
