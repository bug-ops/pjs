//! Acceptance tests for #604: validating constructors for the repository port query types.
//!
//! API frozen in the architect plan ("Frozen #604 API"); failures assert the
//! `DomainError::InvalidInput` variant, not its text.

use core::assert_matches;
use jiff::{SignedDuration, Timestamp};
use pjson_rs::domain::DomainError;
use pjson_rs::domain::config::limits::{MAX_PAGINATION_LIMIT, MAX_PAGINATION_OFFSET};
use pjson_rs::domain::entities::{Stream, frame::FramePatch};
use pjson_rs::domain::ports::repositories::{
    FramePresence, NonEmptyStatuses, SessionQueryCriteria, StreamFilter,
};
use pjson_rs::domain::ports::{
    SessionPagination, SessionSortField, SortOrder, StreamStatus, StreamStoreGat,
};
use pjson_rs::domain::value_objects::{JsonData, JsonPath, Priority, SessionId};
use pjson_rs::infrastructure::adapters::gat_memory_repository::GatInMemoryStreamStore;

fn assert_invalid_input<T: std::fmt::Debug>(result: Result<T, DomainError>) {
    assert_matches!(
        result,
        Err(DomainError::InvalidInput(_)),
        "expected InvalidInput, got {result:?}"
    );
}

#[test]
fn test_non_empty_statuses_new_empty_rejected() {
    assert_invalid_input(NonEmptyStatuses::new(vec![]));
}

#[test]
fn test_stream_filter_build_empty_statuses_rejected() {
    assert_invalid_input(StreamFilter::builder().statuses(vec![]).build());
}

#[test]
fn test_stream_filter_build_with_statuses_exposes_them() {
    let filter = StreamFilter::builder()
        .statuses(vec![StreamStatus::Active])
        .build()
        .expect("non-empty statuses are valid");
    assert_eq!(
        filter.statuses().expect("statuses set").as_slice(),
        [StreamStatus::Active]
    );
}

#[test]
fn test_stream_filter_build_empty_builder_has_no_filters() {
    let filter = StreamFilter::builder()
        .build()
        .expect("empty builder is valid");
    assert!(filter.statuses().is_none());
    assert!(filter.created_after().is_none());
    assert!(filter.has_frames().is_none());
}

#[test]
fn test_stream_filter_getters_return_set_values() {
    let after = Timestamp::now();
    let filter = StreamFilter::builder()
        .created_after(after)
        .has_frames(FramePresence::WithFrames)
        .build()
        .expect("valid");
    assert_eq!(filter.created_after(), Some(after));
    assert_eq!(filter.has_frames(), Some(FramePresence::WithFrames));
}

fn stream_for(session_id: SessionId, with_frame: bool) -> Stream {
    let mut stream = Stream::new(
        session_id,
        JsonData::Object(Default::default()),
        Default::default(),
    );
    stream.start_streaming().expect("starts");
    if with_frame {
        stream
            .finalize_patch_frame(
                Priority::HIGH,
                vec![FramePatch::set(JsonPath::root(), JsonData::Bool(true))],
            )
            .expect("frame committed");
    }
    stream
}

#[tokio::test]
async fn test_find_streams_by_session_has_frames_filter_selects_matching_stream() {
    let store = GatInMemoryStreamStore::new();
    let session_id = SessionId::new();
    let with_frames = stream_for(session_id, true);
    let without_frames = stream_for(session_id, false);
    let (with_id, without_id) = (with_frames.id(), without_frames.id());
    store.store_stream(with_frames).await.expect("stored");
    store.store_stream(without_frames).await.expect("stored");

    let ids = |streams: Vec<Stream>| {
        let mut ids: Vec<_> = streams.iter().map(Stream::id).collect();
        ids.sort_by_key(|id| id.as_uuid());
        ids
    };
    let mut both = vec![with_id, without_id];
    both.sort_by_key(|id| id.as_uuid());

    let only_with = store
        .find_streams_by_session(
            session_id,
            StreamFilter::builder()
                .has_frames(FramePresence::WithFrames)
                .build()
                .expect("valid"),
        )
        .await
        .expect("query");
    assert_eq!(ids(only_with), vec![with_id]);

    let only_without = store
        .find_streams_by_session(
            session_id,
            StreamFilter::builder()
                .has_frames(FramePresence::WithoutFrames)
                .build()
                .expect("valid"),
        )
        .await
        .expect("query");
    assert_eq!(ids(only_without), vec![without_id]);

    let all = store
        .find_streams_by_session(session_id, StreamFilter::default())
        .await
        .expect("query");
    assert_eq!(ids(all), both);
}

#[test]
fn test_pagination_new_invalid_bounds_rejected() {
    assert_invalid_input(SessionPagination::new(0, 0));
    assert_invalid_input(SessionPagination::new(0, MAX_PAGINATION_LIMIT + 1));
    assert_invalid_input(SessionPagination::new(MAX_PAGINATION_OFFSET + 1, 1));
}

#[test]
fn test_pagination_new_at_limits_accepted_with_getters() {
    let pagination = SessionPagination::new(MAX_PAGINATION_OFFSET, MAX_PAGINATION_LIMIT)
        .expect("limits are inclusive");
    assert_eq!(pagination.offset(), MAX_PAGINATION_OFFSET);
    assert_eq!(pagination.limit(), MAX_PAGINATION_LIMIT);
}

#[test]
fn test_pagination_with_sort_sets_field_and_order() {
    let pagination = SessionPagination::new(0, 10)
        .expect("valid")
        .with_sort(SessionSortField::CreatedAt, SortOrder::Descending);
    assert_eq!(pagination.sort_by(), Some(&SessionSortField::CreatedAt));
    assert_eq!(pagination.sort_order(), &SortOrder::Descending);
}

#[test]
fn test_pagination_default_values() {
    let pagination = SessionPagination::default();
    assert_eq!(pagination.offset(), 0);
    assert_eq!(pagination.limit(), 50);
    assert!(pagination.sort_by().is_none());
    assert_eq!(pagination.sort_order(), &SortOrder::Ascending);
}

#[test]
fn test_session_query_criteria_build_min_above_max_rejected() {
    assert_invalid_input(
        SessionQueryCriteria::builder()
            .min_stream_count(5)
            .max_stream_count(3)
            .build(),
    );
}

#[test]
fn test_session_query_criteria_build_created_after_later_than_before_rejected() {
    let now = Timestamp::now();
    assert_invalid_input(
        SessionQueryCriteria::builder()
            .created_after(now)
            .created_before(now - SignedDuration::from_secs(1))
            .build(),
    );
}

#[test]
fn test_session_query_criteria_build_empty_states_rejected() {
    assert_invalid_input(SessionQueryCriteria::builder().states(vec![]).build());
}

#[test]
fn test_session_query_criteria_build_equal_bounds_accepted() {
    let now = Timestamp::now();
    let criteria = SessionQueryCriteria::builder()
        .min_stream_count(3)
        .max_stream_count(3)
        .created_after(now)
        .created_before(now)
        .build()
        .expect("equal bounds are valid");
    assert_eq!(criteria.min_stream_count(), Some(3));
    assert_eq!(criteria.max_stream_count(), Some(3));
}

#[test]
fn test_session_query_criteria_build_empty_builder_accepted() {
    let criteria = SessionQueryCriteria::builder().build().expect("valid");
    assert!(criteria.states().is_none());
    assert!(!criteria.exclude_expired());
}

#[test]
fn test_session_query_criteria_getters_return_set_values() {
    let criteria = SessionQueryCriteria::builder()
        .states(vec!["Active".to_owned()])
        .client_info_pattern("agent".to_owned())
        .has_active_streams(true)
        .exclude_expired(true)
        .build()
        .expect("valid");
    assert_eq!(criteria.states(), Some(["Active".to_owned()].as_slice()));
    assert_eq!(criteria.client_info_pattern(), Some("agent"));
    assert_eq!(criteria.has_active_streams(), Some(true));
    assert!(criteria.exclude_expired());
}
