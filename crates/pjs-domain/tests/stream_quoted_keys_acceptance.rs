//! Acceptance tests for #698: the domain `Stream` addresses keys containing
//! `.`, `[`, `]` or the empty key with ordinary quoted-path `Set` patches
//! instead of a root `Set` or a verbatim skeleton subtree.

use std::collections::BTreeSet;

use pjson_rs_domain::Stream;
use pjson_rs_domain::entities::stream::StreamConfig;
use pjson_rs_domain::value_objects::{JsonData, Priority, SessionId};
use serde_json::{Value, json};

fn streaming(data: &Value) -> Stream {
    let mut stream = Stream::new(
        SessionId::new(),
        JsonData::from(data.clone()),
        StreamConfig::default(),
    );
    stream.start_streaming().expect("fresh stream starts");
    stream
}

#[test]
fn extract_prioritized_patches_addresses_unencodable_keys_without_a_root_set() {
    let stream = streaming(&json!({"a.b": 1, "": 2, "x": {"k]": 3}, "ok": [4]}));

    let patches = stream
        .extract_prioritized_patches(Priority::BACKGROUND)
        .expect("patches");

    let paths: BTreeSet<String> = patches.iter().map(|(p, _)| p.path().to_string()).collect();
    let expected: BTreeSet<String> = [r#"$["a.b"]"#, r#"$[""]"#, r#"$.x["k]"]"#, "$.ok"]
        .map(str::to_owned)
        .into();
    assert_eq!(paths, expected);
}

#[test]
fn create_skeleton_frame_does_not_ship_unencodable_subtrees_verbatim() {
    let mut stream = streaming(&json!({"a.b": {"c": 7}, "x": {"a[0]": 8}}));

    let skeleton = stream.create_skeleton_frame().expect("skeleton");
    let payload = serde_json::to_value(skeleton.body()).expect("serializable");

    assert_ne!(payload["a.b"]["c"], json!(7), "{payload}");
    assert_ne!(payload["x"]["a[0]"], json!(8), "{payload}");
    assert!(
        payload["a.b"].is_object() && payload["x"].is_object(),
        "{payload}"
    );
}
