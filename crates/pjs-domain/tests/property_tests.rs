//! Property-based tests for domain invariants
//!
//! Uses proptest to verify domain invariant preservation
//! across arbitrary inputs. These tests ensure that domain
//! objects maintain their invariants regardless of input.

use pjson_rs_domain::value_objects::{JsonPath, Priority, SessionId, StreamId};
use proptest::prelude::*;

proptest! {
    /// Priority values are always in valid range (1-255)
    #[test]
    fn priority_valid_range(value in 1u8..=255) {
        let priority = Priority::new(value).expect("Valid priority value should succeed");
        prop_assert_eq!(priority.value(), value);
    }

    /// Priority zero is always rejected
    #[test]
    fn priority_rejects_zero(_dummy in Just(0u8)) {
        prop_assert!(Priority::new(0).is_err());
    }

    /// Priority arithmetic saturates correctly
    #[test]
    fn priority_increase_saturates(base in 1u8..=255, delta in 0u8..=255) {
        let priority = Priority::new(base).unwrap();
        let increased = priority.increase_by(delta);
        let expected = base.saturating_add(delta).max(1);
        prop_assert_eq!(increased.value(), expected);
    }

    /// Priority decrease saturates at 1 (never zero)
    #[test]
    fn priority_decrease_saturates(base in 1u8..=255, delta in 0u8..=255) {
        let priority = Priority::new(base).unwrap();
        let decreased = priority.decrease_by(delta);
        prop_assert!(decreased.value() >= 1);
    }

    /// Priority ordering is transitive
    #[test]
    fn priority_ordering_transitive(a in 1u8..=255, b in 1u8..=255, c in 1u8..=255) {
        let pa = Priority::new(a).unwrap();
        let pb = Priority::new(b).unwrap();
        let pc = Priority::new(c).unwrap();

        if pa <= pb && pb <= pc {
            prop_assert!(pa <= pc);
        }
    }

    /// SessionId roundtrip through string preserves value
    #[test]
    fn session_id_string_roundtrip(_seed in any::<u64>()) {
        let id = SessionId::new();
        let string = id.as_str();
        let parsed = SessionId::from_string(&string).unwrap();
        prop_assert_eq!(id, parsed);
    }

    /// SessionId from_uuid roundtrip preserves value
    #[test]
    fn session_id_uuid_roundtrip(_seed in any::<u64>()) {
        let id = SessionId::new();
        let uuid = id.as_uuid();
        let roundtrip = SessionId::from_uuid(uuid);
        prop_assert_eq!(id, roundtrip);
    }

    /// StreamId roundtrip through string preserves value
    #[test]
    fn stream_id_string_roundtrip(_seed in any::<u64>()) {
        let id = StreamId::new();
        let string = id.as_str();
        let parsed = StreamId::from_string(&string).unwrap();
        prop_assert_eq!(id, parsed);
    }

    /// Valid JSON paths accept alphanumeric keys
    #[test]
    fn json_path_valid_keys(key in "[a-zA-Z][a-zA-Z0-9_]{0,30}") {
        let path = JsonPath::root().append_key(&key);
        prop_assert!(path.is_ok());
    }

    /// JSON path depth calculation is consistent
    #[test]
    fn json_path_depth_consistent(depth in 1usize..10) {
        let mut path = JsonPath::root();
        for i in 0..depth {
            path = path.append_key(&format!("key{i}")).unwrap();
        }
        prop_assert_eq!(path.depth(), depth);
    }

    /// JSON path parent relationship is consistent
    #[test]
    fn json_path_parent_child_relationship(depth in 2usize..10) {
        let mut path = JsonPath::root();
        for i in 0..depth {
            path = path.append_key(&format!("key{i}")).unwrap();
        }

        let parent = path.parent().expect("Non-root path should have parent");
        prop_assert!(parent.is_prefix_of(&path));
    }

    /// JSON path array indices are always valid
    #[test]
    fn json_path_array_index_valid(index in 0usize..10000) {
        let path = JsonPath::root().append_index(index);
        let expected = format!("$[{index}]");
        prop_assert_eq!(path.as_str(), expected);
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;

    #[test]
    fn priority_constants_maintain_ordering() {
        assert!(Priority::CRITICAL > Priority::HIGH);
        assert!(Priority::HIGH > Priority::MEDIUM);
        assert!(Priority::MEDIUM > Priority::LOW);
        assert!(Priority::LOW > Priority::BACKGROUND);
    }

    #[test]
    fn json_path_root_has_no_parent() {
        assert!(JsonPath::root().parent().is_none());
    }

    #[test]
    fn json_path_root_depth_is_zero() {
        assert_eq!(JsonPath::root().depth(), 0);
    }
}
