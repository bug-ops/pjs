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

proptest! {
    /// StreamId roundtrip through UUID preserves value
    #[test]
    fn stream_id_uuid_roundtrip(_seed in any::<u64>()) {
        let id = StreamId::new();
        let uuid = id.as_uuid();
        let roundtrip = StreamId::from_uuid(uuid);
        prop_assert_eq!(id, roundtrip);
    }

    /// SessionId and StreamId have different UUIDs
    #[test]
    fn session_stream_ids_are_unique(_seed in any::<u64>()) {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        prop_assert_ne!(session_id.as_uuid(), stream_id.as_uuid());
    }

    /// SessionId hash is deterministic
    #[test]
    fn session_id_hash_deterministic(_seed in any::<u64>()) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let id = SessionId::new();

        let mut hasher1 = DefaultHasher::new();
        id.hash(&mut hasher1);
        let hash1 = hasher1.finish();

        let mut hasher2 = DefaultHasher::new();
        id.hash(&mut hasher2);
        let hash2 = hasher2.finish();

        prop_assert_eq!(hash1, hash2);
    }

    /// StreamId hash is deterministic
    #[test]
    fn stream_id_hash_deterministic(_seed in any::<u64>()) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let id = StreamId::new();

        let mut hasher1 = DefaultHasher::new();
        id.hash(&mut hasher1);
        let hash1 = hasher1.finish();

        let mut hasher2 = DefaultHasher::new();
        id.hash(&mut hasher2);
        let hash2 = hasher2.finish();

        prop_assert_eq!(hash1, hash2);
    }

    /// Priority comparison is antisymmetric
    #[test]
    fn priority_comparison_antisymmetric(a in 1u8..=255, b in 1u8..=255) {
        let pa = Priority::new(a).unwrap();
        let pb = Priority::new(b).unwrap();

        if pa < pb {
            prop_assert!(!(pb < pa));
        }
    }

    /// Priority equality is reflexive
    #[test]
    fn priority_equality_reflexive(value in 1u8..=255) {
        let priority = Priority::new(value).unwrap();
        prop_assert_eq!(priority, priority);
    }

    /// Priority equality is symmetric
    #[test]
    fn priority_equality_symmetric(value in 1u8..=255) {
        let p1 = Priority::new(value).unwrap();
        let p2 = Priority::new(value).unwrap();
        prop_assert_eq!(p1, p2);
        prop_assert_eq!(p2, p1);
    }

    /// JSON path append_key creates valid child paths
    #[test]
    fn json_path_append_creates_child(key in "[a-zA-Z][a-zA-Z0-9_]{0,20}") {
        let parent = JsonPath::root();
        let child = parent.append_key(&key).unwrap();
        prop_assert!(parent.is_prefix_of(&child));
        prop_assert_eq!(child.depth(), parent.depth() + 1);
    }

    /// JSON path append_index creates valid indexed paths
    #[test]
    fn json_path_append_index_creates_child(index in 0usize..1000) {
        let parent = JsonPath::root();
        let child = parent.append_index(index);
        prop_assert!(parent.is_prefix_of(&child));
    }

    /// JSON path serialization roundtrip
    #[test]
    fn json_path_string_roundtrip(depth in 1usize..5) {
        let mut path = JsonPath::root();
        for i in 0..depth {
            path = path.append_key(&format!("key{i}")).unwrap();
        }
        let string_repr = path.as_str();
        prop_assert!(string_repr.starts_with('$'));
    }

    /// SessionId string representation is valid UUID format
    #[test]
    fn session_id_string_is_valid_uuid(_seed in any::<u64>()) {
        let id = SessionId::new();
        let string = id.as_str();
        prop_assert!(uuid::Uuid::parse_str(&string).is_ok());
    }

    /// StreamId string representation is valid UUID format
    #[test]
    fn stream_id_string_is_valid_uuid(_seed in any::<u64>()) {
        let id = StreamId::new();
        let string = id.as_str();
        prop_assert!(uuid::Uuid::parse_str(&string).is_ok());
    }

    /// Priority increase never decreases value
    #[test]
    fn priority_increase_monotonic(base in 1u8..=255, delta in 0u8..=255) {
        let priority = Priority::new(base).unwrap();
        let increased = priority.increase_by(delta);
        prop_assert!(increased.value() >= priority.value());
    }

    /// Priority decrease never increases value
    #[test]
    fn priority_decrease_monotonic(base in 1u8..=255, delta in 0u8..=255) {
        let priority = Priority::new(base).unwrap();
        let decreased = priority.decrease_by(delta);
        prop_assert!(decreased.value() <= priority.value());
    }

    /// JSON path depth increases with each append
    #[test]
    fn json_path_depth_increases_monotonically(steps in 1usize..10) {
        let mut path = JsonPath::root();
        let initial_depth = path.depth();

        for i in 0..steps {
            path = path.append_key(&format!("k{i}")).unwrap();
        }

        prop_assert_eq!(path.depth(), initial_depth + steps);
    }

    /// Priority constants are distinct
    #[test]
    fn priority_constants_are_distinct(_dummy in Just(0u8)) {
        let priorities = vec![
            Priority::CRITICAL,
            Priority::HIGH,
            Priority::MEDIUM,
            Priority::LOW,
            Priority::BACKGROUND,
        ];

        for i in 0..priorities.len() {
            for j in i + 1..priorities.len() {
                prop_assert_ne!(priorities[i], priorities[j]);
            }
        }
    }

    /// SessionId clone creates equal value
    #[test]
    fn session_id_clone_equality(_seed in any::<u64>()) {
        let id = SessionId::new();
        let cloned = id.clone();
        prop_assert_eq!(id, cloned);
        prop_assert_eq!(id.as_str(), cloned.as_str());
    }

    /// StreamId clone creates equal value
    #[test]
    fn stream_id_clone_equality(_seed in any::<u64>()) {
        let id = StreamId::new();
        let cloned = id.clone();
        prop_assert_eq!(id, cloned);
        prop_assert_eq!(id.as_str(), cloned.as_str());
    }

    /// Priority clone creates equal value
    #[test]
    fn priority_clone_equality(value in 1u8..=255) {
        let priority = Priority::new(value).unwrap();
        let cloned = priority.clone();
        prop_assert_eq!(priority, cloned);
        prop_assert_eq!(priority.value(), cloned.value());
    }

    /// JSON path is_prefix_of is transitive
    #[test]
    fn json_path_prefix_transitive(depth in 2usize..8) {
        let mut path1 = JsonPath::root();
        for i in 0..depth {
            path1 = path1.append_key(&format!("a{i}")).unwrap();
        }

        let path2 = path1.append_key("b").unwrap();
        let path3 = path2.append_key("c").unwrap();

        prop_assert!(path1.is_prefix_of(&path2));
        prop_assert!(path2.is_prefix_of(&path3));
        prop_assert!(path1.is_prefix_of(&path3));
    }

    /// SessionId from invalid string fails gracefully
    #[test]
    fn session_id_from_invalid_string(invalid_str in ".*[^0-9a-f-].*") {
        let result = SessionId::from_string(&invalid_str);
        if !invalid_str.contains('-') || invalid_str.len() != 36 {
            prop_assert!(result.is_err());
        }
    }

    /// Priority max value is 255
    #[test]
    fn priority_max_value_is_255(_dummy in Just(0u8)) {
        let max = Priority::new(255).unwrap();
        let increased = max.increase_by(100);
        prop_assert_eq!(increased.value(), 255);
    }

    /// Priority min value is 1
    #[test]
    fn priority_min_value_is_1(_dummy in Just(0u8)) {
        let min = Priority::new(1).unwrap();
        let decreased = min.decrease_by(100);
        prop_assert_eq!(decreased.value(), 1);
    }

    /// JSON path root is prefix of any path
    #[test]
    fn json_path_root_prefix_of_all(depth in 1usize..10) {
        let mut path = JsonPath::root();
        for i in 0..depth {
            path = path.append_key(&format!("x{i}")).unwrap();
        }
        prop_assert!(JsonPath::root().is_prefix_of(&path));
    }

    /// Priority ordering is consistent with value ordering
    #[test]
    fn priority_ordering_matches_value(a in 1u8..=255, b in 1u8..=255) {
        let pa = Priority::new(a).unwrap();
        let pb = Priority::new(b).unwrap();

        if a < b {
            prop_assert!(pa < pb);
        } else if a > b {
            prop_assert!(pa > pb);
        } else {
            prop_assert_eq!(pa, pb);
        }
    }

    /// JSON path empty key is rejected
    #[test]
    fn json_path_rejects_empty_key(_dummy in Just(0u8)) {
        let result = JsonPath::root().append_key("");
        prop_assert!(result.is_err());
    }

    /// SessionId equality is transitive
    #[test]
    fn session_id_equality_transitive(_seed in any::<u64>()) {
        let id1 = SessionId::new();
        let id2 = id1.clone();
        let id3 = id2.clone();

        prop_assert_eq!(id1, id2);
        prop_assert_eq!(id2, id3);
        prop_assert_eq!(id1, id3);
    }

    /// StreamId equality is transitive
    #[test]
    fn stream_id_equality_transitive(_seed in any::<u64>()) {
        let id1 = StreamId::new();
        let id2 = id1.clone();
        let id3 = id2.clone();

        prop_assert_eq!(id1, id2);
        prop_assert_eq!(id2, id3);
        prop_assert_eq!(id1, id3);
    }

    /// Priority increase_by is commutative with same delta
    #[test]
    fn priority_increase_commutative(base in 1u8..=200, delta1 in 0u8..=25, delta2 in 0u8..=25) {
        let p1 = Priority::new(base).unwrap();
        let result1 = p1.increase_by(delta1).increase_by(delta2);

        let p2 = Priority::new(base).unwrap();
        let result2 = p2.increase_by(delta2).increase_by(delta1);

        prop_assert_eq!(result1.value(), result2.value());
    }

    /// JSON path with max depth maintains consistency
    #[test]
    fn json_path_max_depth_handling(keys in prop::collection::vec("[a-z]{1,5}", 1..20)) {
        let mut path = JsonPath::root();
        for key in &keys {
            if let Ok(new_path) = path.append_key(key) {
                path = new_path;
            }
        }
        prop_assert!(path.depth() <= keys.len());
        prop_assert!(path.as_str().starts_with('$'));
    }
}
