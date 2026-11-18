//! Acceptance tests for #750: `PriorityStreamer` prices every `Set` and
//! `Append` patch with the shared `compute_priority` heuristic, so a document
//! is ordered the same way on every transport, and its frames still rebuild
//! the input at any depth (spec 031).

use pjson_rs::domain::value_objects::{JsonPath, Priority};
use pjson_rs::stream::JsonReconstructor;
use pjson_rs::stream::patch::WirePatch;
use pjson_rs::stream::priority::{PriorityStreamFrame, PriorityStreamer, StreamingPlan};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Set,
    Append,
}

fn path(text: &str) -> JsonPath {
    JsonPath::new(text).expect("valid path")
}

fn plan_of(document: &Value) -> StreamingPlan {
    PriorityStreamer::new().analyze(document).expect("plan")
}

fn patches(plan: &StreamingPlan) -> Vec<(Kind, JsonPath, Priority)> {
    plan.frames()
        .filter_map(|frame| match frame {
            PriorityStreamFrame::Patch { patches, .. } => Some(patches),
            _ => None,
        })
        .flatten()
        .map(|patch| match &patch.patch {
            WirePatch::Set { path, .. } => (Kind::Set, path.clone(), patch.priority),
            WirePatch::Append { path, .. } => (Kind::Append, path.clone(), patch.priority),
            other => panic!("PriorityStreamer must only emit Set and Append, got {other:?}"),
        })
        .collect()
}

fn priority_of(plan: &StreamingPlan, kind: Kind, target: &str) -> Vec<Priority> {
    let target = path(target);
    patches(plan)
        .into_iter()
        .filter(|(k, p, _)| *k == kind && *p == target)
        .map(|(_, _, priority)| priority)
        .collect()
}

#[test]
fn analyze_prices_field_names_like_compute_priority() {
    let plan = plan_of(&json!({
        "state": "ok",
        "error": null,
        "description": "d",
        "message": "m",
        "Name": "n",
        "created_at": "t",
        "version": 1,
        "analytics": 1,
        "user_stats": 1,
        "metadata_x": 1,
        "nickname": "ada"
    }));

    let rows = [
        ("$.state", Priority::CRITICAL),
        ("$.error", Priority::CRITICAL),
        ("$.description", Priority::HIGH),
        ("$.message", Priority::HIGH),
        ("$.Name", Priority::HIGH),
        ("$.created_at", Priority::LOW),
        ("$.version", Priority::LOW),
        ("$.analytics", Priority::BACKGROUND),
        // fallback: MEDIUM + depth-1 boost 20 (a short string adds 5 more)
        ("$.user_stats", Priority::new(70).unwrap()),
        ("$.metadata_x", Priority::new(70).unwrap()),
        ("$.nickname", Priority::new(75).unwrap()),
    ];
    for (target, expected) in rows {
        assert_eq!(
            priority_of(&plan, Kind::Set, target),
            vec![expected],
            "Set at {target}"
        );
    }
}

#[test]
fn analyze_applies_the_depth_adjustment() {
    // An Append at each depth is always emitted, so it exposes the depth
    // pricing that no-op `Set` elimination would otherwise hide.
    let plan = plan_of(&json!({"a": {"x": [1], "b": {"x": [1], "c": {"x": [1],
        "d": {"x": [1], "e": {"x": [1], "f": {"x": [1], "g": {"x": [1]}}}}}}}}));

    let rows = [
        ("$.a.x", 60),
        ("$.a.b.x", 50),
        ("$.a.b.c.x", 50),
        ("$.a.b.c.d.x", 50),
        ("$.a.b.c.d.e.x", 40),
        ("$.a.b.c.d.e.f.x", 40),
        ("$.a.b.c.d.e.f.g.x", 40),
    ];
    for (target, expected) in rows {
        assert_eq!(
            priority_of(&plan, Kind::Append, target),
            vec![Priority::new(expected).unwrap()],
            "Append at {target}"
        );
    }
}

#[test]
fn analyze_prices_arrays_by_length_and_name() {
    let plan = plan_of(&json!({
        "list": [1, 2, 3],
        "items": (0..60).collect::<Vec<_>>(),
        "big": (0..120).collect::<Vec<_>>(),
        "reviews": [1, 2]
    }));

    let rows = [
        ("$.list", Priority::new(70).unwrap()),
        ("$.items", Priority::new(55).unwrap()),
        ("$.reviews", Priority::BACKGROUND),
    ];
    for (target, expected) in rows {
        assert_eq!(
            priority_of(&plan, Kind::Append, target),
            vec![expected],
            "Append at {target}"
        );
        assert_eq!(
            priority_of(&plan, Kind::Set, target),
            vec![expected],
            "Set at {target}"
        );
    }
    // 50 + 20 - 40; both chunks of the one array share the priority
    let big = Priority::new(30).unwrap();
    assert_eq!(priority_of(&plan, Kind::Append, "$.big"), vec![big, big]);
}

#[test]
fn analyze_raises_a_set_to_the_highest_append_priority_below_it() {
    let plan = plan_of(&json!({"o": {"id": [1, 2]}}));

    assert_eq!(
        priority_of(&plan, Kind::Append, "$.o.id"),
        vec![Priority::CRITICAL]
    );
    assert_eq!(
        priority_of(&plan, Kind::Set, "$.o"),
        vec![Priority::CRITICAL]
    );
}

#[test]
fn analyze_orders_frames_by_the_shared_heuristic() {
    let plan = plan_of(&json!({"analytics": 1, "nickname": "ada", "state": "ok"}));

    let frame_priorities: Vec<Priority> = plan
        .frames()
        .filter_map(|frame| match frame {
            PriorityStreamFrame::Patch { priority, .. } => Some(*priority),
            _ => None,
        })
        .collect();
    assert!(
        frame_priorities.windows(2).all(|w| w[0] >= w[1]),
        "{frame_priorities:?}"
    );
    assert_eq!(frame_priorities.first(), Some(&Priority::CRITICAL));
    assert_eq!(frame_priorities.last(), Some(&Priority::BACKGROUND));
}

fn deeply_nested(levels: usize) -> Value {
    let mut value = json!({"leaf": [1, 2, 3], "state": "ok"});
    for _ in 0..levels {
        value = json!({"k": value, "id": 1, "tags": ["a", "b"]});
    }
    value
}

#[test]
fn analyze_frames_rebuild_the_input_at_any_depth() {
    for document in [
        json!({"state": "ok", "o": {"id": [1, 2], "n": {"deep": [true]}}, "big": (0..120).collect::<Vec<_>>()}),
        deeply_nested(200),
    ] {
        let mut plan = plan_of(&document);
        assert!(!patches(&plan).is_empty());
        let mut reconstructor = JsonReconstructor::new();
        while let Some(frame) = plan.next_frame() {
            reconstructor.add_frame(frame);
        }
        reconstructor.process_all_frames().expect("frames apply");
        assert_eq!(reconstructor.current_state(), &document);
    }
}
