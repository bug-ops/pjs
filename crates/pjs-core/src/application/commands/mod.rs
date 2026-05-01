//! Commands - Write operations that change system state

use crate::application::dto::{PriorityDto, SessionIdDto, StreamIdDto};
use crate::domain::{aggregates::stream_session::SessionConfig, entities::stream::StreamConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Create new streaming session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionCommand {
    pub config: SessionConfig,
    pub client_info: Option<String>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}

/// Create new stream within a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStreamCommand {
    pub session_id: SessionIdDto,
    pub source_data: JsonValue,
    pub config: Option<StreamConfig>,
}

/// Start streaming data for a specific stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartStreamCommand {
    pub session_id: SessionIdDto,
    pub stream_id: StreamIdDto,
}

/// Generate frames for a stream with priority filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateFramesCommand {
    pub session_id: SessionIdDto,
    pub stream_id: StreamIdDto,
    pub priority_threshold: PriorityDto,
    pub max_frames: usize,
}

/// Complete a stream successfully
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteStreamCommand {
    pub session_id: SessionIdDto,
    pub stream_id: StreamIdDto,
    pub checksum: Option<String>,
}

/// Close session gracefully
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseSessionCommand {
    pub session_id: SessionIdDto,
}

/// Batch generate frames across multiple streams with priority
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchGenerateFramesCommand {
    pub session_id: SessionIdDto,
    pub priority_threshold: PriorityDto,
    pub max_frames: usize,
}
