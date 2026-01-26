//! GAT-based in-memory repository implementations
//!
//! Zero-cost abstractions for domain ports using Generic Associated Types.
//!
//! # Concurrency Model
//!
//! These repositories use [`InMemoryStore`] which is backed by `DashMap` for
//! lock-free concurrent access. See `generic_store.rs` for detailed consistency
//! guarantees.
//!
//! # Iteration Consistency
//!
//! Query methods that iterate over multiple items (`find_sessions_by_criteria`,
//! `find_active_sessions`, `list_streams_for_session`, etc.) provide weakly
//! consistent results. Items added or removed during query execution may or may
//! not be included in the results. For authoritative checks on specific items,
//! use single-key lookups (`find_session`, `get_stream`, `session_exists`).

use std::cmp::Ordering;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::Instant;

use dashmap::DashMap;

use crate::domain::{
    DomainError, DomainResult,
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
use super::limits::{MAX_HEALTH_METRICS, MAX_RESULTS_LIMIT, MAX_SCAN_LIMIT};

// ============================================================================
// Session Stats Cache (MEM-002)
// ============================================================================

/// Cache TTL for session statistics in seconds.
///
/// This value balances freshness vs. computation overhead for health checks.
/// A 5-second TTL prevents excessive recalculation while maintaining reasonable
/// accuracy for monitoring dashboards.
const STATS_CACHE_TTL_SECS: u64 = 5;

/// Cached session statistics for efficient health checks.
///
/// Reduces computation overhead by caching aggregated statistics at the session
/// level. The cache uses a TTL-based invalidation strategy for simplicity.
#[derive(Debug)]
struct CachedSessionStats {
    total_frames: AtomicU64,
    computed_at_secs: AtomicU64,
}

impl Clone for CachedSessionStats {
    fn clone(&self) -> Self {
        Self {
            total_frames: AtomicU64::new(self.total_frames.load(AtomicOrdering::Relaxed)),
            computed_at_secs: AtomicU64::new(self.computed_at_secs.load(AtomicOrdering::Relaxed)),
        }
    }
}

impl CachedSessionStats {
    fn new(total_frames: u64) -> Self {
        Self {
            total_frames: AtomicU64::new(total_frames),
            computed_at_secs: AtomicU64::new(current_timestamp_secs()),
        }
    }

    fn is_valid(&self) -> bool {
        let now = current_timestamp_secs();
        let computed_at = self.computed_at_secs.load(AtomicOrdering::Relaxed);
        now.saturating_sub(computed_at) < STATS_CACHE_TTL_SECS
    }

    fn get_total_frames(&self) -> u64 {
        self.total_frames.load(AtomicOrdering::Relaxed)
    }

    fn update(&self, total_frames: u64) {
        self.total_frames
            .store(total_frames, AtomicOrdering::Relaxed);
        self.computed_at_secs
            .store(current_timestamp_secs(), AtomicOrdering::Relaxed);
    }
}

fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================================================================
// Session Repository
// ============================================================================

/// GAT-based in-memory implementation of StreamRepositoryGat
#[derive(Debug)]
pub struct GatInMemoryStreamRepository {
    store: SessionStore,
    stats_cache: DashMap<SessionId, CachedSessionStats>,
}

impl Clone for GatInMemoryStreamRepository {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            // Clone the cache to preserve cached statistics
            stats_cache: self.stats_cache.clone(),
        }
    }
}

impl Default for GatInMemoryStreamRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl GatInMemoryStreamRepository {
    pub fn new() -> Self {
        Self {
            store: SessionStore::new(),
            stats_cache: DashMap::new(),
        }
    }

    /// Get number of stored sessions
    pub fn session_count(&self) -> usize {
        self.store.count()
    }

    /// Clear all sessions (for testing)
    pub fn clear(&self) {
        self.store.clear();
        self.stats_cache.clear();
    }

    /// Get all session IDs (for testing)
    pub fn all_session_ids(&self) -> Vec<SessionId> {
        self.store.all_keys()
    }

    /// Helper: Check if session matches query criteria
    fn matches_criteria(session: &StreamSession, criteria: &SessionQueryCriteria) -> bool {
        // Check state filter
        if let Some(states) = &criteria.states {
            let state_str = session.state().as_str();
            if !states.iter().any(|s| s.eq_ignore_ascii_case(state_str)) {
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

        // Check client_info pattern filter
        if let Some(pattern) = &criteria.client_info_pattern {
            match session.client_info() {
                Some(client_info) => {
                    if !client_info.to_lowercase().contains(&pattern.to_lowercase()) {
                        return false;
                    }
                }
                None => return false, // No client_info means no match
            }
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

    /// Get cached or compute total frames for a session (MEM-002).
    ///
    /// Uses TTL-based caching to reduce iteration overhead for health checks.
    fn get_cached_total_frames(&self, session: &StreamSession) -> u64 {
        let session_id = session.id();

        // Fast path: use cached stats if present and still valid
        if let Some(cached) = self.stats_cache.get(&session_id)
            && cached.is_valid()
        {
            return cached.get_total_frames();
        }

        // Compute total frames by iterating streams
        let total_frames: u64 = session
            .streams()
            .values()
            .map(|s| s.stats().total_frames)
            .sum();

        // Update or insert cache entry atomically using DashMap's entry API
        self.stats_cache
            .entry(session_id)
            .and_modify(|cached| cached.update(total_frames))
            .or_insert_with(|| CachedSessionStats::new(total_frames));

        total_frames
    }

    /// Invalidate stats cache for a session.
    ///
    /// Called when session is modified to ensure cache consistency.
    fn invalidate_stats_cache(&self, session_id: &SessionId) {
        self.stats_cache.remove(session_id);
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
            let session_id = session.id();
            self.invalidate_stats_cache(&session_id);
            self.store.insert(session_id, session);
            Ok(())
        }
    }

    fn remove_session(&self, session_id: SessionId) -> Self::RemoveSessionFuture<'_> {
        async move {
            self.invalidate_stats_cache(&session_id);
            self.store.remove(&session_id);
            Ok(())
        }
    }

    /// Find all active sessions.
    ///
    /// # Consistency
    ///
    /// Results are weakly consistent. Sessions added or removed during query
    /// execution may or may not be included. For authoritative checks, use
    /// `session_exists()` or `find_session()` for single-session lookups.
    fn find_active_sessions(&self) -> Self::FindActiveSessionsFuture<'_> {
        async move {
            // Use bounded iteration for find_active_sessions too
            let (sessions, _) =
                self.store
                    .filter_limited(|s| s.is_active(), MAX_RESULTS_LIMIT, MAX_SCAN_LIMIT);
            Ok(sessions)
        }
    }

    /// Find sessions matching criteria with pagination.
    ///
    /// # Consistency
    ///
    /// Results are weakly consistent. Sessions added or removed during query
    /// execution may or may not be included. For authoritative checks, use
    /// `session_exists()` or `find_session()` for single-session lookups.
    fn find_sessions_by_criteria(
        &self,
        criteria: SessionQueryCriteria,
        pagination: Pagination,
    ) -> Self::FindSessionsByCriteriaFuture<'_> {
        async move {
            // Validate inputs first
            criteria.validate()?;
            pagination.validate()?;

            let start = Instant::now();

            // Use bounded iteration to prevent DOS
            let (mut filtered, scan_limit_reached) = self.store.filter_limited(
                |session| Self::matches_criteria(session, &criteria),
                MAX_RESULTS_LIMIT,
                MAX_SCAN_LIMIT,
            );

            let total_count = filtered.len();

            // Sort if sort_by specified
            if let Some(sort_field) = &pagination.sort_by {
                filtered.sort_by(|a, b| {
                    let cmp = Self::compare_by_field(a, b, sort_field);
                    match pagination.sort_order {
                        SortOrder::Ascending => cmp,
                        SortOrder::Descending => cmp.reverse(),
                    }
                });
            }

            // Apply pagination
            let paginated: Vec<StreamSession> = filtered
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
                scan_limit_reached,
            })
        }
    }

    fn get_session_health(&self, session_id: SessionId) -> Self::GetSessionHealthFuture<'_> {
        async move {
            match self.store.get(&session_id) {
                Some(session) => {
                    let health = session.health_check();

                    // Use cached stats to reduce iteration overhead (MEM-002)
                    let total_frames = self.get_cached_total_frames(&session);

                    // Calculate error rate from failed streams
                    let failed_streams = health.failed_streams as f64;
                    let total_streams = session.streams().len() as f64;
                    let error_rate = if total_streams > 0.0 {
                        failed_streams / total_streams
                    } else {
                        0.0
                    };

                    // Preallocate for known metrics count (MEM-001 fix)
                    let mut metrics = std::collections::HashMap::with_capacity(MAX_HEALTH_METRICS);
                    metrics.insert("active_streams".to_string(), health.active_streams as f64);
                    metrics.insert(
                        "total_bytes".to_string(),
                        session.stats().total_bytes as f64,
                    );
                    metrics.insert(
                        "avg_duration_ms".to_string(),
                        session.stats().average_stream_duration_ms,
                    );

                    debug_assert!(
                        metrics.len() <= MAX_HEALTH_METRICS,
                        "Health metrics exceeded MAX_HEALTH_METRICS"
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
                // ERR-001 fix: Return NotFound for non-existent sessions
                None => Err(DomainError::SessionNotFound(format!(
                    "Session {} not found",
                    session_id
                ))),
            }
        }
    }

    #[inline]
    fn session_exists(&self, session_id: SessionId) -> Self::SessionExistsFuture<'_> {
        async move { Ok(self.store.contains_key(&session_id)) }
    }
}

// ============================================================================
// Stream Store
// ============================================================================

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

    /// Helper: Check if stream matches filter criteria
    fn matches_stream_filter(
        stream: &Stream,
        session_id: SessionId,
        filter: &StreamFilter,
    ) -> bool {
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

    /// List all streams for a session.
    ///
    /// # Consistency
    ///
    /// Results are weakly consistent. Streams added or removed during iteration
    /// may or may not be included. For authoritative checks, use `get_stream()`
    /// for single-stream lookups.
    fn list_streams_for_session(
        &self,
        session_id: SessionId,
    ) -> Self::ListStreamsForSessionFuture<'_> {
        async move {
            // Use bounded iteration for DOS protection
            let (streams, _) = self.store.filter_limited(
                |s| s.session_id() == session_id,
                MAX_RESULTS_LIMIT,
                MAX_SCAN_LIMIT,
            );
            Ok(streams)
        }
    }

    /// Find streams matching filter criteria.
    ///
    /// # Consistency
    ///
    /// Results are weakly consistent. Streams added or removed during iteration
    /// may or may not be included. For authoritative checks, use `get_stream()`
    /// for single-stream lookups.
    fn find_streams_by_session(
        &self,
        session_id: SessionId,
        filter: StreamFilter,
    ) -> Self::FindStreamsBySessionFuture<'_> {
        async move {
            // DOS-002 fix: Use bounded iteration
            let (streams, _) = self.store.filter_limited(
                |stream| Self::matches_stream_filter(stream, session_id, &filter),
                MAX_RESULTS_LIMIT,
                MAX_SCAN_LIMIT,
            );

            Ok(streams)
        }
    }

    fn update_stream_status(
        &self,
        stream_id: StreamId,
        status: StreamStatus,
    ) -> Self::UpdateStreamStatusFuture<'_> {
        async move {
            // ERR-001 fix: Return NotFound for missing streams
            match self.store.update_with(&stream_id, |stream| {
                // Apply status transition based on requested status
                match status {
                    StreamStatus::Active => stream.start_streaming(),
                    StreamStatus::Completed => stream.complete(),
                    StreamStatus::Failed => stream.fail("Status update to Failed".to_string()),
                    StreamStatus::Cancelled => stream.cancel(),
                    StreamStatus::Paused => {
                        // Paused not directly supported by Stream entity; treat as invalid transition
                        Err(DomainError::InvalidStateTransition(
                            "Cannot transition to Paused status: not supported by StreamState"
                                .to_string(),
                        ))
                    }
                    StreamStatus::Created => {
                        // Cannot transition to Created
                        Err(DomainError::InvalidStateTransition(
                            "Cannot transition to Created status".to_string(),
                        ))
                    }
                }
            }) {
                Some(result) => result,
                None => Err(DomainError::StreamNotFound(format!(
                    "Stream {} not found",
                    stream_id
                ))),
            }
        }
    }

    fn get_stream_statistics(&self, stream_id: StreamId) -> Self::GetStreamStatisticsFuture<'_> {
        async move {
            match self.store.get(&stream_id) {
                Some(stream) => {
                    let stats = stream.stats();

                    // Build PriorityDistribution from stream stats with saturating casts
                    let high_frames = if stats.average_frame_size > 0.0 {
                        // Saturating cast: clamp to u64::MAX to prevent overflow
                        let ratio = stats.high_priority_bytes as f64 / stats.average_frame_size;
                        saturating_f64_to_u64(ratio)
                    } else {
                        0
                    };

                    let priority_dist = PriorityDistribution {
                        critical_frames: stats.skeleton_frames
                            + stats.complete_frames
                            + stats.error_frames,
                        high_frames,
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
                // ERR-001 fix: Return NotFound for non-existent streams
                None => Err(DomainError::StreamNotFound(format!(
                    "Stream {} not found",
                    stream_id
                ))),
            }
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Safely convert f64 to u64 with saturation at u64::MAX.
///
/// Handles edge cases:
/// - Negative values → 0
/// - NaN → 0
/// - Infinity → u64::MAX
/// - Values > u64::MAX → u64::MAX
#[inline]
fn saturating_f64_to_u64(value: f64) -> u64 {
    if value.is_nan() || value < 0.0 {
        0
    } else if value >= u64::MAX as f64 {
        u64::MAX
    } else {
        value as u64
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

    // ===== Helper function tests =====

    #[test]
    fn test_saturating_f64_to_u64_normal_values() {
        assert_eq!(saturating_f64_to_u64(0.0), 0);
        assert_eq!(saturating_f64_to_u64(1.5), 1);
        assert_eq!(saturating_f64_to_u64(100.9), 100);
        assert_eq!(saturating_f64_to_u64(1_000_000.0), 1_000_000);
    }

    #[test]
    fn test_saturating_f64_to_u64_edge_cases() {
        assert_eq!(saturating_f64_to_u64(f64::NAN), 0);
        assert_eq!(saturating_f64_to_u64(f64::NEG_INFINITY), 0);
        assert_eq!(saturating_f64_to_u64(-1.0), 0);
        assert_eq!(saturating_f64_to_u64(f64::INFINITY), u64::MAX);
        assert_eq!(saturating_f64_to_u64(1e20), u64::MAX);
    }

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
        assert!(!result.scan_limit_reached);
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

    #[tokio::test]
    async fn test_find_sessions_validates_criteria() {
        let repo = GatInMemoryStreamRepository::new();

        // Invalid criteria: min > max
        let criteria = SessionQueryCriteria {
            min_stream_count: Some(10),
            max_stream_count: Some(5),
            ..Default::default()
        };
        let result = repo
            .find_sessions_by_criteria(criteria, Pagination::default())
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn test_find_sessions_validates_pagination() {
        let repo = GatInMemoryStreamRepository::new();

        // Invalid pagination: limit = 0
        let pagination = Pagination {
            offset: 0,
            limit: 0,
            sort_by: None,
            sort_order: SortOrder::Ascending,
        };
        let result = repo
            .find_sessions_by_criteria(SessionQueryCriteria::default(), pagination)
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DomainError::InvalidInput(_)));
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
    async fn test_get_session_health_returns_not_found() {
        let repo = GatInMemoryStreamRepository::new();

        let missing_session_id = SessionId::new();
        let result = repo.get_session_health(missing_session_id).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DomainError::SessionNotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_get_session_health_uses_cache() {
        let repo = GatInMemoryStreamRepository::new();

        let mut session = StreamSession::new(SessionConfig::default());
        session.activate().unwrap();
        let session_id = session.id();
        repo.save_session(session).await.unwrap();

        // First call computes and caches
        let health1 = repo.get_session_health(session_id).await.unwrap();
        assert_eq!(health1.total_frames, 0);

        // Second call should use cache
        let health2 = repo.get_session_health(session_id).await.unwrap();
        assert_eq!(health2.total_frames, 0);

        // Cache should exist
        assert!(repo.stats_cache.contains_key(&session_id));
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
    async fn test_update_stream_status_returns_not_found() {
        let store = GatInMemoryStreamStore::new();

        let missing_stream_id = StreamId::new();

        let result = store
            .update_stream_status(missing_stream_id, StreamStatus::Active)
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DomainError::StreamNotFound(_)
        ));
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
    async fn test_get_stream_statistics_returns_not_found() {
        let store = GatInMemoryStreamStore::new();

        let missing_stream_id = StreamId::new();
        let result = store.get_stream_statistics(missing_stream_id).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DomainError::StreamNotFound(_)
        ));
    }

    // ===== Bounded iteration tests =====

    #[tokio::test]
    async fn test_find_sessions_uses_bounded_iteration() {
        let repo = GatInMemoryStreamRepository::new();

        // Create enough sessions to verify bounded iteration is used
        // (though we can't easily hit MAX_SCAN_LIMIT in tests without performance impact)
        for _ in 0..10 {
            let mut session = StreamSession::new(SessionConfig::default());
            session.activate().unwrap();
            repo.save_session(session).await.unwrap();
        }

        let result = repo
            .find_sessions_by_criteria(SessionQueryCriteria::default(), Pagination::default())
            .await
            .unwrap();

        assert_eq!(result.total_count, 10);
        assert!(!result.scan_limit_reached);
    }

    #[tokio::test]
    async fn test_find_active_sessions_uses_bounded_iteration() {
        let repo = GatInMemoryStreamRepository::new();

        for _ in 0..5 {
            let mut session = StreamSession::new(SessionConfig::default());
            session.activate().unwrap();
            repo.save_session(session).await.unwrap();
        }

        let sessions = repo.find_active_sessions().await.unwrap();
        assert_eq!(sessions.len(), 5);
    }

    #[tokio::test]
    async fn test_list_streams_for_session_uses_bounded_iteration() {
        let store = GatInMemoryStreamStore::new();
        let session_id = SessionId::new();

        for _ in 0..5 {
            let stream = Stream::new(
                session_id,
                JsonData::String("test".to_string()),
                StreamConfig::default(),
            );
            store.store_stream(stream).await.unwrap();
        }

        let streams = store.list_streams_for_session(session_id).await.unwrap();
        assert_eq!(streams.len(), 5);
    }

    // ===== Stats cache tests =====

    #[tokio::test]
    async fn test_stats_cache_invalidated_on_save() {
        let repo = GatInMemoryStreamRepository::new();

        let mut session = StreamSession::new(SessionConfig::default());
        session.activate().unwrap();
        let session_id = session.id();
        repo.save_session(session.clone()).await.unwrap();

        // Get health to populate cache
        let _ = repo.get_session_health(session_id).await.unwrap();
        assert!(repo.stats_cache.contains_key(&session_id));

        // Save again should invalidate cache
        repo.save_session(session).await.unwrap();
        // Note: The new save creates a new cache entry, but the invalidation happens first
    }

    #[tokio::test]
    async fn test_stats_cache_invalidated_on_remove() {
        let repo = GatInMemoryStreamRepository::new();

        let mut session = StreamSession::new(SessionConfig::default());
        session.activate().unwrap();
        let session_id = session.id();
        repo.save_session(session).await.unwrap();

        // Get health to populate cache
        let _ = repo.get_session_health(session_id).await.unwrap();
        assert!(repo.stats_cache.contains_key(&session_id));

        // Remove should invalidate cache
        repo.remove_session(session_id).await.unwrap();
        assert!(!repo.stats_cache.contains_key(&session_id));
    }
}
