//! Commands - Write operations that change system state

use crate::application::dto::{PriorityDto, SessionIdDto, StreamIdDto};
use crate::domain::{aggregates::stream_session::SessionConfig, entities::stream::StreamConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Create new streaming session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionCommand {
    /// Session configuration (limits, priority thresholds, transport options).
    pub config: SessionConfig,
    /// Optional human-readable client identifier.
    pub client_info: Option<String>,
    /// Optional `User-Agent` header captured from the originating request.
    pub user_agent: Option<String>,
    /// Optional source IP address captured from the originating request.
    pub ip_address: Option<String>,
}

/// Create new stream within a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStreamCommand {
    /// Identifier of the parent session.
    pub session_id: SessionIdDto,
    /// JSON payload that will be decomposed into priority frames.
    pub source_data: JsonValue,
    /// Optional per-stream configuration overriding session defaults.
    pub config: Option<StreamConfig>,
}

/// Start streaming data for a specific stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartStreamCommand {
    /// Identifier of the parent session.
    pub session_id: SessionIdDto,
    /// Identifier of the stream to start.
    pub stream_id: StreamIdDto,
}

/// Generate frames for a stream with priority filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateFramesCommand {
    /// Identifier of the parent session.
    pub session_id: SessionIdDto,
    /// Identifier of the stream to generate frames for.
    pub stream_id: StreamIdDto,
    /// Minimum priority frames must satisfy to be emitted.
    pub priority_threshold: PriorityDto,
    /// Maximum number of frames to emit in this batch.
    pub max_frames: usize,
}

/// Complete a stream successfully
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteStreamCommand {
    /// Identifier of the parent session.
    pub session_id: SessionIdDto,
    /// Identifier of the stream being completed.
    pub stream_id: StreamIdDto,
    /// Optional payload checksum used to verify integrity.
    pub checksum: Option<String>,
}

/// Close session gracefully
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseSessionCommand {
    /// Identifier of the session to close.
    pub session_id: SessionIdDto,
}

/// Batch generate frames across multiple streams with priority
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchGenerateFramesCommand {
    /// Identifier of the parent session.
    pub session_id: SessionIdDto,
    /// Minimum priority frames must satisfy to be emitted.
    pub priority_threshold: PriorityDto,
    /// Maximum total number of frames to emit across all streams.
    pub max_frames: usize,
}
