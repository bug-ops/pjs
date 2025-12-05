//! Comprehensive tests for Frame entity
//!
//! Tests cover all frame types (Skeleton, Patch, Complete, Error),
//! frame operations, validation, metadata, and edge cases.

use pjson_rs_domain::entities::frame::{Frame, FramePatch, FrameType, PatchOperation};
use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority, StreamId};
use std::collections::HashMap;

// ============================================================================
// FrameType Tests
// ============================================================================

#[test]
fn test_frame_type_variants() {
    let skeleton = FrameType::Skeleton;
    let patch = FrameType::Patch;
    let complete = FrameType::Complete;
    let error = FrameType::Error;

    assert_eq!(skeleton, FrameType::Skeleton);
    assert_eq!(patch, FrameType::Patch);
    assert_eq!(complete, FrameType::Complete);
    assert_eq!(error, FrameType::Error);
}

#[test]
fn test_frame_type_clone() {
    let original = FrameType::Skeleton;
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

#[test]
fn test_frame_type_debug() {
    let frame_type = FrameType::Patch;
    let debug = format!("{:?}", frame_type);
    assert!(debug.contains("Patch"));
}

#[test]
fn test_frame_type_hash() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let hash_value = |v: &FrameType| {
        let mut hasher = DefaultHasher::new();
        v.hash(&mut hasher);
        hasher.finish()
    };

    assert_eq!(
        hash_value(&FrameType::Skeleton),
        hash_value(&FrameType::Skeleton)
    );
    assert_ne!(
        hash_value(&FrameType::Skeleton),
        hash_value(&FrameType::Patch)
    );
}

// ============================================================================
// Skeleton Frame Tests
// ============================================================================

#[test]
fn test_skeleton_frame_creation() {
    let stream_id = StreamId::new();
    let skeleton_data = JsonData::Object(HashMap::new());
    let frame = Frame::skeleton(stream_id, 1, skeleton_data);

    assert_eq!(frame.stream_id(), stream_id);
    assert_eq!(frame.frame_type(), &FrameType::Skeleton);
    assert_eq!(frame.priority(), Priority::CRITICAL);
    assert_eq!(frame.sequence(), 1);
    assert!(frame.validate().is_ok());
}

#[test]
fn test_skeleton_frame_with_data() {
    let stream_id = StreamId::new();
    let mut skeleton_obj = HashMap::new();
    skeleton_obj.insert("users".to_string(), JsonData::Array(vec![]));
    skeleton_obj.insert("total".to_string(), JsonData::Integer(0));
    let skeleton_data = JsonData::Object(skeleton_obj);

    let frame = Frame::skeleton(stream_id, 1, skeleton_data);

    assert!(frame.payload().is_object());
    assert!(frame.payload().get("users").is_some());
    assert!(frame.payload().get("total").is_some());
}

#[test]
fn test_skeleton_frame_always_critical() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Null);

    assert!(frame.is_critical());
    assert!(frame.is_high_priority());
    assert_eq!(frame.priority(), Priority::CRITICAL);
}

#[test]
fn test_skeleton_frame_validate_success() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Object(HashMap::new()));
    assert!(frame.validate().is_ok());
}

// ============================================================================
// Patch Frame Tests
// ============================================================================

#[test]
fn test_patch_frame_creation() {
    let stream_id = StreamId::new();
    let path = JsonPath::new("$.user.name").unwrap();
    let patch = FramePatch::set(path, JsonData::String("Alice".to_string()));
    let frame = Frame::patch(stream_id, 2, Priority::HIGH, vec![patch]).unwrap();

    assert_eq!(frame.frame_type(), &FrameType::Patch);
    assert_eq!(frame.priority(), Priority::HIGH);
    assert_eq!(frame.sequence(), 2);
    assert!(frame.validate().is_ok());
}

#[test]
fn test_patch_frame_multiple_patches() {
    let stream_id = StreamId::new();
    let patches = vec![
        FramePatch::set(
            JsonPath::new("$.name").unwrap(),
            JsonData::String("Bob".to_string()),
        ),
        FramePatch::set(JsonPath::new("$.age").unwrap(), JsonData::Integer(30)),
        FramePatch::set(JsonPath::new("$.active").unwrap(), JsonData::Bool(true)),
    ];

    let frame = Frame::patch(stream_id, 2, Priority::MEDIUM, patches).unwrap();

    assert!(frame.payload().is_object());
    if let JsonData::Object(obj) = frame.payload() {
        if let Some(JsonData::Array(patches_array)) = obj.get("patches") {
            assert_eq!(patches_array.len(), 3);
        } else {
            panic!("Expected patches array");
        }
    } else {
        panic!("Expected object payload");
    }
}

#[test]
fn test_patch_frame_empty_patches_error() {
    let stream_id = StreamId::new();
    let result = Frame::patch(stream_id, 2, Priority::HIGH, vec![]);

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("at least one patch"));
}

#[test]
fn test_patch_frame_validate_success() {
    let stream_id = StreamId::new();
    let path = JsonPath::new("$.test").unwrap();
    let patch = FramePatch::set(path, JsonData::Null);
    let frame = Frame::patch(stream_id, 1, Priority::LOW, vec![patch]).unwrap();

    assert!(frame.validate().is_ok());
}

#[test]
fn test_patch_frame_with_different_priorities() {
    let stream_id = StreamId::new();
    let path = JsonPath::new("$.data").unwrap();
    let patch = FramePatch::set(path, JsonData::Integer(42));

    let high = Frame::patch(stream_id, 1, Priority::HIGH, vec![patch.clone()]).unwrap();
    assert_eq!(high.priority(), Priority::HIGH);

    let medium = Frame::patch(stream_id, 2, Priority::MEDIUM, vec![patch.clone()]).unwrap();
    assert_eq!(medium.priority(), Priority::MEDIUM);

    let low = Frame::patch(stream_id, 3, Priority::LOW, vec![patch.clone()]).unwrap();
    assert_eq!(low.priority(), Priority::LOW);
}

// ============================================================================
// Complete Frame Tests
// ============================================================================

#[test]
fn test_complete_frame_with_checksum() {
    let stream_id = StreamId::new();
    let frame = Frame::complete(stream_id, 10, Some("abc123def456".to_string()));

    assert_eq!(frame.frame_type(), &FrameType::Complete);
    assert_eq!(frame.priority(), Priority::CRITICAL);
    assert_eq!(frame.sequence(), 10);

    if let JsonData::Object(obj) = frame.payload() {
        assert_eq!(obj.get("checksum").unwrap().as_str(), Some("abc123def456"));
    } else {
        panic!("Expected object payload");
    }
}

#[test]
fn test_complete_frame_without_checksum() {
    let stream_id = StreamId::new();
    let frame = Frame::complete(stream_id, 5, None);

    assert_eq!(frame.frame_type(), &FrameType::Complete);
    assert!(frame.payload().is_object());

    if let JsonData::Object(obj) = frame.payload() {
        assert!(obj.is_empty());
    } else {
        panic!("Expected object payload");
    }
}

#[test]
fn test_complete_frame_validate_success() {
    let stream_id = StreamId::new();
    let frame = Frame::complete(stream_id, 10, Some("checksum".to_string()));
    assert!(frame.validate().is_ok());
}

#[test]
fn test_complete_frame_always_critical() {
    let stream_id = StreamId::new();
    let frame = Frame::complete(stream_id, 1, None);
    assert!(frame.is_critical());
    assert_eq!(frame.priority(), Priority::CRITICAL);
}

// ============================================================================
// Error Frame Tests
// ============================================================================

#[test]
fn test_error_frame_with_code() {
    let stream_id = StreamId::new();
    let frame = Frame::error(
        stream_id,
        5,
        "Something went wrong".to_string(),
        Some("ERR_500".to_string()),
    );

    assert_eq!(frame.frame_type(), &FrameType::Error);
    assert_eq!(frame.priority(), Priority::CRITICAL);
    assert_eq!(frame.sequence(), 5);

    if let JsonData::Object(obj) = frame.payload() {
        assert_eq!(
            obj.get("message").unwrap().as_str(),
            Some("Something went wrong")
        );
        assert_eq!(obj.get("code").unwrap().as_str(), Some("ERR_500"));
    } else {
        panic!("Expected object payload");
    }
}

#[test]
fn test_error_frame_without_code() {
    let stream_id = StreamId::new();
    let frame = Frame::error(stream_id, 3, "Error occurred".to_string(), None);

    assert_eq!(frame.frame_type(), &FrameType::Error);

    if let JsonData::Object(obj) = frame.payload() {
        assert_eq!(obj.get("message").unwrap().as_str(), Some("Error occurred"));
        assert!(obj.get("code").is_none());
    } else {
        panic!("Expected object payload");
    }
}

#[test]
fn test_error_frame_validate_success() {
    let stream_id = StreamId::new();
    let frame = Frame::error(stream_id, 1, "Test error".to_string(), None);
    assert!(frame.validate().is_ok());
}

#[test]
fn test_error_frame_always_critical() {
    let stream_id = StreamId::new();
    let frame = Frame::error(stream_id, 1, "Error".to_string(), None);
    assert!(frame.is_critical());
    assert_eq!(frame.priority(), Priority::CRITICAL);
}

// ============================================================================
// Frame Metadata Tests
// ============================================================================

#[test]
fn test_frame_with_metadata() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Null)
        .with_metadata("source".to_string(), "api".to_string())
        .with_metadata("version".to_string(), "1.0".to_string());

    assert_eq!(frame.metadata().len(), 2);
    assert_eq!(frame.get_metadata("source"), Some(&"api".to_string()));
    assert_eq!(frame.get_metadata("version"), Some(&"1.0".to_string()));
}

#[test]
fn test_frame_metadata_chaining() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Null)
        .with_metadata("key1".to_string(), "value1".to_string())
        .with_metadata("key2".to_string(), "value2".to_string())
        .with_metadata("key3".to_string(), "value3".to_string());

    assert_eq!(frame.metadata().len(), 3);
}

#[test]
fn test_frame_get_metadata_nonexistent() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Null);
    assert!(frame.get_metadata("nonexistent").is_none());
}

#[test]
fn test_frame_empty_metadata() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Null);
    assert!(frame.metadata().is_empty());
}

// ============================================================================
// Frame Accessor Tests
// ============================================================================

#[test]
fn test_frame_getters() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 42, JsonData::Integer(100));

    assert_eq!(frame.stream_id(), stream_id);
    assert_eq!(frame.sequence(), 42);
    assert_eq!(frame.priority(), Priority::CRITICAL);
    assert_eq!(frame.payload().as_i64(), Some(100));
}

#[test]
fn test_frame_timestamp() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Null);
    let timestamp = frame.timestamp();

    // Timestamp should be recent
    let now = chrono::Utc::now();
    let diff = now.signed_duration_since(timestamp);
    assert!(diff.num_seconds() < 5);
}

// ============================================================================
// Frame Priority Methods Tests
// ============================================================================

#[test]
fn test_frame_is_critical() {
    let stream_id = StreamId::new();

    let skeleton = Frame::skeleton(stream_id, 1, JsonData::Null);
    assert!(skeleton.is_critical());

    let complete = Frame::complete(stream_id, 2, None);
    assert!(complete.is_critical());

    let error = Frame::error(stream_id, 3, "Error".to_string(), None);
    assert!(error.is_critical());
}

#[test]
fn test_frame_is_high_priority() {
    let stream_id = StreamId::new();

    let critical = Frame::skeleton(stream_id, 1, JsonData::Null);
    assert!(critical.is_high_priority());

    let path = JsonPath::new("$.test").unwrap();
    let high = Frame::patch(
        stream_id,
        2,
        Priority::HIGH,
        vec![FramePatch::set(path.clone(), JsonData::Null)],
    )
    .unwrap();
    assert!(high.is_high_priority());

    let medium = Frame::patch(
        stream_id,
        3,
        Priority::MEDIUM,
        vec![FramePatch::set(path.clone(), JsonData::Null)],
    )
    .unwrap();
    assert!(!medium.is_high_priority());

    let low = Frame::patch(
        stream_id,
        4,
        Priority::LOW,
        vec![FramePatch::set(path, JsonData::Null)],
    )
    .unwrap();
    assert!(!low.is_high_priority());
}

// ============================================================================
// Frame Size Estimation Tests
// ============================================================================

#[test]
fn test_frame_estimated_size_minimal() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Null);
    let size = frame.estimated_size();

    // Should have at least base overhead
    assert!(size >= 200);
}

#[test]
fn test_frame_estimated_size_with_data() {
    let stream_id = StreamId::new();
    let large_string = "x".repeat(1000);
    let frame = Frame::skeleton(stream_id, 1, JsonData::String(large_string));
    let size = frame.estimated_size();

    // Should be significantly larger
    assert!(size > 1000);
}

#[test]
fn test_frame_estimated_size_with_metadata() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Null)
        .with_metadata("key1".to_string(), "value1".to_string())
        .with_metadata("key2".to_string(), "value2".to_string());

    let size = frame.estimated_size();
    assert!(size > 200);
}

// ============================================================================
// FramePatch Tests
// ============================================================================

#[test]
fn test_frame_patch_set() {
    let path = JsonPath::new("$.user.name").unwrap();
    let patch = FramePatch::set(path.clone(), JsonData::String("Alice".to_string()));

    assert_eq!(patch.path, path);
    assert_eq!(patch.operation, PatchOperation::Set);
    assert_eq!(patch.value.as_str(), Some("Alice"));
}

#[test]
fn test_frame_patch_append() {
    let path = JsonPath::new("$.items").unwrap();
    let patch = FramePatch::append(path.clone(), JsonData::Integer(42));

    assert_eq!(patch.path, path);
    assert_eq!(patch.operation, PatchOperation::Append);
    assert_eq!(patch.value.as_i64(), Some(42));
}

#[test]
fn test_frame_patch_merge() {
    let path = JsonPath::new("$.config").unwrap();
    let patch = FramePatch::merge(path.clone(), JsonData::Object(HashMap::new()));

    assert_eq!(patch.path, path);
    assert_eq!(patch.operation, PatchOperation::Merge);
    assert!(patch.value.is_object());
}

#[test]
fn test_frame_patch_delete() {
    let path = JsonPath::new("$.old_field").unwrap();
    let patch = FramePatch::delete(path.clone());

    assert_eq!(patch.path, path);
    assert_eq!(patch.operation, PatchOperation::Delete);
    assert_eq!(patch.value, JsonData::Null);
}

#[test]
fn test_frame_patch_clone() {
    let path = JsonPath::new("$.test").unwrap();
    let original = FramePatch::set(path, JsonData::Bool(true));
    let cloned = original.clone();

    assert_eq!(original, cloned);
}

#[test]
fn test_frame_patch_debug() {
    let path = JsonPath::new("$.test").unwrap();
    let patch = FramePatch::set(path, JsonData::Integer(42));
    let debug = format!("{:?}", patch);

    assert!(debug.contains("FramePatch"));
}

// ============================================================================
// PatchOperation Tests
// ============================================================================

#[test]
fn test_patch_operation_variants() {
    let set = PatchOperation::Set;
    let append = PatchOperation::Append;
    let merge = PatchOperation::Merge;
    let delete = PatchOperation::Delete;

    assert_eq!(set, PatchOperation::Set);
    assert_eq!(append, PatchOperation::Append);
    assert_eq!(merge, PatchOperation::Merge);
    assert_eq!(delete, PatchOperation::Delete);
}

#[test]
fn test_patch_operation_clone() {
    let original = PatchOperation::Set;
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

// ============================================================================
// Frame Serialization Tests
// ============================================================================

#[test]
fn test_frame_serialize_skeleton() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Null);
    let _ = serde_json::to_string(&frame);
}

#[test]
fn test_frame_serialize_patch() {
    let stream_id = StreamId::new();
    let path = JsonPath::new("$.test").unwrap();
    let patch = FramePatch::set(path, JsonData::Integer(42));
    let frame = Frame::patch(stream_id, 1, Priority::HIGH, vec![patch]).unwrap();

    let _ = serde_json::to_string(&frame);
}

#[test]
fn test_frame_deserialize() {
    let stream_id = StreamId::new();
    let original = Frame::skeleton(stream_id, 1, JsonData::Null);
    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: Frame = serde_json::from_str(&serialized).unwrap();

    assert_eq!(original.sequence(), deserialized.sequence());
    assert_eq!(original.frame_type(), deserialized.frame_type());
}

// ============================================================================
// Frame Hash Tests
// ============================================================================

#[test]
fn test_frame_hash() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 1, JsonData::Null);

    let mut hasher = DefaultHasher::new();
    frame.hash(&mut hasher);
    let _ = hasher.finish();
}

#[test]
fn test_frame_hash_with_metadata() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let hash_value = |f: &Frame| {
        let mut hasher = DefaultHasher::new();
        f.hash(&mut hasher);
        hasher.finish()
    };

    let stream_id = StreamId::new();
    let frame1 = Frame::skeleton(stream_id, 1, JsonData::Null)
        .with_metadata("key1".to_string(), "value1".to_string());

    // Hashing should work consistently for the same frame
    let hash1 = hash_value(&frame1);
    let hash2 = hash_value(&frame1);
    assert_eq!(hash1, hash2);

    // Note: Two frames with same data but different timestamps will have different hashes
    // This is expected behavior
}

// ============================================================================
// Frame Equality Tests
// ============================================================================

#[test]
fn test_frame_equality() {
    let stream_id1 = StreamId::new();
    let stream_id2 = StreamId::new();

    let frame1 = Frame::skeleton(stream_id1, 1, JsonData::Null);
    let _frame2 = Frame::skeleton(stream_id1, 1, JsonData::Null);
    let frame3 = Frame::skeleton(stream_id2, 1, JsonData::Null);

    // Different stream IDs should not be equal
    assert_ne!(frame1, frame3);

    // Note: Same stream_id but different timestamps will not be equal
    // This is expected behavior as timestamp is part of the frame
}

#[test]
fn test_frame_clone() {
    let stream_id = StreamId::new();
    let original = Frame::skeleton(stream_id, 1, JsonData::Integer(42));
    let cloned = original.clone();

    assert_eq!(original.sequence(), cloned.sequence());
    assert_eq!(original.stream_id(), cloned.stream_id());
    assert_eq!(original.frame_type(), cloned.frame_type());
}

// ============================================================================
// Edge Cases and Error Scenarios
// ============================================================================

#[test]
fn test_frame_sequence_zero() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, 0, JsonData::Null);
    assert_eq!(frame.sequence(), 0);
}

#[test]
fn test_frame_sequence_large() {
    let stream_id = StreamId::new();
    let frame = Frame::skeleton(stream_id, u64::MAX, JsonData::Null);
    assert_eq!(frame.sequence(), u64::MAX);
}

#[test]
fn test_complete_frame_empty_checksum() {
    let stream_id = StreamId::new();
    let frame = Frame::complete(stream_id, 1, Some("".to_string()));

    if let JsonData::Object(obj) = frame.payload() {
        assert_eq!(obj.get("checksum").unwrap().as_str(), Some(""));
    } else {
        panic!("Expected object payload");
    }
}

#[test]
fn test_error_frame_empty_message() {
    let stream_id = StreamId::new();
    let frame = Frame::error(stream_id, 1, "".to_string(), None);

    if let JsonData::Object(obj) = frame.payload() {
        assert_eq!(obj.get("message").unwrap().as_str(), Some(""));
    } else {
        panic!("Expected object payload");
    }
}

#[test]
fn test_patch_with_null_value() {
    let stream_id = StreamId::new();
    let path = JsonPath::new("$.field").unwrap();
    let patch = FramePatch::set(path, JsonData::Null);
    let frame = Frame::patch(stream_id, 1, Priority::LOW, vec![patch]).unwrap();

    assert!(frame.validate().is_ok());
}
