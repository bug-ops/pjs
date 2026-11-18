//! Acceptance tests for #713 (strict index grammar) and #698 (a `JsonPath` can
//! address every JSON key through a quoted segment).
//!
//! Grammar: `path = "$" *( "." plain / "[" index "]" / "[" quoted "]" )` with
//! `index = "0" / [1-9][0-9]*`, `plain` a non-empty key without `.`, `[`, `]`,
//! and `quoted` the canonical `serde_json` string literal of a key that is not
//! plain.

use pjson_rs_domain::value_objects::{JsonPath, PathSegment};
use proptest::prelude::*;

fn parse(text: &str) -> Option<JsonPath> {
    text.parse::<JsonPath>().ok()
}

fn path_of(segments: impl IntoIterator<Item = PathSegment>) -> JsonPath {
    JsonPath::from_segments(segments)
}

#[test]
fn from_str_rejects_non_canonical_indices() {
    let rejected = [
        "$[0",
        "$.a[0",
        "$[+5]",
        "$[007]",
        "$[00]",
        "$[-1]",
        "$[]",
        "$[1a]",
        "$[18446744073709551616]",
    ];
    for text in rejected {
        assert!(parse(text).is_none(), "{text} must be rejected");
    }
    for text in ["$[0]", "$[10]", "$.a[0].b", "$[0][1]"] {
        assert_eq!(parse(text).map(|p| p.to_string()).as_deref(), Some(text));
    }
}

#[test]
fn from_str_accepts_only_canonical_quoted_keys() {
    let accepted = [
        (r#"$["a.b"]"#, "a.b"),
        (r#"$[""]"#, ""),
        (r#"$["x]"]"#, "x]"),
        (r#"$["[0]"]"#, "[0]"),
        (r#"$["a\"b.c"]"#, "a\"b.c"),
        (r#"$["a\\b.c"]"#, "a\\b.c"),
        (r#"$["a\nb.c"]"#, "a\nb.c"),
        (r#"$["\u0000.a"]"#, "\u{0}.a"),
        ("$[\"ключ.x\"]", "ключ.x"),
    ];
    for (text, key) in accepted {
        let path = parse(text).unwrap_or_else(|| panic!("{text} must parse"));
        assert_eq!(
            path.segments(),
            [PathSegment::Key(key.to_owned())],
            "{text}"
        );
        assert_eq!(path.to_string(), text);
    }

    let rejected = [
        r#"$["a"]"#,
        r#"$["a.b""#,
        r#"$["a.b" ]"#,
        r#"$[ "a.b"]"#,
        r#"$['a.b']"#,
        r#"$["A.b"]"#,
        r#"$["a\/b.c"]"#,
        r#"$["\ud800"]"#,
        r#"$["a.b"]x"#,
        r#"$["a.b""c"]"#,
    ];
    for text in rejected {
        assert!(parse(text).is_none(), "{text} must be rejected");
    }
}

#[test]
fn append_key_addresses_any_key_and_renders_plain_keys_unchanged() {
    let root = JsonPath::root();
    assert_eq!(root.append_key("a").to_string(), "$.a");
    assert_eq!(root.append_key("a").append_index(0).to_string(), "$.a[0]");
    assert_eq!(root.append_key("a.b").to_string(), r#"$["a.b"]"#);
    assert_eq!(root.append_key("").to_string(), r#"$[""]"#);
    assert_eq!(root.append_key("[").to_string(), r#"$["["]"#);
    assert_eq!(root.append_key("]").to_string(), r#"$["]"]"#);
    assert_eq!(root.append_key("a\"b").to_string(), "$.a\"b");
    assert_eq!(
        root.append_key("x")
            .append_key("a.b")
            .append_index(2)
            .to_string(),
        r#"$.x["a.b"][2]"#
    );
}

#[test]
fn from_segments_accepts_any_key_sequence() {
    let segments = vec![
        PathSegment::Key(String::new()),
        PathSegment::Key("a.b".to_owned()),
        PathSegment::Index(3),
        PathSegment::Key("]".to_owned()),
    ];
    let path = path_of(segments.clone());
    assert_eq!(path.segments(), segments.as_slice());
    assert_eq!(parse(&path.to_string()), Some(path));
}

#[test]
fn path_segment_is_a_closed_key_or_index_enum() {
    let label = |segment: &PathSegment| match segment {
        PathSegment::Key(_) => "key",
        PathSegment::Index(_) => "index",
    };
    assert_eq!(label(&PathSegment::Key("k".to_owned())), "key");
    assert_eq!(label(&PathSegment::Index(0)), "index");
}

#[test]
fn json_path_serde_text_is_the_rendered_form() {
    let plain = JsonPath::root()
        .append_key("a")
        .append_index(0)
        .append_key("b");
    assert_eq!(serde_json::to_string(&plain).unwrap(), r#""$.a[0].b""#);

    let quoted = JsonPath::root().append_key("a.b").append_key("");
    let wire = serde_json::to_string(&quoted).unwrap();
    assert_eq!(wire, r#""$[\"a.b\"][\"\"]""#);
    assert_eq!(serde_json::from_str::<JsonPath>(&wire).ok(), Some(quoted));
}

#[test]
fn to_json_pointer_renders_root_as_empty_string_and_empty_key_as_slash() {
    assert_eq!(JsonPath::root().to_json_pointer(), "");
    assert_eq!(JsonPath::root().append_key("").to_json_pointer(), "/");
    assert_eq!(
        JsonPath::new("$.users[0].name").unwrap().to_json_pointer(),
        "/users/0/name"
    );
}

#[test]
fn from_str_accepted_text_always_re_renders_identically() {
    let alphabet = ['.', '[', ']', '0', '1', 'a', '+', '"'];
    let mut level = vec![String::new()];
    for _ in 0..=4 {
        for tail in &level {
            let text = format!("${tail}");
            if let Some(path) = parse(&text) {
                assert_eq!(path.to_string(), text, "{text} re-renders differently");
            }
        }
        level = level
            .iter()
            .flat_map(|tail| alphabet.iter().map(move |c| format!("{tail}{c}")))
            .collect();
    }
}

fn segment() -> impl Strategy<Value = PathSegment> {
    prop_oneof![
        any::<String>().prop_map(PathSegment::Key),
        "[a-z.\\[\\]\"\\\\ ]{0,6}".prop_map(PathSegment::Key),
        any::<usize>().prop_map(PathSegment::Index),
    ]
}

proptest! {
    #[test]
    fn display_then_from_str_round_trips_every_path(
        segments in proptest::collection::vec(segment(), 0..6)
    ) {
        let path = path_of(segments);
        let text = path.to_string();
        prop_assert_eq!(parse(&text), Some(path));
    }
}
