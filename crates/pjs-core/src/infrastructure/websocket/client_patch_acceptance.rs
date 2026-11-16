//! Acceptance tests for #659: patches are consumed by value.
//!
//! `apply_patch` and `ClientStreamSession::apply_patches` are crate-private,
//! so this file is a test-only child of the `websocket` module (wired in
//! `mod.rs`). It needs `ClientStreamSession`, `new`, `apply_patches` and the
//! `reconstructed_*` fields to be `pub(super)`.

use super::client::ClientStreamSession;
use super::protocol::{WirePatch, apply_patch};
use crate::domain::SessionId;
use crate::error::{Error, LimitKind, PatchError};
use core::assert_matches;
use serde_json::{Value, json};

fn patch(text: &str) -> WirePatch {
    serde_json::from_str(text).expect("valid wire patch")
}

fn session() -> ClientStreamSession {
    ClientStreamSession::new(SessionId::new())
}

fn serialized_len(value: &Value) -> usize {
    serde_json::to_vec(value).expect("serializable").len()
}

#[test]
fn test_apply_patch_by_value_applies_every_operation() {
    let mut state = json!({"s": "ab", "items": [1], "obj": {"k": 1}});

    apply_patch(&mut state, patch(r#"["set", ["s2"], "new"]"#)).unwrap();
    apply_patch(&mut state, patch(r#"["merge", ["obj"], {"k": 2, "j": 3}]"#)).unwrap();
    apply_patch(&mut state, patch(r#"["append", ["items"], [2, 3]]"#)).unwrap();
    apply_patch(&mut state, patch(r#"["append_str", ["s"], "cd"]"#)).unwrap();

    assert_eq!(
        state,
        json!({"s": "abcd", "s2": "new", "items": [1, 2, 3], "obj": {"k": 2, "j": 3}})
    );
}

#[test]
fn test_apply_patch_by_value_root_set_replaces_state() {
    let mut state = json!({"stale": true});

    apply_patch(
        &mut state,
        patch(r#"["set", [], {"fresh": [1, {"deep": null}]}]"#),
    )
    .unwrap();

    assert_eq!(state, json!({"fresh": [1, {"deep": null}]}));
}

#[test]
fn test_apply_patch_by_value_set_at_array_index_replaces_element() {
    let mut state = json!({"xs": [1, 2, 3]});

    apply_patch(&mut state, patch(r#"["set", ["xs", 1], {"n": 9}]"#)).unwrap();

    assert_eq!(state, json!({"xs": [1, {"n": 9}, 3]}));
}

#[test]
fn test_apply_patch_by_value_reports_type_and_resolution_errors() {
    let mut state = json!({"s": "x", "n": 1, "xs": [1]});

    assert_matches!(
        apply_patch(&mut state, patch(r#"["set", ["missing", "k"], 1]"#)),
        Err(PatchError::KeyNotFound(_))
    );
    assert_matches!(
        apply_patch(&mut state, patch(r#"["set", ["xs", 5], 1]"#)),
        Err(PatchError::IndexOutOfBounds { index: 5, len: 1 })
    );
    assert_matches!(
        apply_patch(&mut state, patch(r#"["merge", ["n"], {"a": 1}]"#)),
        Err(PatchError::ExpectedObject)
    );
    assert_matches!(
        apply_patch(&mut state, patch(r#"["append", ["n"], [1]]"#)),
        Err(PatchError::ExpectedArray)
    );
    assert_matches!(
        apply_patch(&mut state, patch(r#"["append_str", ["n"], "t"]"#)),
        Err(PatchError::ExpectedString)
    );
}

#[test]
fn test_apply_patches_by_value_applies_in_order_and_tracks_bytes() {
    let mut session = session();

    session
        .apply_patches(
            vec![
                patch(r#"["set", ["a"], {"x": [1, 2]}]"#),
                patch(r#"["merge", ["a"], {"y": "text"}]"#),
                patch(r#"["append", ["a", "x"], [3]]"#),
                patch(r#"["append_str", ["a", "y"], "!"]"#),
            ],
            usize::MAX,
        )
        .unwrap();

    let expected = json!({"a": {"x": [1, 2, 3], "y": "text!"}});
    assert_eq!(session.reconstructed_data, expected);
    assert!(
        session.reconstructed_bytes >= serialized_len(&expected),
        "tracked bytes must never undercount the retained state"
    );
}

#[test]
fn test_apply_patches_by_value_empty_vec_is_a_no_op() {
    let mut session = session();
    let before = session.reconstructed_bytes;

    session.apply_patches(Vec::new(), usize::MAX).unwrap();

    assert_eq!(session.reconstructed_data, json!({}));
    assert_eq!(session.reconstructed_bytes, before);
}

#[test]
fn test_apply_patches_by_value_limit_exceeded_leaves_state_unchanged() {
    let mut session = session();
    session
        .apply_patches(vec![patch(r#"["set", ["a"], 1]"#)], usize::MAX)
        .unwrap();
    let data_before = session.reconstructed_data.clone();
    let bytes_before = session.reconstructed_bytes;

    let result = session.apply_patches(
        vec![patch(
            r#"["set", ["big"], "0123456789012345678901234567890123456789"]"#,
        )],
        bytes_before + 5,
    );

    assert_matches!(
        result,
        Err(Error::LimitExceeded {
            kind: LimitKind::SessionBytes,
            ..
        })
    );
    assert_eq!(session.reconstructed_data, data_before);
    assert_eq!(session.reconstructed_bytes, bytes_before);
}

#[test]
fn test_apply_patches_by_value_stops_at_first_failing_patch() {
    let mut session = session();

    let result = session.apply_patches(
        vec![
            patch(r#"["set", ["a"], 1]"#),
            patch(r#"["set", ["nope", "k"], 2]"#),
            patch(r#"["set", ["never"], 3]"#),
        ],
        usize::MAX,
    );

    assert_matches!(result, Err(Error::Patch(PatchError::KeyNotFound(_))));
    assert_eq!(session.reconstructed_data, json!({"a": 1}));
}
