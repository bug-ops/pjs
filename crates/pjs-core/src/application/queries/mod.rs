//! Queries - Read operations that don't change system state

use crate::application::dto::{PriorityDto, SessionIdDto, StreamIdDto};
use crate::domain::{
    aggregates::{StreamSession, stream_session::SessionHealth},
    entities::{Frame, Stream},
    events::DomainEvent,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Get session information by ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSessionQuery {
    pub session_id: SessionIdDto,
}

/// Get all active sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetActiveSessionsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Get stream information by ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStreamQuery {
    pub session_id: SessionIdDto,
    pub stream_id: StreamIdDto,
}

/// Get all streams for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStreamsForSessionQuery {
    pub session_id: SessionIdDto,
    pub include_inactive: bool,
}

/// Get session health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSessionHealthQuery {
    pub session_id: SessionIdDto,
}

/// Get frames for a stream with filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStreamFramesQuery {
    pub session_id: SessionIdDto,
    pub stream_id: StreamIdDto,
    pub since_sequence: Option<u64>,
    pub priority_filter: Option<PriorityDto>,
    pub limit: Option<usize>,
}

/// Get session statistics and metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSessionStatsQuery {
    pub session_id: SessionIdDto,
}

/// Get system-wide statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSystemStatsQuery {
    pub include_historical: bool,
}

/// Get events for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSessionEventsQuery {
    pub session_id: SessionIdDto,
    pub since: Option<DateTime<Utc>>,
    pub event_types: Option<Vec<String>>,
    pub limit: Option<usize>,
}

/// Get events for a stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetStreamEventsQuery {
    pub stream_id: StreamIdDto,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
}

/// Search sessions by criteria
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSessionsQuery {
    pub filters: SessionFilters,
    pub sort_by: Option<SessionSortField>,
    pub sort_order: Option<SortOrder>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Session filtering criteria
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionFilters {
    pub state: Option<String>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub client_info: Option<String>,
    pub has_active_streams: Option<bool>,
}

/// Fields to sort sessions by
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionSortField {
    CreatedAt,
    UpdatedAt,
    StreamCount,
    TotalBytes,
}

/// Sort order
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Query response types
/// Response for session queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    pub session: StreamSession,
}

/// Response for multiple sessions queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsResponse {
    pub sessions: Vec<StreamSession>,
    pub total_count: usize,
}

/// Response for stream queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamResponse {
    pub stream: Stream,
}

/// Response for multiple streams queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamsResponse {
    pub streams: Vec<Stream>,
}

/// Response for frame queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FramesResponse {
    pub frames: Vec<Frame>,
    pub total_count: usize,
}

/// Response for health queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub health: SessionHealth,
}

/// Response for events queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsResponse {
    pub events: Vec<DomainEvent>,
    pub total_count: usize,
}

/// System statistics response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatsResponse {
    pub total_sessions: u64,
    pub active_sessions: u64,
    pub total_streams: u64,
    pub active_streams: u64,
    pub total_frames: u64,
    pub total_bytes: u64,
    pub average_session_duration_seconds: f64,
    pub frames_per_second: f64,
    pub bytes_per_second: f64,
    pub uptime_seconds: u64,
}

