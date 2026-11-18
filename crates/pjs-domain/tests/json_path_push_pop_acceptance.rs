//! Acceptance tests for #770 (track A): `JsonPath::push` and `JsonPath::pop`
//! are public, so `PriorityStreamer` in `pjs-core` can walk a document on one
//! path buffer.

use pjson_rs_domain::value_objects::{JsonPath, PathSegment};

#[test]
fn push_appends_segments_in_place_and_matches_the_append_builders() {
    let mut path = JsonPath::root();

    path.push(PathSegment::Key("a".to_owned()));
    path.push(PathSegment::Index(2));
    path.push(PathSegment::Key("b.c".to_owned()));

    let built = JsonPath::root()
        .append_key("a")
        .append_index(2)
        .append_key("b.c");
    assert_eq!(path, built);
    assert_eq!(path.depth(), 3);
}

#[test]
fn pop_returns_the_last_pushed_segment_and_none_at_the_root() {
    let mut path = JsonPath::root().append_key("a").append_index(1);

    assert_eq!(path.pop(), Some(PathSegment::Index(1)));
    assert_eq!(path.pop(), Some(PathSegment::Key("a".to_owned())));
    assert_eq!(path.pop(), None);
    assert_eq!(path, JsonPath::root());
}

#[test]
fn push_then_pop_restores_the_original_path() {
    let original = JsonPath::new("$.a.b").expect("valid path");
    let mut path = original.clone();

    path.push(PathSegment::Key("c".to_owned()));
    let popped = path.pop();

    assert_eq!(popped, Some(PathSegment::Key("c".to_owned())));
    assert_eq!(path, original);
}
