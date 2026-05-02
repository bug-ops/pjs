//! Queries - Read operations that don't change system state

use crate::application::dto::{PriorityDto, SessionIdDto, StreamIdDto};
use crate::domain::{
    aggregates::{
        StreamSession,
        stream_session::{SessionHealth, SessionStats},
    },
    entities::{Frame, Stream},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Get session information by ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSessionQuery {
    /// Identifier of the session to retrieve.
    pub session_id: SessionIdDto,
}

/// Get all active sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetActiveSessionsQuery {
    /// Maximum number of sessions to return.
    pub limit: Option<usize>,
    /// Number of sessions to skip before returning results.
    pub offset: Option<usize>,
}

/// Get stream information by ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStreamQuery {
    /// Identifier of the parent session.
    pub session_id: SessionIdDto,
    /// Identifier of the stream to retrieve.
    pub stream_id: StreamIdDto,
}

/// Get all streams for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStreamsForSessionQuery {
    /// Identifier of the parent session.
    pub session_id: SessionIdDto,
    /// When `true`, includes streams that are no longer active.
    pub include_inactive: bool,
}

/// Get session health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSessionHealthQuery {
    /// Identifier of the session whose health is being queried.
    pub session_id: SessionIdDto,
}

/// Get frames for a stream with filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStreamFramesQuery {
    /// Identifier of the parent session.
    pub session_id: SessionIdDto,
    /// Identifier of the stream whose frames are being queried.
    pub stream_id: StreamIdDto,
    /// Return only frames whose sequence number is greater than this value.
    pub since_sequence: Option<u64>,
    /// Return only frames whose priority satisfies this filter.
    pub priority_filter: Option<PriorityDto>,
    /// Maximum number of frames to return.
    pub limit: Option<usize>,
}

/// Get session statistics and metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSessionStatsQuery {
    /// Identifier of the session whose statistics are being queried.
    pub session_id: SessionIdDto,
}

/// Get system-wide statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSystemStatsQuery {
    /// When `true`, includes historical (closed) sessions in aggregates.
    pub include_historical: bool,
}

/// Search sessions by criteria
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSessionsQuery {
    /// Filters that returned sessions must satisfy.
    pub filters: SessionFilters,
    /// Field to order results by.
    pub sort_by: Option<SessionSortField>,
    /// Direction of the sort applied to `sort_by`.
    pub sort_order: Option<SortOrder>,
    /// Maximum number of sessions to return.
    pub limit: Option<usize>,
    /// Number of sessions to skip before returning results.
    pub offset: Option<usize>,
}

/// Session filtering criteria
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionFilters {
    /// Match sessions whose state equals this value.
    pub state: Option<String>,
    /// Match sessions created at or after this timestamp.
    pub created_after: Option<DateTime<Utc>>,
    /// Match sessions created at or before this timestamp.
    pub created_before: Option<DateTime<Utc>>,
    /// Match sessions whose client info contains this string.
    pub client_info: Option<String>,
    /// Match sessions that currently have (or do not have) active streams.
    pub has_active_streams: Option<bool>,
}

/// Fields to sort sessions by
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Sort order
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// Ascending order (smallest first).
    Ascending,
    /// Descending order (largest first).
    Descending,
}

/// Query response types
/// Response for session queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    /// The retrieved session aggregate.
    pub session: StreamSession,
}

/// Response for multiple sessions queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsResponse {
    /// Sessions returned in this page.
    pub sessions: Vec<StreamSession>,
    /// Total number of sessions matching the query, ignoring pagination.
    pub total_count: usize,
    /// Whether more sessions exist beyond this page.
    pub has_more: bool,
}

/// Response for stream queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamResponse {
    /// The retrieved stream entity.
    pub stream: Stream,
}

/// Response for multiple streams queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamsResponse {
    /// Streams returned by the query.
    pub streams: Vec<Stream>,
}

/// Response for frame queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FramesResponse {
    /// Frames returned in this page.
    pub frames: Vec<Frame>,
    /// Total number of frames matching the query, ignoring pagination.
    pub total_count: usize,
}

/// Response for health queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Health snapshot of the session.
    pub health: SessionHealth,
}

/// Response for session stats queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatsResponse {
    /// Identifier of the session whose statistics are reported.
    pub session_id: SessionIdDto,
    /// Aggregate domain statistics for the session.
    pub stats: SessionStats,
    /// Number of streams currently attached to the session.
    pub stream_count: usize,
    /// Number of streams currently in an active state.
    pub active_stream_count: usize,
    /// Timestamp when the session was created.
    pub created_at: DateTime<Utc>,
    /// Timestamp when the session was last updated.
    pub updated_at: DateTime<Utc>,
    /// Duration in milliseconds since session creation, or `None` if not yet completed.
    pub duration_ms: Option<i64>,
}

/// System statistics response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatsResponse {
    /// Total number of sessions ever created.
    pub total_sessions: u64,
    /// Number of sessions currently active.
    pub active_sessions: u64,
    /// Total number of streams ever created.
    pub total_streams: u64,
    /// Number of streams currently active.
    pub active_streams: u64,
    /// Total number of frames ever emitted.
    pub total_frames: u64,
    /// Total number of payload bytes ever emitted.
    pub total_bytes: u64,
    /// Average session lifetime in seconds across completed sessions.
    pub average_session_duration_seconds: f64,
    /// Throughput in frames emitted per second across the system.
    pub frames_per_second: f64,
    /// Throughput in payload bytes emitted per second across the system.
    pub bytes_per_second: f64,
    /// Number of seconds the system has been running.
    pub uptime_seconds: u64,
}
