//! Repository ports for data persistence
//!
//! Supporting types for repository operations. GAT trait definitions are in `super::gat`.
//!
//! # Migration Note
//!
//! async_trait-based repository traits have been removed. Use GAT equivalents from `super::gat`:
//! - `StreamRepositoryGat` - Session storage
//! - `StreamStoreGat` - Stream storage
//! - `FrameRepositoryGat` - Frame storage
//! - `EventStoreGat` - Event sourcing
//! - `CacheGat` - Caching operations

use crate::domain::{
    DomainResult,
    aggregates::StreamSession,
    entities::Frame,
    value_objects::{Priority, SessionId},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Supporting Types
// ============================================================================

// async_trait traits removed - use GAT traits from super::gat instead

/// Helper extension trait for type-safe cache operations
/// This can be used as extension methods on implementers
#[allow(async_fn_in_trait)]
pub trait CacheExtensions: super::gat::CacheGat {
    /// Get cached value with deserialization
    async fn get_typed<T>(&self, key: &str) -> DomainResult<Option<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        if let Some(bytes) = self.get_bytes(key).await? {
            let value = serde_json::from_slice(&bytes).map_err(|e| {
                crate::domain::DomainError::Logic(format!("Deserialization failed: {e}"))
            })?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    /// Set cached value with serialization
    async fn set_typed<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<std::time::Duration>,
    ) -> DomainResult<()>
    where
        T: serde::Serialize,
    {
        let bytes = serde_json::to_vec(value)
            .map_err(|e| crate::domain::DomainError::Logic(format!("Serialization failed: {e}")))?;
        self.set_bytes(key, bytes, ttl).await
    }
}

// Blanket implementation for all CacheGat implementers
impl<T: super::gat::CacheGat> CacheExtensions for T {}

// Supporting types for query operations

/// Criteria for session queries
///
/// Built through [`SessionQueryCriteria::builder`], which rejects inconsistent
/// combinations, so every value of this type is already valid.
///
/// # Examples
///
/// ```
/// use pjson_rs::domain::ports::SessionQueryCriteria;
///
/// let criteria = SessionQueryCriteria::builder()
///     .states(vec!["Active".to_owned()])
///     .exclude_expired(true)
///     .build()?;
/// assert!(criteria.exclude_expired());
///
/// assert!(SessionQueryCriteria::builder().states(vec![]).build().is_err());
/// # Ok::<(), pjson_rs::domain::DomainError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct SessionQueryCriteria {
    states: Option<Vec<String>>,
    created_after: Option<DateTime<Utc>>,
    created_before: Option<DateTime<Utc>>,
    client_info_pattern: Option<String>,
    has_active_streams: Option<bool>,
    min_stream_count: Option<usize>,
    max_stream_count: Option<usize>,
    exclude_expired: bool,
}

impl SessionQueryCriteria {
    /// Starts a builder with no filters set.
    #[must_use]
    pub fn builder() -> SessionQueryCriteriaBuilder {
        SessionQueryCriteriaBuilder::default()
    }

    /// Match sessions whose state is one of these values.
    #[must_use]
    pub fn states(&self) -> Option<&[String]> {
        self.states.as_deref()
    }

    /// Match sessions created at or after this timestamp.
    #[must_use]
    pub fn created_after(&self) -> Option<DateTime<Utc>> {
        self.created_after
    }

    /// Match sessions created at or before this timestamp.
    #[must_use]
    pub fn created_before(&self) -> Option<DateTime<Utc>> {
        self.created_before
    }

    /// Match sessions whose client info matches this pattern (substring or glob).
    #[must_use]
    pub fn client_info_pattern(&self) -> Option<&str> {
        self.client_info_pattern.as_deref()
    }

    /// When `Some`, match sessions that currently do (or do not) have active streams.
    #[must_use]
    pub fn has_active_streams(&self) -> Option<bool> {
        self.has_active_streams
    }

    /// Minimum number of streams the session must contain.
    #[must_use]
    pub fn min_stream_count(&self) -> Option<usize> {
        self.min_stream_count
    }

    /// Maximum number of streams the session may contain.
    #[must_use]
    pub fn max_stream_count(&self) -> Option<usize> {
        self.max_stream_count
    }

    /// When `true`, exclude sessions past their `expires_at` regardless of `states`.
    #[must_use]
    pub fn exclude_expired(&self) -> bool {
        self.exclude_expired
    }
}

/// Builder for [`SessionQueryCriteria`]; validation happens in [`Self::build`].
#[derive(Debug, Default)]
pub struct SessionQueryCriteriaBuilder {
    criteria: SessionQueryCriteria,
}

impl SessionQueryCriteriaBuilder {
    /// Sets the accepted session states.
    #[must_use]
    pub fn states(mut self, states: Vec<String>) -> Self {
        self.criteria.states = Some(states);
        self
    }

    /// Sets the lower bound on session creation time.
    #[must_use]
    pub fn created_after(mut self, at: DateTime<Utc>) -> Self {
        self.criteria.created_after = Some(at);
        self
    }

    /// Sets the upper bound on session creation time.
    #[must_use]
    pub fn created_before(mut self, at: DateTime<Utc>) -> Self {
        self.criteria.created_before = Some(at);
        self
    }

    /// Sets the client info pattern.
    #[must_use]
    pub fn client_info_pattern(mut self, pattern: String) -> Self {
        self.criteria.client_info_pattern = Some(pattern);
        self
    }

    /// Requires sessions to have (or not have) active streams.
    #[must_use]
    pub fn has_active_streams(mut self, active: bool) -> Self {
        self.criteria.has_active_streams = Some(active);
        self
    }

    /// Sets the minimum stream count.
    #[must_use]
    pub fn min_stream_count(mut self, count: usize) -> Self {
        self.criteria.min_stream_count = Some(count);
        self
    }

    /// Sets the maximum stream count.
    #[must_use]
    pub fn max_stream_count(mut self, count: usize) -> Self {
        self.criteria.max_stream_count = Some(count);
        self
    }

    /// Excludes sessions past their expiry.
    #[must_use]
    pub fn exclude_expired(mut self, exclude: bool) -> Self {
        self.criteria.exclude_expired = exclude;
        self
    }

    /// Validates the criteria and returns them.
    ///
    /// # Errors
    ///
    /// Returns `DomainError::InvalidInput` if:
    /// - `min_stream_count > max_stream_count`
    /// - `created_after > created_before`
    /// - `states` is empty (leave it unset for no filter)
    pub fn build(self) -> DomainResult<SessionQueryCriteria> {
        use crate::domain::DomainError;

        let c = &self.criteria;
        if let (Some(min), Some(max)) = (c.min_stream_count, c.max_stream_count)
            && min > max
        {
            return Err(DomainError::InvalidInput(format!(
                "min_stream_count ({min}) > max_stream_count ({max})"
            )));
        }

        if let (Some(after), Some(before)) = (c.created_after, c.created_before)
            && after > before
        {
            return Err(DomainError::InvalidInput(
                "created_after cannot be after created_before".to_string(),
            ));
        }

        if c.states.as_ref().is_some_and(Vec::is_empty) {
            return Err(DomainError::InvalidInput(
                "states filter cannot be empty (leave it unset for no filter)".to_string(),
            ));
        }

        Ok(self.criteria)
    }
}

/// Fields by which session query results can be sorted.
///
/// Every variant corresponds to an indexed field on `StreamSession`. Because
/// this is an exhaustive enum rather than a raw field name, an unrecognized
/// sort field is unrepresentable — callers can no longer construct a
/// `Pagination` whose `sort_by` needs runtime whitelist validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionSortField {
    /// Sort by session creation timestamp.
    CreatedAt,
    /// Sort by session last-update timestamp.
    UpdatedAt,
    /// Sort by number of streams attached to the session.
    StreamCount,
    /// Sort by total bytes streamed within the session.
    TotalBytes,
}

/// Fields by which frame query results can be sorted.
///
/// Mirrors [`SessionSortField`] but for the frame bounded context — grounded
/// in [`Frame::sequence`] and [`Frame::priority`]. Unlike `SessionSortField`,
/// this type has no `Serialize`/`Deserialize` derive yet, since no adapter
/// currently surfaces frame sorting to an external caller (e.g. over HTTP);
/// add one, matching `SessionSortField`'s `#[serde(rename_all = "snake_case")]`
/// convention, when such an adapter is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSortField {
    /// Sort by frame sequence number.
    Sequence,
    /// Sort by frame priority.
    Priority,
}

/// Pagination parameters, generic over the bounded context's sort-field vocabulary.
///
/// `S` fixes which fields `sort_by` may reference, so a `Pagination<S>` built for one
/// bounded context (e.g. sorted by [`SessionSortField`]) can never be passed to a port
/// expecting a different context's pagination (e.g. one expecting frame sorting) — the
/// guarantee is enforced at the port signature, not at construction; `S` itself carries
/// no bound, so `Pagination<S>` is constructible for any type. Use one of the
/// context-specific aliases rather than this type directly:
///
/// - [`SessionPagination`] — paginating session queries, sorted by [`SessionSortField`].
/// - [`FramePagination`] — paginating frame queries, sorted by [`FrameSortField`].
///
/// `offset` and `limit` are checked once by [`Self::new`]; `sort_by` and `sort_order`
/// carry no invariants and are set with [`Self::with_sort`].
///
/// # Examples
///
/// ```
/// use pjson_rs::domain::ports::{SessionPagination, SessionSortField, SortOrder};
///
/// let pagination = SessionPagination::new(0, 50)?
///     .with_sort(SessionSortField::CreatedAt, SortOrder::Descending);
/// assert_eq!(pagination.limit(), 50);
/// assert!(SessionPagination::new(0, 0).is_err());
/// # Ok::<(), pjson_rs::domain::DomainError>(())
/// ```
#[derive(Debug, Clone)]
pub struct Pagination<S> {
    offset: usize,
    limit: usize,
    sort_by: Option<S>,
    sort_order: SortOrder,
}

impl<S> Default for Pagination<S> {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 50,
            sort_by: None,
            sort_order: SortOrder::Ascending,
        }
    }
}

impl<S> Pagination<S> {
    /// Creates pagination with no sort, validated against the security limits.
    ///
    /// # Errors
    ///
    /// Returns `DomainError::InvalidInput` if:
    /// - `limit` is 0
    /// - `limit` exceeds `MAX_PAGINATION_LIMIT` (1000)
    /// - `offset` exceeds `MAX_PAGINATION_OFFSET` (1_000_000)
    pub fn new(offset: usize, limit: usize) -> DomainResult<Self> {
        use crate::domain::DomainError;
        use crate::domain::config::limits::{MAX_PAGINATION_LIMIT, MAX_PAGINATION_OFFSET};

        if limit == 0 {
            return Err(DomainError::InvalidInput(
                "pagination limit must be at least 1".to_string(),
            ));
        }

        if limit > MAX_PAGINATION_LIMIT {
            return Err(DomainError::InvalidInput(format!(
                "pagination limit {limit} exceeds maximum {MAX_PAGINATION_LIMIT}"
            )));
        }

        if offset > MAX_PAGINATION_OFFSET {
            return Err(DomainError::InvalidInput(format!(
                "pagination offset {offset} exceeds maximum {MAX_PAGINATION_OFFSET}"
            )));
        }

        Ok(Self {
            offset,
            limit,
            ..Self::default()
        })
    }

    /// Sets the sort field and direction.
    #[must_use]
    pub fn with_sort(mut self, sort_by: S, sort_order: SortOrder) -> Self {
        self.sort_by = Some(sort_by);
        self.sort_order = sort_order;
        self
    }

    /// Number of items to skip before returning results.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Maximum number of items to return.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Field to sort by, if any.
    #[must_use]
    pub fn sort_by(&self) -> Option<&S> {
        self.sort_by.as_ref()
    }

    /// Sort direction applied to `sort_by`.
    #[must_use]
    pub fn sort_order(&self) -> &SortOrder {
        &self.sort_order
    }
}

/// [`Pagination`] instantiated for session queries, sorted by [`SessionSortField`].
pub type SessionPagination = Pagination<SessionSortField>;

/// [`Pagination`] instantiated for frame queries, sorted by [`FrameSortField`].
pub type FramePagination = Pagination<FrameSortField>;

/// Direction in which results are ordered.
#[derive(Debug, Clone, PartialEq)]
pub enum SortOrder {
    /// Ascending order (smallest first).
    Ascending,
    /// Descending order (largest first).
    Descending,
}

/// Result of session query with metadata
#[derive(Debug, Clone)]
pub struct SessionQueryResult {
    /// Sessions returned in this page.
    pub sessions: Vec<StreamSession>,
    /// Total number of sessions matching the query, ignoring pagination.
    pub total_count: usize,
    /// Whether more sessions exist beyond this page.
    pub has_more: bool,
    /// Wall-clock duration of the query in milliseconds.
    pub query_duration_ms: u64,
    /// Indicates if MAX_SCAN_LIMIT was reached during query execution.
    ///
    /// When true, the query may have missed matching items because the scan
    /// limit was exceeded before all items could be evaluated. Results are
    /// still valid but potentially incomplete.
    pub scan_limit_reached: bool,
}

/// Snapshot of session health
#[derive(Debug, Clone)]
pub struct SessionHealthSnapshot {
    /// Identifier of the session this snapshot describes.
    pub session_id: SessionId,
    /// Aggregate health flag derived from rates and recent activity.
    pub is_healthy: bool,
    /// Number of streams currently in an active state.
    pub active_streams: usize,
    /// Total number of frames emitted by the session.
    pub total_frames: u64,
    /// Timestamp of the most recent activity within the session.
    pub last_activity: DateTime<Utc>,
    /// Fraction of recent operations that returned errors, in `[0.0, 1.0]`.
    pub error_rate: f64,
    /// Free-form named metrics captured at snapshot time.
    ///
    /// No key is contractually guaranteed across implementations, but
    /// [`GatInMemoryStreamRepository`](crate::infrastructure::adapters::GatInMemoryStreamRepository)
    /// currently populates:
    /// - `"active_streams"` — same value as [`Self::active_streams`], as `f64`.
    /// - `"total_bytes"` — `SessionStats::total_bytes`, as `f64`.
    /// - `"recent_avg_duration_ms"` — `SessionStats::recent_avg_duration_ms`
    ///   (the recency-weighted EMA, deliberately *not*
    ///   `SessionStats::average_stream_duration_ms`'s lifetime mean — see #458);
    ///   named to match the `SessionStats` field it mirrors so it reads
    ///   unambiguously next to `GET /pjs/sessions/{id}/stats`'s response,
    ///   which serializes `SessionStats` (and both duration fields) verbatim.
    pub metrics: HashMap<String, f64>,
}

/// Non-empty set of stream statuses for a [`StreamFilter`].
///
/// An empty list would be ambiguous (match nothing or everything), so it is
/// unrepresentable; leave the filter unset to match any status.
///
/// # Examples
///
/// ```
/// use pjson_rs::domain::ports::StreamStatus;
/// use pjson_rs::domain::ports::repositories::NonEmptyStatuses;
///
/// let statuses = NonEmptyStatuses::new(vec![StreamStatus::Active])?;
/// assert_eq!(statuses.as_slice(), [StreamStatus::Active]);
/// assert!(NonEmptyStatuses::new(vec![]).is_err());
/// # Ok::<(), pjson_rs::domain::DomainError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct NonEmptyStatuses(Vec<StreamStatus>);

impl NonEmptyStatuses {
    /// Wraps `statuses`.
    ///
    /// # Errors
    ///
    /// Returns `DomainError::InvalidInput` if `statuses` is empty.
    pub fn new(statuses: Vec<StreamStatus>) -> DomainResult<Self> {
        if statuses.is_empty() {
            return Err(crate::domain::DomainError::InvalidInput(
                "statuses filter cannot be empty (leave it unset for no filter)".to_string(),
            ));
        }
        Ok(Self(statuses))
    }

    /// The statuses, never empty.
    #[must_use]
    pub fn as_slice(&self) -> &[StreamStatus] {
        &self.0
    }
}

/// Whether a stream must have emitted frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramePresence {
    /// Match streams with at least one frame.
    WithFrames,
    /// Match streams with no frames.
    WithoutFrames,
}

/// Filter for stream queries
///
/// Built through [`StreamFilter::builder`]; the default value matches every stream.
///
/// # Examples
///
/// ```
/// use pjson_rs::domain::ports::StreamStatus;
/// use pjson_rs::domain::ports::repositories::{FramePresence, StreamFilter};
///
/// let filter = StreamFilter::builder()
///     .statuses(vec![StreamStatus::Active])
///     .has_frames(FramePresence::WithFrames)
///     .build()?;
/// assert_eq!(filter.has_frames(), Some(FramePresence::WithFrames));
/// # Ok::<(), pjson_rs::domain::DomainError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct StreamFilter {
    statuses: Option<NonEmptyStatuses>,
    created_after: Option<DateTime<Utc>>,
    has_frames: Option<FramePresence>,
}

impl StreamFilter {
    /// Starts a builder with no filters set.
    #[must_use]
    pub fn builder() -> StreamFilterBuilder {
        StreamFilterBuilder::default()
    }

    /// Accepted stream statuses, if filtered.
    #[must_use]
    pub fn statuses(&self) -> Option<&NonEmptyStatuses> {
        self.statuses.as_ref()
    }

    /// Lower bound on stream creation time, if filtered.
    #[must_use]
    pub fn created_after(&self) -> Option<DateTime<Utc>> {
        self.created_after
    }

    /// Required frame presence, if filtered.
    #[must_use]
    pub fn has_frames(&self) -> Option<FramePresence> {
        self.has_frames
    }
}

/// Builder for [`StreamFilter`]; validation happens in [`Self::build`].
#[derive(Debug, Default)]
pub struct StreamFilterBuilder {
    statuses: Option<Vec<StreamStatus>>,
    created_after: Option<DateTime<Utc>>,
    has_frames: Option<FramePresence>,
}

impl StreamFilterBuilder {
    /// Sets the accepted stream statuses.
    #[must_use]
    pub fn statuses(mut self, statuses: Vec<StreamStatus>) -> Self {
        self.statuses = Some(statuses);
        self
    }

    /// Sets the lower bound on stream creation time.
    #[must_use]
    pub fn created_after(mut self, at: DateTime<Utc>) -> Self {
        self.created_after = Some(at);
        self
    }

    /// Requires streams to have (or not have) frames.
    #[must_use]
    pub fn has_frames(mut self, presence: FramePresence) -> Self {
        self.has_frames = Some(presence);
        self
    }

    /// Validates the filter and returns it.
    ///
    /// # Errors
    ///
    /// Returns `DomainError::InvalidInput` if `statuses` was set to an empty list.
    pub fn build(self) -> DomainResult<StreamFilter> {
        Ok(StreamFilter {
            statuses: self.statuses.map(NonEmptyStatuses::new).transpose()?,
            created_after: self.created_after,
            has_frames: self.has_frames,
        })
    }
}

/// Stream status enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum StreamStatus {
    /// Stream has been created but not yet started.
    Created,
    /// Stream is actively emitting frames.
    Active,
    /// Stream has been paused; can resume.
    Paused,
    /// Stream finished successfully.
    Completed,
    /// Stream terminated with an error.
    Failed,
    /// Stream was cancelled by request before completion.
    Cancelled,
}

/// Stream metadata for indexing
#[derive(Debug, Clone, Default)]
pub struct StreamMetadata {
    /// Free-form name/value tags attached to the stream.
    pub tags: HashMap<String, String>,
    /// MIME type of the payload, when known.
    pub content_type: Option<String>,
    /// Estimated total payload size in bytes, when known.
    pub estimated_size: Option<u64>,
    /// Hints for likely priorities of frames within the stream.
    pub priority_hints: Vec<Priority>,
}

/// Stream statistics
#[derive(Debug, Clone)]
pub struct StreamStatistics {
    /// Total number of frames emitted by the stream.
    pub total_frames: u64,
    /// Total number of payload bytes emitted by the stream.
    pub total_bytes: u64,
    /// Distribution of frames across priority buckets.
    pub priority_distribution: PriorityDistribution,
    /// Average payload size per frame, in bytes.
    pub avg_frame_size: f64,
    /// Timestamp when the stream was created.
    pub creation_time: DateTime<Utc>,
    /// Timestamp when the stream completed, if it has.
    pub completion_time: Option<DateTime<Utc>>,
    /// Total processing duration from start to completion, if completed.
    pub processing_duration: Option<std::time::Duration>,
}

/// Result of frame queries
#[derive(Debug, Clone)]
pub struct FrameQueryResult {
    /// Frames returned in this page.
    pub frames: Vec<Frame>,
    /// Total number of frames matching the query, ignoring pagination.
    pub total_count: usize,
    /// Whether more frames exist beyond this page.
    pub has_more: bool,
    /// Highest priority observed across the returned frames, if any.
    pub highest_priority: Option<Priority>,
    /// Lowest priority observed across the returned frames, if any.
    pub lowest_priority: Option<Priority>,
}

// Use the canonical PriorityDistribution from events
pub use crate::domain::events::PriorityDistribution;

/// Cache performance statistics
#[derive(Debug, Clone)]
pub struct CacheStatistics {
    /// Fraction of lookups served from cache, in `[0.0, 1.0]`.
    pub hit_rate: f64,
    /// Fraction of lookups that missed the cache, in `[0.0, 1.0]`.
    pub miss_rate: f64,
    /// Total number of keys currently held in the cache.
    pub total_keys: u64,
    /// Approximate memory occupied by the cache, in bytes.
    pub memory_usage_bytes: u64,
    /// Number of entries evicted since the cache was created.
    pub eviction_count: u64,
}

impl Default for CacheStatistics {
    fn default() -> Self {
        Self {
            hit_rate: 0.0,
            miss_rate: 0.0,
            total_keys: 0,
            memory_usage_bytes: 0,
            eviction_count: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::gat::CacheGat;
    use super::*;
    use crate::domain::DomainError;
    use std::future::Future;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    // ========================================================================
    // MockCacheGat - GAT-based mock for testing CacheExtensions
    // ========================================================================

    struct MockCacheGat {
        store: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        fail_on_get: bool,
        fail_on_set: bool,
    }

    impl MockCacheGat {
        fn new() -> Self {
            Self {
                store: Arc::new(Mutex::new(HashMap::new())),
                fail_on_get: false,
                fail_on_set: false,
            }
        }

        fn with_get_failure() -> Self {
            Self {
                store: Arc::new(Mutex::new(HashMap::new())),
                fail_on_get: true,
                fail_on_set: false,
            }
        }

        fn with_set_failure() -> Self {
            Self {
                store: Arc::new(Mutex::new(HashMap::new())),
                fail_on_get: false,
                fail_on_set: true,
            }
        }
    }

    impl super::super::gat::CacheGat for MockCacheGat {
        type GetBytesFuture<'a>
            = impl Future<Output = DomainResult<Option<Vec<u8>>>> + Send + 'a
        where
            Self: 'a;

        type SetBytesFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type RemoveFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type ClearPrefixFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type GetStatsFuture<'a>
            = impl Future<Output = DomainResult<CacheStatistics>> + Send + 'a
        where
            Self: 'a;

        fn get_bytes<'a>(&'a self, key: &'a str) -> Self::GetBytesFuture<'a> {
            async move {
                if self.fail_on_get {
                    return Err(DomainError::Logic("Mock get failure".to_string()));
                }
                let store = self.store.lock().await;
                Ok(store.get(key).cloned())
            }
        }

        fn set_bytes<'a>(
            &'a self,
            key: &'a str,
            value: Vec<u8>,
            _ttl: Option<Duration>,
        ) -> Self::SetBytesFuture<'a> {
            async move {
                if self.fail_on_set {
                    return Err(DomainError::Logic("Mock set failure".to_string()));
                }
                let mut store = self.store.lock().await;
                store.insert(key.to_string(), value);
                Ok(())
            }
        }

        fn remove<'a>(&'a self, key: &'a str) -> Self::RemoveFuture<'a> {
            async move {
                let mut store = self.store.lock().await;
                store.remove(key);
                Ok(())
            }
        }

        fn clear_prefix<'a>(&'a self, prefix: &'a str) -> Self::ClearPrefixFuture<'a> {
            async move {
                let mut store = self.store.lock().await;
                store.retain(|k, _| !k.starts_with(prefix));
                Ok(())
            }
        }

        fn get_stats(&self) -> Self::GetStatsFuture<'_> {
            async move {
                let store = self.store.lock().await;
                Ok(CacheStatistics {
                    hit_rate: 0.0,
                    miss_rate: 0.0,
                    total_keys: store.len() as u64,
                    memory_usage_bytes: 0,
                    eviction_count: 0,
                })
            }
        }
    }

    // Tests for Pagination
    #[test]
    fn test_pagination_default() {
        let pagination = SessionPagination::default();
        assert_eq!(pagination.offset, 0);
        assert_eq!(pagination.limit, 50);
        assert!(pagination.sort_by.is_none());
        assert_eq!(pagination.sort_order, SortOrder::Ascending);
    }

    #[test]
    fn test_pagination_custom() {
        let pagination = SessionPagination {
            offset: 10,
            limit: 100,
            sort_by: Some(SessionSortField::CreatedAt),
            sort_order: SortOrder::Descending,
        };
        assert_eq!(pagination.offset, 10);
        assert_eq!(pagination.limit, 100);
        assert_eq!(pagination.sort_by, Some(SessionSortField::CreatedAt));
        assert_eq!(pagination.sort_order, SortOrder::Descending);
    }

    #[test]
    fn test_pagination_debug() {
        let pagination = SessionPagination::default();
        let debug = format!("{:?}", pagination);
        assert!(debug.contains("Pagination"));
        assert!(debug.contains("offset: 0"));
        assert!(debug.contains("limit: 50"));
    }

    #[test]
    fn test_pagination_clone() {
        let pagination = SessionPagination {
            offset: 5,
            limit: 25,
            sort_by: Some(SessionSortField::UpdatedAt),
            sort_order: SortOrder::Ascending,
        };
        let cloned = pagination.clone();
        assert_eq!(cloned.offset, pagination.offset);
        assert_eq!(cloned.limit, pagination.limit);
        assert_eq!(cloned.sort_by, pagination.sort_by);
        assert_eq!(cloned.sort_order, pagination.sort_order);
    }

    // Tests for Pagination construction
    #[test]
    fn test_pagination_new_zero_limit_rejected() {
        let err = SessionPagination::new(0, 0).unwrap_err();
        assert!(matches!(err, DomainError::InvalidInput(_)));
        assert!(format!("{}", err).contains("at least 1"));
    }

    #[test]
    fn test_pagination_new_excessive_limit_rejected() {
        let err = SessionPagination::new(0, 10_000).unwrap_err();
        assert!(format!("{}", err).contains("exceeds maximum"));
    }

    #[test]
    fn test_pagination_new_excessive_offset_rejected() {
        let err = SessionPagination::new(2_000_000, 10).unwrap_err();
        assert!(format!("{}", err).contains("offset"));
    }

    #[test]
    fn test_pagination_new_extreme_offset_rejected() {
        let err = SessionPagination::new(usize::MAX, 1).unwrap_err();
        assert!(format!("{}", err).contains("exceeds maximum"));
    }

    #[test]
    fn test_pagination_with_sort_accepts_every_sort_field() {
        for field in [
            SessionSortField::CreatedAt,
            SessionSortField::UpdatedAt,
            SessionSortField::StreamCount,
            SessionSortField::TotalBytes,
        ] {
            let pagination = SessionPagination::new(0, 10)
                .unwrap()
                .with_sort(field, SortOrder::Ascending);
            assert_eq!(pagination.sort_by(), Some(&field));
        }
    }

    /// #494: `SessionSortField` must round-trip through serde using its exact
    /// `snake_case` spellings, matching what `parse_sort_field` in the HTTP
    /// adapter deserializes from a raw query-string value.
    #[test]
    fn test_session_sort_field_serde_round_trip() {
        for (field, expected) in [
            (SessionSortField::CreatedAt, "created_at"),
            (SessionSortField::UpdatedAt, "updated_at"),
            (SessionSortField::StreamCount, "stream_count"),
            (SessionSortField::TotalBytes, "total_bytes"),
        ] {
            let json = serde_json::to_value(field).unwrap();
            assert_eq!(json, serde_json::Value::String(expected.to_string()));

            let round_tripped: SessionSortField = serde_json::from_value(json).unwrap();
            assert_eq!(round_tripped, field);
        }
    }

    /// #494/#492: an unrecognized spelling must fail deserialization — this is the
    /// domain-level guarantee `parse_sort_field` relies on to reject bad `sort_by`
    /// query values with `400` instead of silently accepting them.
    #[test]
    fn test_session_sort_field_rejects_unknown_value() {
        let result: Result<SessionSortField, _> =
            serde_json::from_value(serde_json::Value::String("bogus".to_string()));
        assert!(result.is_err());

        // Missing-underscore spelling: never matched the old hand-rolled `match` either
        // — it fell through to `_ => None` and was silently ignored, never accepted.
        let result: Result<SessionSortField, _> =
            serde_json::from_value(serde_json::Value::String("createdat".to_string()));
        assert!(result.is_err());
    }

    /// #491 decoupling regression: `FramePagination` must be constructible
    /// using only `FrameSortField` — this only compiles if the frame
    /// bounded context no longer depends on `SessionSortField`.
    #[test]
    fn test_frame_pagination_decoupled_from_session_sort_field() {
        let pagination = FramePagination::new(0, 10)
            .unwrap()
            .with_sort(FrameSortField::Sequence, SortOrder::Ascending);
        assert_eq!(pagination.sort_by(), Some(&FrameSortField::Sequence));
    }

    // Tests for SortOrder
    #[test]
    fn test_sort_order_equality() {
        assert_eq!(SortOrder::Ascending, SortOrder::Ascending);
        assert_eq!(SortOrder::Descending, SortOrder::Descending);
        assert_ne!(SortOrder::Ascending, SortOrder::Descending);
    }

    #[test]
    fn test_sort_order_debug() {
        assert_eq!(format!("{:?}", SortOrder::Ascending), "Ascending");
        assert_eq!(format!("{:?}", SortOrder::Descending), "Descending");
    }

    #[test]
    fn test_sort_order_clone() {
        let order = SortOrder::Ascending;
        let cloned = order.clone();
        assert_eq!(order, cloned);
    }

    // Tests for SessionQueryCriteria
    #[test]
    fn test_session_query_criteria_default() {
        let criteria = SessionQueryCriteria::default();
        assert!(criteria.states.is_none());
        assert!(criteria.created_after.is_none());
        assert!(criteria.created_before.is_none());
        assert!(criteria.client_info_pattern.is_none());
        assert!(criteria.has_active_streams.is_none());
        assert!(criteria.min_stream_count.is_none());
        assert!(criteria.max_stream_count.is_none());
    }

    #[test]
    fn test_session_query_criteria_with_states() {
        let criteria = SessionQueryCriteria {
            states: Some(vec!["active".to_string(), "pending".to_string()]),
            ..Default::default()
        };
        assert!(criteria.states.is_some());
        let states = criteria.states.unwrap();
        assert_eq!(states.len(), 2);
        assert!(states.iter().any(|s| s == "active"));
    }

    #[test]
    fn test_session_query_criteria_with_time_range() {
        let now = Utc::now();
        let criteria = SessionQueryCriteria {
            created_after: Some(now - chrono::Duration::hours(1)),
            created_before: Some(now),
            ..Default::default()
        };
        assert!(criteria.created_after.is_some());
        assert!(criteria.created_before.is_some());
    }

    #[test]
    fn test_session_query_criteria_with_stream_counts() {
        let criteria = SessionQueryCriteria {
            min_stream_count: Some(1),
            max_stream_count: Some(10),
            has_active_streams: Some(true),
            ..Default::default()
        };
        assert_eq!(criteria.min_stream_count, Some(1));
        assert_eq!(criteria.max_stream_count, Some(10));
        assert_eq!(criteria.has_active_streams, Some(true));
    }

    #[test]
    fn test_session_query_criteria_debug() {
        let criteria = SessionQueryCriteria::default();
        let debug = format!("{:?}", criteria);
        assert!(debug.contains("SessionQueryCriteria"));
    }

    #[test]
    fn test_session_query_criteria_clone() {
        let criteria = SessionQueryCriteria {
            states: Some(vec!["active".to_string()]),
            client_info_pattern: Some("test*".to_string()),
            ..Default::default()
        };
        let cloned = criteria.clone();
        assert_eq!(cloned.states, criteria.states);
        assert_eq!(cloned.client_info_pattern, criteria.client_info_pattern);
    }

    // Tests for SessionQueryCriteria validation
    #[test]
    fn test_criteria_build_min_greater_than_max_rejected() {
        let err = SessionQueryCriteria::builder()
            .min_stream_count(10)
            .max_stream_count(5)
            .build()
            .unwrap_err();
        assert!(matches!(err, DomainError::InvalidInput(_)));
        assert!(format!("{}", err).contains("min_stream_count"));
    }

    #[test]
    fn test_criteria_build_after_greater_than_before_rejected() {
        let now = Utc::now();
        let err = SessionQueryCriteria::builder()
            .created_after(now)
            .created_before(now - chrono::Duration::hours(1))
            .build()
            .unwrap_err();
        assert!(format!("{}", err).contains("created_after"));
    }

    #[test]
    fn test_criteria_build_empty_states_rejected() {
        let err = SessionQueryCriteria::builder()
            .states(vec![])
            .build()
            .unwrap_err();
        assert!(format!("{}", err).contains("empty"));
    }

    #[test]
    fn test_criteria_build_valid_criteria_accepted() {
        let now = Utc::now();
        let criteria = SessionQueryCriteria::builder()
            .states(vec!["active".to_string()])
            .created_after(now - chrono::Duration::hours(1))
            .created_before(now)
            .min_stream_count(1)
            .max_stream_count(10)
            .build()
            .unwrap();
        assert_eq!(criteria.states(), Some(["active".to_string()].as_slice()));
        assert_eq!(criteria.min_stream_count(), Some(1));
    }

    // Tests for StreamFilter
    #[test]
    fn test_stream_filter_default() {
        let filter = StreamFilter::default();
        assert!(filter.statuses().is_none());
        assert!(filter.created_after().is_none());
        assert!(filter.has_frames().is_none());
    }

    #[test]
    fn test_stream_filter_build_empty_statuses_rejected() {
        let err = StreamFilter::builder()
            .statuses(vec![])
            .build()
            .unwrap_err();
        assert!(matches!(err, DomainError::InvalidInput(_)));
        assert!(format!("{}", err).contains("empty"));
    }

    #[test]
    fn test_stream_filter_build_keeps_statuses_in_order() {
        let filter = StreamFilter::builder()
            .statuses(vec![StreamStatus::Active, StreamStatus::Paused])
            .build()
            .unwrap();
        assert_eq!(
            filter.statuses().unwrap().as_slice(),
            [StreamStatus::Active, StreamStatus::Paused]
        );
    }

    #[test]
    fn test_stream_filter_clone() {
        let filter = StreamFilter::builder()
            .has_frames(FramePresence::WithFrames)
            .created_after(Utc::now())
            .build()
            .unwrap();
        let cloned = filter.clone();
        assert_eq!(cloned.has_frames(), filter.has_frames());
        assert_eq!(cloned.created_after(), filter.created_after());
    }

    // Tests for StreamStatus
    #[test]
    fn test_stream_status_all_variants() {
        let statuses = [
            StreamStatus::Created,
            StreamStatus::Active,
            StreamStatus::Paused,
            StreamStatus::Completed,
            StreamStatus::Failed,
            StreamStatus::Cancelled,
        ];
        assert_eq!(statuses.len(), 6);
    }

    #[test]
    fn test_stream_status_equality() {
        assert_eq!(StreamStatus::Created, StreamStatus::Created);
        assert_eq!(StreamStatus::Active, StreamStatus::Active);
        assert_ne!(StreamStatus::Created, StreamStatus::Active);
        assert_ne!(StreamStatus::Completed, StreamStatus::Failed);
    }

    #[test]
    fn test_stream_status_debug() {
        assert_eq!(format!("{:?}", StreamStatus::Created), "Created");
        assert_eq!(format!("{:?}", StreamStatus::Active), "Active");
        assert_eq!(format!("{:?}", StreamStatus::Paused), "Paused");
        assert_eq!(format!("{:?}", StreamStatus::Completed), "Completed");
        assert_eq!(format!("{:?}", StreamStatus::Failed), "Failed");
        assert_eq!(format!("{:?}", StreamStatus::Cancelled), "Cancelled");
    }

    #[test]
    fn test_stream_status_clone() {
        let status = StreamStatus::Active;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    // Tests for StreamMetadata
    #[test]
    fn test_stream_metadata_default() {
        let metadata = StreamMetadata::default();
        assert!(metadata.tags.is_empty());
        assert!(metadata.content_type.is_none());
        assert!(metadata.estimated_size.is_none());
        assert!(metadata.priority_hints.is_empty());
    }

    #[test]
    fn test_stream_metadata_with_tags() {
        let mut tags = HashMap::new();
        tags.insert("env".to_string(), "production".to_string());
        tags.insert("version".to_string(), "1.0".to_string());

        let metadata = StreamMetadata {
            tags,
            content_type: Some("application/json".to_string()),
            estimated_size: Some(1024),
            priority_hints: vec![Priority::HIGH, Priority::MEDIUM],
        };

        assert_eq!(metadata.tags.len(), 2);
        assert_eq!(metadata.tags.get("env"), Some(&"production".to_string()));
        assert_eq!(metadata.content_type, Some("application/json".to_string()));
        assert_eq!(metadata.estimated_size, Some(1024));
        assert_eq!(metadata.priority_hints.len(), 2);
    }

    #[test]
    fn test_stream_metadata_clone() {
        let mut tags = HashMap::new();
        tags.insert("key".to_string(), "value".to_string());
        let metadata = StreamMetadata {
            tags,
            content_type: Some("text/plain".to_string()),
            estimated_size: Some(512),
            priority_hints: vec![Priority::LOW],
        };
        let cloned = metadata.clone();
        assert_eq!(cloned.tags, metadata.tags);
        assert_eq!(cloned.content_type, metadata.content_type);
        assert_eq!(cloned.estimated_size, metadata.estimated_size);
        assert_eq!(cloned.priority_hints, metadata.priority_hints);
    }

    // Tests for CacheStatistics
    #[test]
    fn test_cache_statistics_default() {
        let stats = CacheStatistics::default();
        assert_eq!(stats.hit_rate, 0.0);
        assert_eq!(stats.miss_rate, 0.0);
        assert_eq!(stats.total_keys, 0);
        assert_eq!(stats.memory_usage_bytes, 0);
        assert_eq!(stats.eviction_count, 0);
    }

    #[test]
    fn test_cache_statistics_custom() {
        let stats = CacheStatistics {
            hit_rate: 0.85,
            miss_rate: 0.15,
            total_keys: 1000,
            memory_usage_bytes: 1024 * 1024,
            eviction_count: 50,
        };
        assert_eq!(stats.hit_rate, 0.85);
        assert_eq!(stats.miss_rate, 0.15);
        assert_eq!(stats.total_keys, 1000);
        assert_eq!(stats.memory_usage_bytes, 1024 * 1024);
        assert_eq!(stats.eviction_count, 50);
    }

    #[test]
    fn test_cache_statistics_debug() {
        let stats = CacheStatistics::default();
        let debug = format!("{:?}", stats);
        assert!(debug.contains("CacheStatistics"));
        assert!(debug.contains("hit_rate"));
        assert!(debug.contains("miss_rate"));
    }

    #[test]
    fn test_cache_statistics_clone() {
        let stats = CacheStatistics {
            hit_rate: 0.75,
            miss_rate: 0.25,
            total_keys: 500,
            memory_usage_bytes: 2048,
            eviction_count: 10,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.hit_rate, stats.hit_rate);
        assert_eq!(cloned.miss_rate, stats.miss_rate);
        assert_eq!(cloned.total_keys, stats.total_keys);
        assert_eq!(cloned.memory_usage_bytes, stats.memory_usage_bytes);
        assert_eq!(cloned.eviction_count, stats.eviction_count);
    }

    // Tests for SessionQueryResult
    #[test]
    fn test_session_query_result() {
        let result = SessionQueryResult {
            sessions: vec![],
            total_count: 100,
            has_more: true,
            query_duration_ms: 50,
            scan_limit_reached: false,
        };
        assert!(result.sessions.is_empty());
        assert_eq!(result.total_count, 100);
        assert!(result.has_more);
        assert_eq!(result.query_duration_ms, 50);
        assert!(!result.scan_limit_reached);
    }

    #[test]
    fn test_session_query_result_debug() {
        let result = SessionQueryResult {
            sessions: vec![],
            total_count: 0,
            has_more: false,
            query_duration_ms: 0,
            scan_limit_reached: false,
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("SessionQueryResult"));
    }

    #[test]
    fn test_session_query_result_clone() {
        let result = SessionQueryResult {
            sessions: vec![],
            total_count: 25,
            has_more: false,
            query_duration_ms: 10,
            scan_limit_reached: true,
        };
        let cloned = result.clone();
        assert_eq!(cloned.total_count, result.total_count);
        assert_eq!(cloned.has_more, result.has_more);
        assert_eq!(cloned.query_duration_ms, result.query_duration_ms);
        assert_eq!(cloned.scan_limit_reached, result.scan_limit_reached);
    }

    // Tests for SessionHealthSnapshot
    #[test]
    fn test_session_health_snapshot() {
        let mut metrics = HashMap::new();
        metrics.insert("cpu_usage".to_string(), 0.5);
        metrics.insert("memory_usage".to_string(), 0.75);

        let session_id = SessionId::new();
        let snapshot = SessionHealthSnapshot {
            session_id,
            is_healthy: true,
            active_streams: 5,
            total_frames: 1000,
            last_activity: Utc::now(),
            error_rate: 0.01,
            metrics,
        };

        assert_eq!(snapshot.session_id, session_id);
        assert!(snapshot.is_healthy);
        assert_eq!(snapshot.active_streams, 5);
        assert_eq!(snapshot.total_frames, 1000);
        assert_eq!(snapshot.error_rate, 0.01);
        assert_eq!(snapshot.metrics.len(), 2);
    }

    #[test]
    fn test_session_health_snapshot_unhealthy() {
        let snapshot = SessionHealthSnapshot {
            session_id: SessionId::new(),
            is_healthy: false,
            active_streams: 0,
            total_frames: 0,
            last_activity: Utc::now(),
            error_rate: 0.5,
            metrics: HashMap::new(),
        };
        assert!(!snapshot.is_healthy);
        assert_eq!(snapshot.error_rate, 0.5);
    }

    #[test]
    fn test_session_health_snapshot_clone() {
        let mut metrics = HashMap::new();
        metrics.insert("test".to_string(), 1.0);
        let snapshot = SessionHealthSnapshot {
            session_id: SessionId::new(),
            is_healthy: true,
            active_streams: 3,
            total_frames: 500,
            last_activity: Utc::now(),
            error_rate: 0.0,
            metrics,
        };
        let cloned = snapshot.clone();
        assert_eq!(cloned.session_id, snapshot.session_id);
        assert_eq!(cloned.is_healthy, snapshot.is_healthy);
        assert_eq!(cloned.active_streams, snapshot.active_streams);
    }

    // Tests for FrameQueryResult
    #[test]
    fn test_frame_query_result() {
        let result = FrameQueryResult {
            frames: vec![],
            total_count: 50,
            has_more: true,
            highest_priority: Some(Priority::CRITICAL),
            lowest_priority: Some(Priority::BACKGROUND),
        };
        assert!(result.frames.is_empty());
        assert_eq!(result.total_count, 50);
        assert!(result.has_more);
        assert_eq!(result.highest_priority, Some(Priority::CRITICAL));
        assert_eq!(result.lowest_priority, Some(Priority::BACKGROUND));
    }

    #[test]
    fn test_frame_query_result_no_priority() {
        let result = FrameQueryResult {
            frames: vec![],
            total_count: 0,
            has_more: false,
            highest_priority: None,
            lowest_priority: None,
        };
        assert!(result.highest_priority.is_none());
        assert!(result.lowest_priority.is_none());
    }

    #[test]
    fn test_frame_query_result_clone() {
        let result = FrameQueryResult {
            frames: vec![],
            total_count: 10,
            has_more: false,
            highest_priority: Some(Priority::HIGH),
            lowest_priority: Some(Priority::LOW),
        };
        let cloned = result.clone();
        assert_eq!(cloned.total_count, result.total_count);
        assert_eq!(cloned.has_more, result.has_more);
        assert_eq!(cloned.highest_priority, result.highest_priority);
        assert_eq!(cloned.lowest_priority, result.lowest_priority);
    }

    // Tests for StreamStatistics
    #[test]
    fn test_stream_statistics() {
        let stats = StreamStatistics {
            total_frames: 100,
            total_bytes: 1024 * 1024,
            priority_distribution: PriorityDistribution::default(),
            avg_frame_size: 10240.0,
            creation_time: Utc::now(),
            completion_time: Some(Utc::now()),
            processing_duration: Some(Duration::from_secs(60)),
        };
        assert_eq!(stats.total_frames, 100);
        assert_eq!(stats.total_bytes, 1024 * 1024);
        assert_eq!(stats.avg_frame_size, 10240.0);
        assert!(stats.completion_time.is_some());
        assert!(stats.processing_duration.is_some());
    }

    #[test]
    fn test_stream_statistics_in_progress() {
        let stats = StreamStatistics {
            total_frames: 50,
            total_bytes: 512 * 1024,
            priority_distribution: PriorityDistribution::default(),
            avg_frame_size: 10485.76,
            creation_time: Utc::now(),
            completion_time: None,
            processing_duration: None,
        };
        assert!(stats.completion_time.is_none());
        assert!(stats.processing_duration.is_none());
    }

    #[test]
    fn test_stream_statistics_clone() {
        let stats = StreamStatistics {
            total_frames: 25,
            total_bytes: 256 * 1024,
            priority_distribution: PriorityDistribution::default(),
            avg_frame_size: 10485.76,
            creation_time: Utc::now(),
            completion_time: None,
            processing_duration: None,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.total_frames, stats.total_frames);
        assert_eq!(cloned.total_bytes, stats.total_bytes);
        assert_eq!(cloned.avg_frame_size, stats.avg_frame_size);
    }

    // ========================================================================
    // Tests for CacheExtensions trait with MockCacheGat
    // ========================================================================

    #[tokio::test]
    async fn test_cache_extensions_set_and_get_typed() {
        let cache = MockCacheGat::new();

        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        cache.set_typed("key1", &data, None).await.unwrap();
        let retrieved: Option<TestData> = cache.get_typed("key1").await.unwrap();

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), data);
    }

    #[tokio::test]
    async fn test_cache_extensions_get_typed_not_found() {
        let cache = MockCacheGat::new();
        let result: Option<String> = cache.get_typed::<String>("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_extensions_set_typed_with_ttl() {
        let cache = MockCacheGat::new();
        let ttl = Some(Duration::from_secs(60));
        cache.set_typed("key", &"value", ttl).await.unwrap();
        let result: Option<String> = cache.get_typed("key").await.unwrap();
        assert_eq!(result, Some("value".to_string()));
    }

    #[tokio::test]
    async fn test_cache_extensions_get_typed_invalid_json() {
        let cache = MockCacheGat::new();
        cache
            .set_bytes("key", b"invalid json{{{".to_vec(), None)
            .await
            .unwrap();
        let result: DomainResult<Option<HashMap<String, String>>> = cache.get_typed("key").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = format!("{:?}", err);
        assert!(err_msg.contains("Deserialization failed"));
    }

    #[tokio::test]
    async fn test_cache_extensions_get_bytes_failure() {
        let cache = MockCacheGat::with_get_failure();
        let result: DomainResult<Option<String>> = cache.get_typed("key").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cache_extensions_set_bytes_failure() {
        let cache = MockCacheGat::with_set_failure();
        let result = cache.set_typed("key", &"value", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cache_extensions_complex_type() {
        let cache = MockCacheGat::new();

        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct ComplexData {
            items: Vec<String>,
            metadata: HashMap<String, i64>,
        }

        let mut metadata = HashMap::new();
        metadata.insert("count".to_string(), 100);
        metadata.insert("size".to_string(), 1024);

        let data = ComplexData {
            items: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            metadata,
        };

        cache.set_typed("complex", &data, None).await.unwrap();
        let retrieved: Option<ComplexData> = cache.get_typed("complex").await.unwrap();

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.items.len(), 3);
        assert_eq!(retrieved.metadata.get("count"), Some(&100));
    }

    // ========================================================================
    // Tests for MockCacheGat operations
    // ========================================================================

    #[tokio::test]
    async fn test_mock_cache_gat_remove() {
        let cache = MockCacheGat::new();
        cache
            .set_bytes("key", b"value".to_vec(), None)
            .await
            .unwrap();
        assert!(cache.get_bytes("key").await.unwrap().is_some());
        cache.remove("key").await.unwrap();
        assert!(cache.get_bytes("key").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_cache_gat_clear_prefix() {
        let cache = MockCacheGat::new();
        cache
            .set_bytes("prefix:key1", b"value1".to_vec(), None)
            .await
            .unwrap();
        cache
            .set_bytes("prefix:key2", b"value2".to_vec(), None)
            .await
            .unwrap();
        cache
            .set_bytes("other:key3", b"value3".to_vec(), None)
            .await
            .unwrap();

        cache.clear_prefix("prefix:").await.unwrap();

        assert!(cache.get_bytes("prefix:key1").await.unwrap().is_none());
        assert!(cache.get_bytes("prefix:key2").await.unwrap().is_none());
        assert!(cache.get_bytes("other:key3").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_mock_cache_gat_get_stats() {
        let cache = MockCacheGat::new();
        cache
            .set_bytes("key1", b"value1".to_vec(), None)
            .await
            .unwrap();
        cache
            .set_bytes("key2", b"value2".to_vec(), None)
            .await
            .unwrap();

        let stats = cache.get_stats().await.unwrap();
        assert_eq!(stats.total_keys, 2);
    }
}
