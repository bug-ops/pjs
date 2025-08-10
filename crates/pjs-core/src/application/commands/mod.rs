//! Commands - Write operations that change system state

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use crate::domain::{
    value_objects::{SessionId, StreamId, Priority},
    entities::stream::StreamConfig,
    aggregates::stream_session::SessionConfig,
};

/// Create new streaming session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionCommand {
    pub config: SessionConfig,
    pub client_info: Option<String>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}

/// Activate an existing session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivateSessionCommand {
    pub session_id: SessionId,
}

/// Create new stream within a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStreamCommand {
    pub session_id: SessionId,
    pub source_data: JsonValue,
    pub config: Option<StreamConfig>,
}

/// Start streaming data for a specific stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartStreamCommand {
    pub session_id: SessionId,
    pub stream_id: StreamId,
}

/// Generate frames for a stream with priority filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateFramesCommand {
    pub session_id: SessionId,
    pub stream_id: StreamId,
    pub priority_threshold: Priority,
    pub max_frames: usize,
}

/// Complete a stream successfully
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteStreamCommand {
    pub session_id: SessionId,
    pub stream_id: StreamId,
    pub checksum: Option<String>,
}

/// Fail a stream with error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailStreamCommand {
    pub session_id: SessionId,
    pub stream_id: StreamId,
    pub error: String,
}

/// Cancel a stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelStreamCommand {
    pub session_id: SessionId,
    pub stream_id: StreamId,
}

/// Update stream configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStreamConfigCommand {
    pub session_id: SessionId,
    pub stream_id: StreamId,
    pub config: StreamConfig,
}

/// Close session gracefully
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseSessionCommand {
    pub session_id: SessionId,
}

/// Batch generate frames across multiple streams with priority
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchGenerateFramesCommand {
    pub session_id: SessionId,
    pub priority_threshold: Priority,
    pub max_frames: usize,
}

/// Adjust priority thresholds based on performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdjustPriorityThresholdCommand {
    pub session_id: SessionId,
    pub new_threshold: Priority,
    pub reason: String,
}