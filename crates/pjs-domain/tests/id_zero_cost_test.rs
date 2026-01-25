//! Zero-Cost Abstraction Verification for Generic Id<T>
//!
//! This module verifies that the generic Id<T> implementation with PhantomData<T>
//! provides zero runtime overhead compared to a plain Uuid.

use pjson_rs_domain::value_objects::{Id, SessionId, SessionMarker, StreamId, StreamMarker};
use std::mem::{align_of, size_of};
use uuid::Uuid;

/// Verify that Id<T> has the same size as Uuid (zero-cost abstraction)
#[test]
fn test_id_size_equals_uuid_size() {
    let uuid_size = size_of::<Uuid>();
    let session_id_size = size_of::<SessionId>();
    let stream_id_size = size_of::<StreamId>();

    assert_eq!(
        uuid_size, session_id_size,
        "SessionId size ({session_id_size}) should equal Uuid size ({uuid_size})"
    );
    assert_eq!(
        uuid_size, stream_id_size,
        "StreamId size ({stream_id_size}) should equal Uuid size ({uuid_size})"
    );

    // Uuid is 16 bytes (128 bits)
    assert_eq!(uuid_size, 16, "Uuid should be 16 bytes");
    assert_eq!(session_id_size, 16, "SessionId should be 16 bytes");
    assert_eq!(stream_id_size, 16, "StreamId should be 16 bytes");
}

/// Verify that Id<T> has the same alignment as Uuid
#[test]
fn test_id_alignment_equals_uuid_alignment() {
    let uuid_align = align_of::<Uuid>();
    let session_id_align = align_of::<SessionId>();
    let stream_id_align = align_of::<StreamId>();

    assert_eq!(
        uuid_align, session_id_align,
        "SessionId alignment ({session_id_align}) should equal Uuid alignment ({uuid_align})"
    );
    assert_eq!(
        uuid_align, stream_id_align,
        "StreamId alignment ({stream_id_align}) should equal Uuid alignment ({uuid_align})"
    );
}

/// Verify that marker types are zero-sized (ZST)
#[test]
fn test_markers_are_zero_sized() {
    use std::marker::PhantomData;

    // Marker types should be ZST
    assert_eq!(
        size_of::<SessionMarker>(),
        0,
        "SessionMarker should be zero-sized"
    );
    assert_eq!(
        size_of::<StreamMarker>(),
        0,
        "StreamMarker should be zero-sized"
    );

    // PhantomData should be ZST
    assert_eq!(
        size_of::<PhantomData<SessionMarker>>(),
        0,
        "PhantomData<SessionMarker> should be zero-sized"
    );
    assert_eq!(
        size_of::<PhantomData<StreamMarker>>(),
        0,
        "PhantomData<StreamMarker> should be zero-sized"
    );
}

/// Verify that different Id<T> types are the same size (monomorphization doesn't add overhead)
#[test]
fn test_monomorphization_no_size_overhead() {
    let session_id_size = size_of::<Id<SessionMarker>>();
    let stream_id_size = size_of::<Id<StreamMarker>>();

    assert_eq!(
        session_id_size, stream_id_size,
        "All Id<T> instantiations should have the same size"
    );
}

/// Verify memory layout is optimal (no padding)
#[test]
fn test_no_padding_in_layout() {
    // Uuid is 16 bytes with typically 1 or 8 byte alignment
    // Id<T> should have the exact same layout
    let uuid_size = size_of::<Uuid>();
    let id_size = size_of::<SessionId>();

    // If there's no padding, size should equal uuid size
    assert_eq!(
        uuid_size, id_size,
        "Id<T> should have no padding beyond Uuid"
    );

    // Verify alignment doesn't cause any issues
    let uuid_align = align_of::<Uuid>();
    let id_align = align_of::<SessionId>();
    assert_eq!(uuid_align, id_align, "Alignment should be identical");
}

/// Verify that arrays of Id<T> have optimal memory layout
#[test]
fn test_array_layout() {
    let uuid_array_size = size_of::<[Uuid; 10]>();
    let session_id_array_size = size_of::<[SessionId; 10]>();
    let stream_id_array_size = size_of::<[StreamId; 10]>();

    assert_eq!(
        uuid_array_size, session_id_array_size,
        "Array of SessionId should have same size as array of Uuid"
    );
    assert_eq!(
        uuid_array_size, stream_id_array_size,
        "Array of StreamId should have same size as array of Uuid"
    );

    // 10 UUIDs = 160 bytes
    assert_eq!(
        uuid_array_size, 160,
        "Array of 10 Uuids should be 160 bytes"
    );
}

/// Verify that Option<Id<T>> has the same size as Option<Uuid>
#[test]
fn test_option_size_preservation() {
    // This test asserts that Option<Id<T>> has the same size as Option<Uuid>.
    // Note: Uuid has no niche for None, so Option<Uuid> is typically larger
    // than Uuid itself (it needs a discriminant). This test verifies that
    // wrapping Uuid in Id<T> doesn't add any extra overhead to Option.
    let uuid_option_size = size_of::<Option<Uuid>>();
    let session_id_option_size = size_of::<Option<SessionId>>();
    let stream_id_option_size = size_of::<Option<StreamId>>();

    assert_eq!(
        uuid_option_size, session_id_option_size,
        "Option<SessionId> should have same size as Option<Uuid>"
    );
    assert_eq!(
        uuid_option_size, stream_id_option_size,
        "Option<StreamId> should have same size as Option<Uuid>"
    );
}

/// Verify that Id<T> can be copied without allocation (Copy trait)
#[test]
fn test_id_is_copy() {
    fn assert_copy<T: Copy>() {}

    assert_copy::<SessionId>();
    assert_copy::<StreamId>();
    assert_copy::<Uuid>();
}

/// Verify that Id<T> implements Copy (no allocation needed)
#[test]
fn test_id_copy_is_trivial() {
    let original = SessionId::new();
    let copied = original; // Copy happens implicitly

    // Copy should be bitwise identical
    assert_eq!(original.as_uuid(), copied.as_uuid());

    // Verify both Copy and Clone are implemented
    fn assert_copy_clone<T: Copy + Clone>() {}
    assert_copy_clone::<SessionId>();
    assert_copy_clone::<StreamId>();
}

/// Verify that type safety is preserved despite zero-cost abstraction
#[test]
fn test_type_safety_preserved() {
    let uuid = Uuid::new_v4();
    let session_id = SessionId::from_uuid(uuid);
    let stream_id = StreamId::from_uuid(uuid);

    // Same underlying UUID
    assert_eq!(session_id.as_uuid(), stream_id.as_uuid());

    // But they are different types - this is checked at compile time
    // The following would not compile:
    // let _: SessionId = stream_id;
    // assert_eq!(session_id, stream_id);
}

/// Integration test: verify full lifecycle has no overhead
#[test]
fn test_full_lifecycle_no_overhead() {
    // Create
    let session_id = SessionId::new();

    // Convert to UUID and back
    let uuid: Uuid = session_id.into();
    let session_id_back: SessionId = uuid.into();

    assert_eq!(session_id.as_uuid(), session_id_back.as_uuid());

    // String roundtrip
    let string_repr = session_id.as_str();
    let parsed = SessionId::from_string(&string_repr).unwrap();
    assert_eq!(session_id.as_uuid(), parsed.as_uuid());
}

/// Compile-time verification summary
#[test]
fn test_zero_cost_summary() {
    // This test documents the zero-cost abstraction guarantees

    // 1. Size equality
    assert_eq!(size_of::<SessionId>(), size_of::<Uuid>());
    assert_eq!(size_of::<StreamId>(), size_of::<Uuid>());

    // 2. Alignment equality
    assert_eq!(align_of::<SessionId>(), align_of::<Uuid>());
    assert_eq!(align_of::<StreamId>(), align_of::<Uuid>());

    // 3. Zero-sized markers
    assert_eq!(size_of::<SessionMarker>(), 0);
    assert_eq!(size_of::<StreamMarker>(), 0);

    // 4. Copy semantics (no heap allocation)
    let id = SessionId::new();
    let _copied = id; // Copy, not move
    let _still_valid = id; // Original still valid

    // VERDICT: Id<T> is a zero-cost abstraction over Uuid
}
