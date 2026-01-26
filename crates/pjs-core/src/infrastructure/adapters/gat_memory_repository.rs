//! GAT-based in-memory repository implementations
//!
//! Zero-cost abstractions for domain ports using Generic Associated Types.

use chrono::Utc;
use std::cmp::Ordering;
use std::future::Future;

use crate::domain::{
    DomainResult,
    aggregates::StreamSession,
    entities::{Stream, stream::StreamState},
    ports::{
        Pagination, PriorityDistribution, SessionHealthSnapshot, SessionQueryCriteria,
        SessionQueryResult, SortOrder, StreamFilter, StreamRepositoryGat, StreamStatistics,
        StreamStatus, StreamStoreGat,
    },
    value_objects::{SessionId, StreamId},
};

use super::generic_store::{SessionStore, StreamStore};

/// GAT-based in-memory implementation of StreamRepositoryGat
#[derive(Debug, Clone, Default)]
pub struct GatInMemoryStreamRepository {
    store: SessionStore,
}

impl GatInMemoryStreamRepository {
    pub fn new() -> Self {
        Self {
            store: SessionStore::new(),
        }
    }

    /// Get number of stored sessions
    pub fn session_count(&self) -> usize {
        self.store.count()
    }

    /// Clear all sessions (for testing)
    pub fn clear(&self) {
        self.store.clear();
    }

    /// Get all session IDs (for testing)
    pub fn all_session_ids(&self) -> Vec<SessionId> {
        self.store.all_keys()
    }

    /// Helper: Check if session matches query criteria
    fn matches_criteria(session: &StreamSession, criteria: &SessionQueryCriteria) -> bool {
        // Check state filter
        if let Some(states) = &criteria.states {
            let state_str = format!("{:?}", session.state());
            if !states.iter().any(|s| s.eq_ignore_ascii_case(&state_str)) {
                return false;
            }
        }

        // Check time range
        if let Some(after) = criteria.created_after
            && session.created_at() < after
        {
            return false;
        }
        if let Some(before) = criteria.created_before
            && session.created_at() > before
        {
            return false;
        }

        // Check active streams filter
        if let Some(has_active) = criteria.has_active_streams {
            let active_count = session.streams().values().filter(|s| s.is_active()).count();
            if has_active && active_count == 0 {
                return false;
            }
            if !has_active && active_count > 0 {
                return false;
            }
        }

        // Check stream count bounds
        let stream_count = session.streams().len();
        if let Some(min) = criteria.min_stream_count
            && stream_count < min
        {
            return false;
        }
        if let Some(max) = criteria.max_stream_count
            && stream_count > max
        {
            return false;
        }

        true
    }

    /// Helper: Compare sessions by field for sorting
    fn compare_by_field(a: &StreamSession, b: &StreamSession, field: &str) -> Ordering {
        match field {
            "created_at" => a.created_at().cmp(&b.created_at()),
            "updated_at" => a.updated_at().cmp(&b.updated_at()),
            "stream_count" => a.streams().len().cmp(&b.streams().len()),
            _ => Ordering::Equal,
        }
    }
}

impl StreamRepositoryGat for GatInMemoryStreamRepository {
    type FindSessionFuture<'a>
        = impl Future<Output = DomainResult<Option<StreamSession>>> + Send + 'a
    where
        Self: 'a;

    type SaveSessionFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type RemoveSessionFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type FindActiveSessionsFuture<'a>
        = impl Future<Output = DomainResult<Vec<StreamSession>>> + Send + 'a
    where
        Self: 'a;

    type FindSessionsByCriteriaFuture<'a>
        = impl Future<Output = DomainResult<SessionQueryResult>> + Send + 'a
    where
        Self: 'a;

    type GetSessionHealthFuture<'a>
        = impl Future<Output = DomainResult<SessionHealthSnapshot>> + Send + 'a
    where
        Self: 'a;

    type SessionExistsFuture<'a>
        = impl Future<Output = DomainResult<bool>> + Send + 'a
    where
        Self: 'a;

    fn find_session(&self, session_id: SessionId) -> Self::FindSessionFuture<'_> {
        async move { Ok(self.store.get(&session_id)) }
    }

    fn save_session(&self, session: StreamSession) -> Self::SaveSessionFuture<'_> {
        async move {
            self.store.insert(session.id(), session);
            Ok(())
        }
    }

    fn remove_session(&self, session_id: SessionId) -> Self::RemoveSessionFuture<'_> {
        async move {
            self.store.remove(&session_id);
            Ok(())
        }
    }

    fn find_active_sessions(&self) -> Self::FindActiveSessionsFuture<'_> {
        async move { Ok(self.store.filter(|s| s.is_active())) }
    }

    fn find_sessions_by_criteria(
        &self,
        criteria: SessionQueryCriteria,
        pagination: Pagination,
    ) -> Self::FindSessionsByCriteriaFuture<'_> {
        async move {
            let start = std::time::Instant::now();

            // Filter sessions matching criteria
            let filtered: Vec<StreamSession> = self
                .store
                .filter(|session| Self::matches_criteria(session, &criteria));

            let total_count = filtered.len();

            // Sort if sort_by specified
            let mut sorted = filtered;
            if let Some(sort_field) = &pagination.sort_by {
                sorted.sort_by(|a, b| {
                    let cmp = Self::compare_by_field(a, b, sort_field);
                    match pagination.sort_order {
                        SortOrder::Ascending => cmp,
                        SortOrder::Descending => cmp.reverse(),
                    }
                });
            }

            // Apply pagination
            let paginated: Vec<StreamSession> = sorted
                .into_iter()
                .skip(pagination.offset)
                .take(pagination.limit)
                .collect();

            let has_more = pagination.offset + paginated.len() < total_count;

            Ok(SessionQueryResult {
                sessions: paginated,
                total_count,
                has_more,
                query_duration_ms: start.elapsed().as_millis() as u64,
            })
        }
    }

    fn get_session_health(&self, session_id: SessionId) -> Self::GetSessionHealthFuture<'_> {
        async move {
            match self.store.get(&session_id) {
                Some(session) => {
                    let health = session.health_check();

                    // Aggregate stream frame counts
                    let total_frames: u64 = session
                        .streams()
                        .values()
                        .map(|s| s.stats().total_frames)
                        .sum();

                    // Calculate error rate from failed streams
                    let failed_streams = health.failed_streams as f64;
                    let total_streams = session.streams().len() as f64;
                    let error_rate = if total_streams > 0.0 {
                        failed_streams / total_streams
                    } else {
                        0.0
                    };

                    // Collect metrics from session stats
                    let mut metrics = std::collections::HashMap::new();
                    metrics.insert("active_streams".to_string(), health.active_streams as f64);
                    metrics.insert(
                        "total_bytes".to_string(),
                        session.stats().total_bytes as f64,
                    );
                    metrics.insert(
                        "avg_duration_ms".to_string(),
                        session.stats().average_stream_duration_ms,
                    );

                    Ok(SessionHealthSnapshot {
                        session_id,
                        is_healthy: health.is_healthy,
                        active_streams: health.active_streams,
                        total_frames,
                        last_activity: session.updated_at(),
                        error_rate,
                        metrics,
                    })
                }
                None => {
                    // Return empty health for non-existent session
                    Ok(SessionHealthSnapshot {
                        session_id,
                        is_healthy: false,
                        active_streams: 0,
                        total_frames: 0,
                        last_activity: Utc::now(),
                        error_rate: 0.0,
                        metrics: std::collections::HashMap::new(),
                    })
                }
            }
        }
    }

    #[inline]
    fn session_exists(&self, session_id: SessionId) -> Self::SessionExistsFuture<'_> {
        async move { Ok(self.store.contains_key(&session_id)) }
    }
}

/// GAT-based in-memory implementation of StreamStoreGat
#[derive(Debug, Clone, Default)]
pub struct GatInMemoryStreamStore {
    store: StreamStore,
}

impl GatInMemoryStreamStore {
    pub fn new() -> Self {
        Self {
            store: StreamStore::new(),
        }
    }

    /// Get number of stored streams
    pub fn stream_count(&self) -> usize {
        self.store.count()
    }

    /// Clear all streams (for testing)
    pub fn clear(&self) {
        self.store.clear();
    }

    /// Get all stream IDs (for testing)
    pub fn all_stream_ids(&self) -> Vec<StreamId> {
        self.store.all_keys()
    }

    /// Helper: Check if stream state matches requested status
    fn stream_state_matches_status(state: &StreamState, status: &StreamStatus) -> bool {
        matches!(
            (state, status),
            (StreamState::Preparing, StreamStatus::Created)
                | (StreamState::Streaming, StreamStatus::Active)
                | (StreamState::Completed, StreamStatus::Completed)
                | (StreamState::Failed, StreamStatus::Failed)
                | (StreamState::Cancelled, StreamStatus::Cancelled)
        )
    }
}

impl StreamStoreGat for GatInMemoryStreamStore {
    type StoreStreamFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type GetStreamFuture<'a>
        = impl Future<Output = DomainResult<Option<Stream>>> + Send + 'a
    where
        Self: 'a;

    type DeleteStreamFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type ListStreamsForSessionFuture<'a>
        = impl Future<Output = DomainResult<Vec<Stream>>> + Send + 'a
    where
        Self: 'a;

    type FindStreamsBySessionFuture<'a>
        = impl Future<Output = DomainResult<Vec<Stream>>> + Send + 'a
    where
        Self: 'a;

    type UpdateStreamStatusFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type GetStreamStatisticsFuture<'a>
        = impl Future<Output = DomainResult<StreamStatistics>> + Send + 'a
    where
        Self: 'a;

    fn store_stream(&self, stream: Stream) -> Self::StoreStreamFuture<'_> {
        async move {
            self.store.insert(stream.id(), stream);
            Ok(())
        }
    }

    fn get_stream(&self, stream_id: StreamId) -> Self::GetStreamFuture<'_> {
        async move { Ok(self.store.get(&stream_id)) }
    }

    fn delete_stream(&self, stream_id: StreamId) -> Self::DeleteStreamFuture<'_> {
        async move {
            self.store.remove(&stream_id);
            Ok(())
        }
    }

    fn list_streams_for_session(
        &self,
        session_id: SessionId,
    ) -> Self::ListStreamsForSessionFuture<'_> {
        async move { Ok(self.store.filter(|s| s.session_id() == session_id)) }
    }

    fn find_streams_by_session(
        &self,
        session_id: SessionId,
        filter: StreamFilter,
    ) -> Self::FindStreamsBySessionFuture<'_> {
        async move {
            let streams: Vec<Stream> = self.store.filter(|stream| {
                // First check session membership
                if stream.session_id() != session_id {
                    return false;
                }

                // Apply status filter
                if let Some(statuses) = &filter.statuses {
                    let matches_status = statuses
                        .iter()
                        .any(|status| Self::stream_state_matches_status(stream.state(), status));
                    if !matches_status {
                        return false;
                    }
                }

                // Apply time filter
                if let Some(after) = filter.created_after
                    && stream.created_at() < after
                {
                    return false;
                }

                // Apply has_frames filter
                if let Some(has_frames) = filter.has_frames {
                    let frame_count = stream.stats().total_frames;
                    if has_frames && frame_count == 0 {
                        return false;
                    }
                    if !has_frames && frame_count > 0 {
                        return false;
                    }
                }

                true
            });

            Ok(streams)
        }
    }

    fn update_stream_status(
        &self,
        stream_id: StreamId,
        status: StreamStatus,
    ) -> Self::UpdateStreamStatusFuture<'_> {
        async move {
            use crate::domain::DomainError;

            self.store
                .update_with(&stream_id, |stream| {
                    // Apply status transition based on requested status
                    match status {
                        StreamStatus::Active => stream.start_streaming(),
                        StreamStatus::Completed => stream.complete(),
                        StreamStatus::Failed => stream.fail("Status update to Failed".to_string()),
                        StreamStatus::Cancelled => stream.cancel(),
                        StreamStatus::Paused => {
                            // Paused not directly supported by Stream entity
                            Ok(())
                        }
                        StreamStatus::Created => {
                            // Cannot transition to Created
                            Err(DomainError::InvalidStateTransition(
                                "Cannot transition to Created status".to_string(),
                            ))
                        }
                    }
                })
                .unwrap_or(Ok(())) // Stream not found - idempotent behavior
        }
    }

    fn get_stream_statistics(&self, stream_id: StreamId) -> Self::GetStreamStatisticsFuture<'_> {
        async move {
            match self.store.get(&stream_id) {
                Some(stream) => {
                    let stats = stream.stats();

                    // Build PriorityDistribution from stream stats
                    let priority_dist = PriorityDistribution {
                        critical_frames: stats.skeleton_frames
                            + stats.complete_frames
                            + stats.error_frames,
                        high_frames: if stats.average_frame_size > 0.0 {
                            (stats.high_priority_bytes as f64 / stats.average_frame_size) as u64
                        } else {
                            0
                        },
                        medium_frames: 0, // Would need to track in StreamStats
                        low_frames: 0,
                        background_frames: 0,
                    };

                    Ok(StreamStatistics {
                        total_frames: stats.total_frames,
                        total_bytes: stats.total_bytes,
                        priority_distribution: priority_dist,
                        avg_frame_size: stats.average_frame_size,
                        creation_time: stream.created_at(),
                        completion_time: stream.completed_at(),
                        processing_duration: stream.duration().map(|d| {
                            std::time::Duration::from_millis(
                                d.num_milliseconds().try_into().unwrap_or(0),
                            )
                        }),
                    })
                }
                None => {
                    // Return default statistics for non-existent stream
                    Ok(StreamStatistics {
                        total_frames: 0,
                        total_bytes: 0,
                        priority_distribution: PriorityDistribution::default(),
                        avg_frame_size: 0.0,
                        creation_time: Utc::now(),
                        completion_time: None,
                        processing_duration: None,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        aggregates::stream_session::SessionConfig, entities::stream::StreamConfig,
        value_objects::JsonData,
    };
    use chrono::{Duration, Utc};

    // ===== Basic CRUD Tests =====

    #[tokio::test]
    async fn test_gat_repository_crud() {
        let repo = GatInMemoryStreamRepository::new();

        let session = StreamSession::new(SessionConfig::default());
        let session_id = session.id();

        repo.save_session(session.clone()).await.unwrap();

        let found = repo.find_session(session_id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id(), session_id);

        repo.remove_session(session_id).await.unwrap();
        let not_found = repo.find_session(session_id).await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_gat_store_crud() {
        let store = GatInMemoryStreamStore::new();

        assert_eq!(store.stream_count(), 0);
        store.clear();
        assert_eq!(store.stream_count(), 0);
    }

    // ===== StreamRepositoryGat Tests: find_sessions_by_criteria =====

    #[tokio::test]
    async fn test_find_sessions_by_criteria_empty() {
        let repo = GatInMemoryStreamRepository::new();

        // Empty criteria should return all sessions
        let criteria = SessionQueryCriteria::default();
        let pagination = Pagination::default();

        let result = repo
            .find_sessions_by_criteria(criteria, pagination)
            .await
            .unwrap();

        assert_eq!(result.sessions.len(), 0);
        assert_eq!(result.total_count, 0);
        assert!(!result.has_more);
    }

    #[tokio::test]
    async fn test_find_sessions_by_criteria_state_filter() {
        let repo = GatInMemoryStreamRepository::new();

        // Create sessions in different states
        let mut active_session = StreamSession::new(SessionConfig::default());
        active_session.activate().unwrap();
        let active_id = active_session.id();
        repo.save_session(active_session).await.unwrap();

        let inactive_session = StreamSession::new(SessionConfig::default());
        repo.save_session(inactive_session).await.unwrap();

        // Filter by Active state
        let criteria = SessionQueryCriteria {
            states: Some(vec!["Active".to_string()]),
            ..Default::default()
        };
        let pagination = Pagination::default();

        let result = repo
            .find_sessions_by_criteria(criteria, pagination)
            .await
            .unwrap();

        assert_eq!(result.total_count, 1);
        assert_eq!(result.sessions[0].id(), active_id);
    }

    #[tokio::test]
    async fn test_find_sessions_by_criteria_time_range() {
        let repo = GatInMemoryStreamRepository::new();

        // Create a session
        let mut session = StreamSession::new(SessionConfig::default());
        session.activate().unwrap();
        repo.save_session(session).await.unwrap();

        // Filter by time range - should find the session
        let now = Utc::now();
        let criteria = SessionQueryCriteria {
            created_after: Some(now - Duration::hours(1)),
            created_before: Some(now + Duration::hours(1)),
            ..Default::default()
        };
        let pagination = Pagination::default();

        let result = repo
            .find_sessions_by_criteria(criteria, pagination)
            .await
            .unwrap();

        assert_eq!(result.total_count, 1);

        // Filter by future time range - should find nothing
        let criteria_future = SessionQueryCriteria {
            created_after: Some(now + Duration::hours(1)),
            ..Default::default()
        };
        let result_future = repo
            .find_sessions_by_criteria(criteria_future, Pagination::default())
            .await
            .unwrap();

        assert_eq!(result_future.total_count, 0);
    }

    #[tokio::test]
    async fn test_find_sessions_by_criteria_stream_count() {
        let repo = GatInMemoryStreamRepository::new();

        // Create session with streams
        let mut session_with_streams = StreamSession::new(SessionConfig::default());
        session_with_streams.activate().unwrap();
        session_with_streams
            .create_stream(JsonData::String("test1".to_string()))
            .unwrap();
        session_with_streams
            .create_stream(JsonData::String("test2".to_string()))
            .unwrap();
        let session_with_streams_id = session_with_streams.id();
        repo.save_session(session_with_streams).await.unwrap();

        // Create session without streams
        let mut session_no_streams = StreamSession::new(SessionConfig::default());
        session_no_streams.activate().unwrap();
        repo.save_session(session_no_streams).await.unwrap();

        // Filter by min_stream_count
        let criteria = SessionQueryCriteria {
            min_stream_count: Some(2),
            ..Default::default()
        };
        let result = repo
            .find_sessions_by_criteria(criteria, Pagination::default())
            .await
            .unwrap();

        assert_eq!(result.total_count, 1);
        assert_eq!(result.sessions[0].id(), session_with_streams_id);

        // Filter by max_stream_count
        let criteria_max = SessionQueryCriteria {
            max_stream_count: Some(1),
            ..Default::default()
        };
        let result_max = repo
            .find_sessions_by_criteria(criteria_max, Pagination::default())
            .await
            .unwrap();

        assert_eq!(result_max.total_count, 1);
    }

    #[tokio::test]
    async fn test_find_sessions_by_criteria_pagination() {
        let repo = GatInMemoryStreamRepository::new();

        // Create 5 sessions
        for _ in 0..5 {
            let mut session = StreamSession::new(SessionConfig::default());
            session.activate().unwrap();
            repo.save_session(session).await.unwrap();
        }

        // Test offset and limit
        let pagination = Pagination {
            offset: 2,
            limit: 2,
            ..Default::default()
        };

        let result = repo
            .find_sessions_by_criteria(SessionQueryCriteria::default(), pagination)
            .await
            .unwrap();

        assert_eq!(result.sessions.len(), 2);
        assert_eq!(result.total_count, 5);
        assert!(result.has_more); // 2 + 2 < 5
    }

    #[tokio::test]
    async fn test_find_sessions_by_criteria_sorting() {
        let repo = GatInMemoryStreamRepository::new();

        // Create sessions with different stream counts
        let mut session1 = StreamSession::new(SessionConfig::default());
        session1.activate().unwrap();
        session1
            .create_stream(JsonData::String("s1".to_string()))
            .unwrap();
        repo.save_session(session1).await.unwrap();

        let mut session2 = StreamSession::new(SessionConfig::default());
        session2.activate().unwrap();
        session2
            .create_stream(JsonData::String("s2".to_string()))
            .unwrap();
        session2
            .create_stream(JsonData::String("s3".to_string()))
            .unwrap();
        session2
            .create_stream(JsonData::String("s4".to_string()))
            .unwrap();
        let session2_id = session2.id();
        repo.save_session(session2).await.unwrap();

        // Sort by stream_count descending
        let pagination = Pagination {
            sort_by: Some("stream_count".to_string()),
            sort_order: SortOrder::Descending,
            ..Default::default()
        };

        let result = repo
            .find_sessions_by_criteria(SessionQueryCriteria::default(), pagination)
            .await
            .unwrap();

        assert_eq!(result.sessions.len(), 2);
        assert_eq!(result.sessions[0].id(), session2_id); // Session with most streams first
    }

    // ===== StreamRepositoryGat Tests: get_session_health =====

    #[tokio::test]
    async fn test_get_session_health_existing() {
        let repo = GatInMemoryStreamRepository::new();

        let mut session = StreamSession::new(SessionConfig::default());
        session.activate().unwrap();
        let session_id = session.id();
        repo.save_session(session).await.unwrap();

        let health = repo.get_session_health(session_id).await.unwrap();

        assert_eq!(health.session_id, session_id);
        assert!(health.is_healthy);
        assert_eq!(health.active_streams, 0);
        assert!(health.error_rate == 0.0);
    }

    #[tokio::test]
    async fn test_get_session_health_not_found() {
        let repo = GatInMemoryStreamRepository::new();

        let missing_session_id = SessionId::new();
        let health = repo.get_session_health(missing_session_id).await.unwrap();

        assert_eq!(health.session_id, missing_session_id);
        assert!(!health.is_healthy);
        assert_eq!(health.active_streams, 0);
        assert_eq!(health.total_frames, 0);
    }

    // ===== StreamRepositoryGat Tests: session_exists =====

    #[tokio::test]
    async fn test_session_exists_true() {
        let repo = GatInMemoryStreamRepository::new();

        let session = StreamSession::new(SessionConfig::default());
        let session_id = session.id();
        repo.save_session(session).await.unwrap();

        let exists = repo.session_exists(session_id).await.unwrap();

        assert!(exists);
    }

    #[tokio::test]
    async fn test_session_exists_false() {
        let repo = GatInMemoryStreamRepository::new();

        let missing_session_id = SessionId::new();
        let exists = repo.session_exists(missing_session_id).await.unwrap();

        assert!(!exists);
    }

    // ===== StreamStoreGat Tests: find_streams_by_session =====

    #[tokio::test]
    async fn test_find_streams_by_session_empty() {
        let store = GatInMemoryStreamStore::new();

        let session_id = SessionId::new();
        let filter = StreamFilter::default();

        let result = store
            .find_streams_by_session(session_id, filter)
            .await
            .unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_find_streams_by_session_status_filter() {
        let store = GatInMemoryStreamStore::new();

        let session_id = SessionId::new();

        // Create a stream in Preparing state
        let stream = Stream::new(
            session_id,
            JsonData::String("test".to_string()),
            StreamConfig::default(),
        );
        let stream_id = stream.id();
        store.store_stream(stream).await.unwrap();

        // Filter by Created status (matches Preparing state)
        let filter = StreamFilter {
            statuses: Some(vec![StreamStatus::Created]),
            ..Default::default()
        };

        let result = store
            .find_streams_by_session(session_id, filter)
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id(), stream_id);

        // Filter by Active status (should not match Preparing)
        let filter_active = StreamFilter {
            statuses: Some(vec![StreamStatus::Active]),
            ..Default::default()
        };

        let result_active = store
            .find_streams_by_session(session_id, filter_active)
            .await
            .unwrap();

        assert!(result_active.is_empty());
    }

    #[tokio::test]
    async fn test_find_streams_by_session_time_filter() {
        let store = GatInMemoryStreamStore::new();

        let session_id = SessionId::new();
        let stream = Stream::new(
            session_id,
            JsonData::String("test".to_string()),
            StreamConfig::default(),
        );
        store.store_stream(stream).await.unwrap();

        let now = Utc::now();

        // Filter by created_after in the past - should find stream
        let filter = StreamFilter {
            created_after: Some(now - Duration::hours(1)),
            ..Default::default()
        };

        let result = store
            .find_streams_by_session(session_id, filter)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);

        // Filter by created_after in the future - should find nothing
        let filter_future = StreamFilter {
            created_after: Some(now + Duration::hours(1)),
            ..Default::default()
        };

        let result_future = store
            .find_streams_by_session(session_id, filter_future)
            .await
            .unwrap();
        assert!(result_future.is_empty());
    }

    #[tokio::test]
    async fn test_find_streams_by_session_has_frames() {
        let store = GatInMemoryStreamStore::new();

        let session_id = SessionId::new();
        let stream = Stream::new(
            session_id,
            JsonData::String("test".to_string()),
            StreamConfig::default(),
        );
        store.store_stream(stream).await.unwrap();

        // Filter by has_frames = false (new stream has no frames)
        let filter = StreamFilter {
            has_frames: Some(false),
            ..Default::default()
        };

        let result = store
            .find_streams_by_session(session_id, filter)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);

        // Filter by has_frames = true (should find nothing)
        let filter_with_frames = StreamFilter {
            has_frames: Some(true),
            ..Default::default()
        };

        let result_with_frames = store
            .find_streams_by_session(session_id, filter_with_frames)
            .await
            .unwrap();
        assert!(result_with_frames.is_empty());
    }

    // ===== StreamStoreGat Tests: update_stream_status =====

    #[tokio::test]
    async fn test_update_stream_status_to_active() {
        let store = GatInMemoryStreamStore::new();

        let session_id = SessionId::new();
        let stream = Stream::new(
            session_id,
            JsonData::String("test".to_string()),
            StreamConfig::default(),
        );
        let stream_id = stream.id();
        store.store_stream(stream).await.unwrap();

        store
            .update_stream_status(stream_id, StreamStatus::Active)
            .await
            .unwrap();

        let updated = store.get_stream(stream_id).await.unwrap().unwrap();
        assert!(matches!(updated.state(), StreamState::Streaming));
    }

    #[tokio::test]
    async fn test_update_stream_status_to_completed() {
        let store = GatInMemoryStreamStore::new();

        let session_id = SessionId::new();
        let mut stream = Stream::new(
            session_id,
            JsonData::String("test".to_string()),
            StreamConfig::default(),
        );
        stream.start_streaming().unwrap(); // Must be streaming to complete
        let stream_id = stream.id();
        store.store_stream(stream).await.unwrap();

        store
            .update_stream_status(stream_id, StreamStatus::Completed)
            .await
            .unwrap();

        let updated = store.get_stream(stream_id).await.unwrap().unwrap();
        assert!(matches!(updated.state(), StreamState::Completed));
    }

    #[tokio::test]
    async fn test_update_stream_status_not_found() {
        let store = GatInMemoryStreamStore::new();

        let missing_stream_id = StreamId::new();

        // Should be idempotent - no error for missing stream
        let result = store
            .update_stream_status(missing_stream_id, StreamStatus::Active)
            .await;

        assert!(result.is_ok());
    }

    // ===== StreamStoreGat Tests: get_stream_statistics =====

    #[tokio::test]
    async fn test_get_stream_statistics_existing() {
        let store = GatInMemoryStreamStore::new();

        let session_id = SessionId::new();
        let stream = Stream::new(
            session_id,
            JsonData::String("test".to_string()),
            StreamConfig::default(),
        );
        let stream_id = stream.id();
        store.store_stream(stream).await.unwrap();

        let stats = store.get_stream_statistics(stream_id).await.unwrap();

        assert_eq!(stats.total_frames, 0);
        assert_eq!(stats.total_bytes, 0);
        assert!(stats.completion_time.is_none());
        assert!(stats.processing_duration.is_none());
    }

    #[tokio::test]
    async fn test_get_stream_statistics_not_found() {
        let store = GatInMemoryStreamStore::new();

        let missing_stream_id = StreamId::new();
        let stats = store
            .get_stream_statistics(missing_stream_id)
            .await
            .unwrap();

        // Should return default statistics for non-existent stream
        assert_eq!(stats.total_frames, 0);
        assert_eq!(stats.total_bytes, 0);
        assert!(stats.completion_time.is_none());
    }
}
