//! Acceptance tests for #757: `JsonPath::to_json_pointer` escapes `~` as `~0`
//! and `/` as `~1` in keys (RFC 6901 section 3), so a pointer decodes back to
//! the exact key sequence it came from.

use pjson_rs_domain::value_objects::{JsonPath, PathSegment};
use proptest::prelude::*;

fn keys(keys: &[&str]) -> JsonPath {
    JsonPath::from_segments(keys.iter().map(|k| PathSegment::Key((*k).to_owned())))
}

#[test]
fn to_json_pointer_escapes_tilde_and_slash_in_keys() {
    let rows = [
        (keys(&["a/b"]), "/a~1b"),
        (keys(&["m~n"]), "/m~0n"),
        (keys(&["~1"]), "/~01"),
        (keys(&["~0"]), "/~00"),
        (keys(&["/"]), "/~1"),
        (keys(&["a/b", "c~d"]), "/a~1b/c~0d"),
    ];
    for (path, expected) in rows {
        assert_eq!(path.to_json_pointer(), expected, "path {path}");
    }
}

#[test]
fn to_json_pointer_keeps_plain_keys_indices_root_and_empty_key() {
    assert_eq!(keys(&["a", "b"]).to_json_pointer(), "/a/b");
    assert_eq!(JsonPath::root().to_json_pointer(), "");
    assert_eq!(keys(&[""]).to_json_pointer(), "/");
    let mixed = JsonPath::from_segments([
        PathSegment::Key("users".to_owned()),
        PathSegment::Index(10),
        PathSegment::Key("na/me".to_owned()),
    ]);
    assert_eq!(mixed.to_json_pointer(), "/users/10/na~1me");
}

#[test]
fn to_json_pointer_distinguishes_a_slash_key_from_two_keys() {
    assert_ne!(
        keys(&["a/b"]).to_json_pointer(),
        keys(&["a", "b"]).to_json_pointer()
    );
}

/// RFC 6901 section 4: split on `/`, then `~1` -> `/` before `~0` -> `~`.
fn decode_pointer(pointer: &str) -> Vec<String> {
    match pointer.strip_prefix('/') {
        None => {
            assert!(pointer.is_empty(), "pointer must be empty or start with /");
            Vec::new()
        }
        Some(rest) => rest
            .split('/')
            .map(|token| {
                let mut out = String::new();
                let mut chars = token.chars();
                while let Some(c) = chars.next() {
                    if c == '~' {
                        match chars.next() {
                            Some('0') => out.push('~'),
                            Some('1') => out.push('/'),
                            other => panic!("invalid escape ~{other:?} in {pointer:?}"),
                        }
                    } else {
                        out.push(c);
                    }
                }
                out
            })
            .collect(),
    }
}

proptest! {
    #[test]
    fn to_json_pointer_decodes_back_to_the_keys(
        ks in proptest::collection::vec("[a~/0-9]{0,4}", 0..4),
    ) {
        let path = JsonPath::from_segments(ks.iter().cloned().map(PathSegment::Key));
        prop_assert_eq!(decode_pointer(&path.to_json_pointer()), ks);
    }

    #[test]
    fn to_json_pointer_is_injective_over_key_paths(
        a in proptest::collection::vec("[a/]{0,2}", 0..3),
        b in proptest::collection::vec("[a/]{0,2}", 0..3),
    ) {
        let pa = JsonPath::from_segments(a.iter().cloned().map(PathSegment::Key));
        let pb = JsonPath::from_segments(b.iter().cloned().map(PathSegment::Key));
        prop_assert_eq!(a == b, pa.to_json_pointer() == pb.to_json_pointer());
    }
}
