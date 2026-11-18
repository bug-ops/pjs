//! Acceptance tests for #751: `JsonPatch` wraps the wire vocabulary
//! (`WirePatch`) and `JsonReconstructor` applies every op through the shared
//! core: no depth limit (spec 031), per-patch atomicity, and `Merge` /
//! `AppendStr` support with the WebSocket semantics.

use pjson_rs::Error;
use pjson_rs::domain::value_objects::{JsonPath, PathSegment, Priority};
use pjson_rs::error::PatchError;
use pjson_rs::stream::JsonReconstructor;
use pjson_rs::stream::patch::WirePatch;
use pjson_rs::stream::priority::{JsonPatch, PriorityStreamFrame};
use pjson_rs::stream::reconstruction::ProcessResult;
use serde_json::{Map, Value, json};

fn path(text: &str) -> JsonPath {
    JsonPath::new(text).expect("valid path")
}

fn entries(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

fn patch(wire: WirePatch) -> JsonPatch {
    JsonPatch {
        patch: wire,
        priority: Priority::HIGH,
    }
}

fn reconstructor_with(skeleton: Value) -> JsonReconstructor {
    let mut reconstructor = JsonReconstructor::new();
    reconstructor.add_frame(PriorityStreamFrame::Skeleton {
        data: skeleton,
        priority: Priority::CRITICAL,
        complete: false,
    });
    reconstructor
        .process_all_frames()
        .expect("skeleton applies");
    reconstructor
}

fn apply(
    reconstructor: &mut JsonReconstructor,
    patches: Vec<JsonPatch>,
) -> Result<Vec<ProcessResult>, Error> {
    reconstructor.add_frame(PriorityStreamFrame::Patch {
        patches,
        priority: Priority::HIGH,
    });
    reconstructor.process_all_frames()
}

#[test]
fn json_patch_serializes_the_wire_patch_and_priority() {
    let encoded = serde_json::to_value(patch(WirePatch::Set {
        path: path("$.a"),
        value: json!(1),
    }))
    .unwrap();
    assert_eq!(encoded, json!({"patch": ["set", ["a"], 1], "priority": 80}));
}

#[test]
fn reconstructor_applies_each_wire_op() {
    let mut reconstructor =
        reconstructor_with(json!({"a": null, "list": [], "text": "ab", "obj": {"x": 1}}));

    apply(
        &mut reconstructor,
        vec![
            patch(WirePatch::Set {
                path: path("$.a"),
                value: json!(7),
            }),
            patch(WirePatch::Append {
                path: path("$.list"),
                values: vec![json!(1), json!(2)],
            }),
            patch(WirePatch::AppendStr {
                path: path("$.text"),
                text: "cd".to_owned(),
            }),
            patch(WirePatch::Merge {
                path: path("$.obj"),
                entries: entries(json!({"x": 2, "y": 3})),
            }),
        ],
    )
    .expect("patches apply");

    assert_eq!(
        reconstructor.current_state(),
        &json!({"a": 7, "list": [1, 2], "text": "abcd", "obj": {"x": 2, "y": 3}})
    );
}

#[test]
fn reconstructor_reports_applied_paths_in_order_without_loss() {
    let mut reconstructor = reconstructor_with(json!({"a": null, "b": []}));

    let results = apply(
        &mut reconstructor,
        vec![
            patch(WirePatch::Set {
                path: path("$.a"),
                value: json!(1),
            }),
            patch(WirePatch::Append {
                path: path("$.b"),
                values: vec![json!(1)],
            }),
        ],
    )
    .expect("patches apply");

    let [
        ProcessResult::PatchesApplied {
            count,
            paths,
            priority,
        },
    ] = results.as_slice()
    else {
        panic!("expected one PatchesApplied, got {results:?}");
    };
    assert_eq!(*count, 2);
    assert_eq!(*priority, Priority::HIGH);
    assert_eq!(paths, &[path("$.a"), path("$.b")]);
    assert_eq!(reconstructor.stats().patches_applied, 2);
}

#[test]
fn reconstructor_has_no_depth_limit() {
    const DEPTH: usize = 200;
    let mut document = json!({"leaf": 0});
    for _ in 0..DEPTH {
        document = json!({"k": document});
    }
    let mut reconstructor = reconstructor_with(Value::Null);
    let mut segments = vec![PathSegment::Key("k".to_owned()); DEPTH];
    segments.push(PathSegment::Key("leaf".to_owned()));

    apply(
        &mut reconstructor,
        vec![
            patch(WirePatch::Set {
                path: JsonPath::root(),
                value: document,
            }),
            patch(WirePatch::Set {
                path: JsonPath::from_segments(segments),
                value: json!(7),
            }),
        ],
    )
    .expect("a path deeper than the websocket limit applies");

    let mut node = reconstructor.current_state();
    for _ in 0..DEPTH {
        node = &node["k"];
    }
    assert_eq!(node, &json!({"leaf": 7}));
}

#[test]
fn reconstructor_reports_each_kind_mismatch_and_leaves_the_state_unchanged() {
    let initial = json!({"num": 1, "obj": {"x": 1}, "list": [1]});
    let rows: Vec<(&str, WirePatch, PatchError)> = vec![
        (
            "append to a non-array",
            WirePatch::Append {
                path: path("$.num"),
                values: vec![json!(1)],
            },
            PatchError::ExpectedArray,
        ),
        (
            "merge into a non-object",
            WirePatch::Merge {
                path: path("$.list"),
                entries: entries(json!({"y": 1})),
            },
            PatchError::ExpectedObject,
        ),
        (
            "append_str to a non-string",
            WirePatch::AppendStr {
                path: path("$.num"),
                text: "x".to_owned(),
            },
            PatchError::ExpectedString,
        ),
        (
            "set below a missing key",
            WirePatch::Set {
                path: path("$.missing.child"),
                value: json!(1),
            },
            PatchError::KeyNotFound("missing".to_owned()),
        ),
    ];
    for (label, wire, expected) in rows {
        let mut reconstructor = reconstructor_with(initial.clone());
        let result = apply(&mut reconstructor, vec![patch(wire)]);
        match result {
            Err(Error::Patch(error)) => assert_eq!(error, expected, "{label}"),
            other => panic!("{label}: expected Error::Patch, got {other:?}"),
        }
        assert_eq!(reconstructor.current_state(), &initial, "{label}");
    }
}

#[test]
fn reconstructor_keeps_the_patches_applied_before_a_failing_one() {
    let mut reconstructor = reconstructor_with(json!({"a": 0, "num": 1}));

    let result = apply(
        &mut reconstructor,
        vec![
            patch(WirePatch::Set {
                path: path("$.a"),
                value: json!(5),
            }),
            patch(WirePatch::Append {
                path: path("$.num"),
                values: vec![json!(1)],
            }),
        ],
    );

    assert!(matches!(
        result,
        Err(Error::Patch(PatchError::ExpectedArray))
    ));
    assert_eq!(reconstructor.current_state(), &json!({"a": 5, "num": 1}));
}
