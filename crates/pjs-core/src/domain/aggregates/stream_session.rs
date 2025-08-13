//! StreamSession aggregate root managing multiple streams

use crate::domain::{
    DomainError, DomainResult,
    entities::{Frame, Stream, stream::StreamConfig},
    events::DomainEvent,
    value_objects::{JsonData, Priority, SessionId, StreamId},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as SerdeValue;
use std::collections::{HashMap, VecDeque};

/// Temporary converter function until full refactor is complete
fn convert_serde_to_domain(value: &SerdeValue) -> JsonData {
    match value {
        SerdeValue::Null => JsonData::Null,
        SerdeValue::Bool(b) => JsonData::Bool(*b),
        SerdeValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonData::Integer(i)
            } else if let Some(f) = n.as_f64() {
                JsonData::Float(f)
            } else {
                JsonData::Integer(0)
            }
        }
        SerdeValue::String(s) => JsonData::String(s.clone()),
        SerdeValue::Array(arr) => {
            let values: Vec<JsonData> = arr.iter().map(convert_serde_to_domain).collect();
            JsonData::Array(values)
        }
        SerdeValue::Object(obj) => {
            let mut map = HashMap::new();
            for (key, value) in obj {
                map.insert(key.clone(), convert_serde_to_domain(value));
            }
            JsonData::Object(map)
        }
    }
}

/// Session state in its lifecycle
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session is being initialized
    Initializing,
    /// Session is active with streams
    Active,
    /// Session is gracefully closing
    Closing,
    /// Session completed successfully
    Completed,
    /// Session failed with error
    Failed,
}

/// Session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Maximum concurrent streams
    pub max_concurrent_streams: usize,
    /// Session timeout in seconds
    pub session_timeout_seconds: u64,
    /// Default stream configuration
    pub default_stream_config: StreamConfig,
    /// Enable session-level compression
    pub enable_compression: bool,
    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_concurrent_streams: 10,
            session_timeout_seconds: 3600, // 1 hour
            default_stream_config: StreamConfig::default(),
            enable_compression: true,
            metadata: HashMap::new(),
        }
    }
}

/// Session statistics and monitoring
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStats {
    pub total_streams: u64,
    pub active_streams: u64,
    pub completed_streams: u64,
    pub failed_streams: u64,
    pub total_frames: u64,
    pub total_bytes: u64,
    pub average_stream_duration_ms: f64,
}

/// StreamSession aggregate root - manages multiple prioritized streams
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSession {
    id: SessionId,
    state: SessionState,
    config: SessionConfig,
    stats: SessionStats,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,

    // Aggregate state
    streams: HashMap<StreamId, Stream>,
    pending_events: VecDeque<DomainEvent>,

    // Session metadata
    client_info: Option<String>,
    user_agent: Option<String>,
    ip_address: Option<String>,
}

impl StreamSession {
    /// Create new session
    pub fn new(config: SessionConfig) -> Self {
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(config.session_timeout_seconds as i64);

        Self {
            id: SessionId::new(),
            state: SessionState::Initializing,
            config,
            stats: SessionStats::default(),
            created_at: now,
            updated_at: now,
            expires_at,
            completed_at: None,
            streams: HashMap::new(),
            pending_events: VecDeque::new(),
            client_info: None,
            user_agent: None,
            ip_address: None,
        }
    }

    /// Get session ID
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// Get current state
    pub fn state(&self) -> &SessionState {
        &self.state
    }

    /// Get configuration
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Get statistics
    pub fn stats(&self) -> &SessionStats {
        &self.stats
    }

    /// Get creation timestamp
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Get last update timestamp
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Get expiration timestamp
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    /// Get completion timestamp
    pub fn completed_at(&self) -> Option<DateTime<Utc>> {
        self.completed_at
    }

    /// Get session duration if completed
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.completed_at.map(|end| end - self.created_at)
    }

    /// Check if session is expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if session is active
    pub fn is_active(&self) -> bool {
        matches!(self.state, SessionState::Active) && !self.is_expired()
    }

    /// Get all streams
    pub fn streams(&self) -> &HashMap<StreamId, Stream> {
        &self.streams
    }

    /// Get stream by ID
    pub fn get_stream(&self, stream_id: StreamId) -> Option<&Stream> {
        self.streams.get(&stream_id)
    }

    /// Get mutable stream by ID
    pub fn get_stream_mut(&mut self, stream_id: StreamId) -> Option<&mut Stream> {
        self.streams.get_mut(&stream_id)
    }

    /// Activate session
    pub fn activate(&mut self) -> DomainResult<()> {
        match self.state {
            SessionState::Initializing => {
                self.state = SessionState::Active;
                self.update_timestamp();

                self.add_event(DomainEvent::SessionActivated {
                    session_id: self.id,
                    timestamp: Utc::now(),
                });

                Ok(())
            }
            _ => Err(DomainError::InvalidStateTransition(format!(
                "Cannot activate session from state: {:?}",
                self.state
            ))),
        }
    }

    /// Create new stream in this session
    pub fn create_stream(&mut self, source_data: SerdeValue) -> DomainResult<StreamId> {
        if !self.is_active() {
            return Err(DomainError::InvalidSessionState(
                "Session is not active".to_string(),
            ));
        }

        if self.streams.len() >= self.config.max_concurrent_streams {
            return Err(DomainError::TooManyStreams(format!(
                "Maximum {} concurrent streams exceeded",
                self.config.max_concurrent_streams
            )));
        }

        // Convert serde_json::Value to domain JsonData
        let domain_data = convert_serde_to_domain(&source_data);

        let stream = Stream::new(
            self.id,
            domain_data,
            self.config.default_stream_config.clone(),
        );
        let stream_id = stream.id();

        self.streams.insert(stream_id, stream);
        self.stats.total_streams += 1;
        self.stats.active_streams += 1;
        self.update_timestamp();

        self.add_event(DomainEvent::StreamCreated {
            session_id: self.id,
            stream_id,
            timestamp: Utc::now(),
        });

        Ok(stream_id)
    }

    /// Start streaming for a specific stream
    pub fn start_stream(&mut self, stream_id: StreamId) -> DomainResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| DomainError::StreamNotFound(stream_id.to_string()))?;

        stream.start_streaming()?;
        self.update_timestamp();

        self.add_event(DomainEvent::StreamStarted {
            session_id: self.id,
            stream_id,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Complete a specific stream
    pub fn complete_stream(&mut self, stream_id: StreamId) -> DomainResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| DomainError::StreamNotFound(stream_id.to_string()))?;

        stream.complete()?;

        // Update session stats
        self.stats.active_streams = self.stats.active_streams.saturating_sub(1);
        self.stats.completed_streams += 1;

        // Update average duration
        if let Some(duration) = stream.duration() {
            let duration_ms = duration.num_milliseconds() as f64;
            self.stats.average_stream_duration_ms =
                (self.stats.average_stream_duration_ms + duration_ms) / 2.0;
        }

        self.update_timestamp();

        self.add_event(DomainEvent::StreamCompleted {
            session_id: self.id,
            stream_id,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Fail a specific stream
    pub fn fail_stream(&mut self, stream_id: StreamId, error: String) -> DomainResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| DomainError::StreamNotFound(stream_id.to_string()))?;

        stream.fail(error.clone())?;

        // Update session stats
        self.stats.active_streams = self.stats.active_streams.saturating_sub(1);
        self.stats.failed_streams += 1;

        self.update_timestamp();

        self.add_event(DomainEvent::StreamFailed {
            session_id: self.id,
            stream_id,
            error,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Create frames for all active streams based on priority
    pub fn create_priority_frames(&mut self, batch_size: usize) -> DomainResult<Vec<Frame>> {
        if !self.is_active() {
            return Err(DomainError::InvalidSessionState(
                "Session is not active".to_string(),
            ));
        }

        let mut all_frames = Vec::new();
        let mut frame_count = 0;

        // Collect frames from all active streams, sorted by priority
        let mut stream_frames: Vec<(Priority, StreamId, Frame)> = Vec::new();

        for (stream_id, stream) in &mut self.streams {
            if !stream.is_active() {
                continue;
            }

            // Try to create frames from this stream
            let frames = stream.create_patch_frames(Priority::BACKGROUND, 5)?;

            for frame in frames {
                let priority = frame.priority();
                stream_frames.push((priority, *stream_id, frame));
            }
        }

        // Sort by priority (descending)
        stream_frames.sort_by(|a, b| b.0.cmp(&a.0));

        // Take up to batch_size frames
        for (_, _, frame) in stream_frames.into_iter().take(batch_size) {
            all_frames.push(frame);
            frame_count += 1;
        }

        // Update session stats
        self.stats.total_frames += frame_count;
        self.update_timestamp();

        if !all_frames.is_empty() {
            self.add_event(DomainEvent::FramesBatched {
                session_id: self.id,
                frame_count: all_frames.len(),
                timestamp: Utc::now(),
            });
        }

        Ok(all_frames)
    }

    /// Close session gracefully
    pub fn close(&mut self) -> DomainResult<()> {
        match self.state {
            SessionState::Active => {
                self.state = SessionState::Closing;

                // Close all active streams
                let active_stream_ids: Vec<_> = self
                    .streams
                    .iter()
                    .filter(|(_, stream)| stream.is_active())
                    .map(|(id, _)| *id)
                    .collect();

                for stream_id in active_stream_ids {
                    if let Some(stream) = self.streams.get_mut(&stream_id) {
                        let _ = stream.cancel(); // Best effort
                    }
                }

                self.state = SessionState::Completed;
                self.completed_at = Some(Utc::now());
                self.update_timestamp();

                self.add_event(DomainEvent::SessionClosed {
                    session_id: self.id,
                    timestamp: Utc::now(),
                });

                Ok(())
            }
            _ => Err(DomainError::InvalidStateTransition(format!(
                "Cannot close session from state: {:?}",
                self.state
            ))),
        }
    }

    /// Force close expired session with proper cleanup
    pub fn force_close_expired(&mut self) -> DomainResult<bool> {
        if !self.is_expired() {
            return Ok(false);
        }

        // Force close regardless of current state
        let old_state = self.state.clone();
        self.state = SessionState::Failed;
        self.completed_at = Some(Utc::now());
        self.update_timestamp();

        // Force cancel all streams with timeout reason
        for stream in self.streams.values_mut() {
            let _ = stream.cancel(); // Best effort cleanup
        }

        // Clear stream collections for memory cleanup
        self.streams.clear();

        // Emit timeout event
        self.add_event(DomainEvent::SessionTimedOut {
            session_id: self.id,
            original_state: old_state,
            timeout_duration: self.config.session_timeout_seconds,
            timestamp: Utc::now(),
        });

        Ok(true)
    }

    /// Extend session timeout (if allowed)
    pub fn extend_timeout(&mut self, additional_seconds: u64) -> DomainResult<()> {
        if self.is_expired() {
            return Err(DomainError::InvalidStateTransition(
                "Cannot extend timeout for expired session".to_string(),
            ));
        }

        self.expires_at = self.expires_at + chrono::Duration::seconds(additional_seconds as i64);
        self.update_timestamp();

        self.add_event(DomainEvent::SessionTimeoutExtended {
            session_id: self.id,
            additional_seconds,
            new_expires_at: self.expires_at,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    /// Set client information
    pub fn set_client_info(
        &mut self,
        client_info: String,
        user_agent: Option<String>,
        ip_address: Option<String>,
    ) {
        self.client_info = Some(client_info);
        self.user_agent = user_agent;
        self.ip_address = ip_address;
        self.update_timestamp();
    }

    /// Get pending domain events
    pub fn pending_events(&self) -> &VecDeque<DomainEvent> {
        &self.pending_events
    }

    /// Take all pending events (clears the queue)
    pub fn take_events(&mut self) -> VecDeque<DomainEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// Check session health
    pub fn health_check(&self) -> SessionHealth {
        let active_count = self.streams.values().filter(|s| s.is_active()).count();
        let failed_count = self
            .streams
            .values()
            .filter(|s| {
                matches!(
                    s.state(),
                    crate::domain::entities::stream::StreamState::Failed
                )
            })
            .count();

        SessionHealth {
            is_healthy: self.is_active() && failed_count == 0,
            active_streams: active_count,
            failed_streams: failed_count,
            is_expired: self.is_expired(),
            uptime_seconds: (Utc::now() - self.created_at).num_seconds(),
        }
    }

    /// Private helper: Add domain event
    fn add_event(&mut self, event: DomainEvent) {
        self.pending_events.push_back(event);
    }

    /// Private helper: Update timestamp
    fn update_timestamp(&mut self) {
        self.updated_at = Utc::now();
    }
}

/// Session health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHealth {
    pub is_healthy: bool,
    pub active_streams: usize,
    pub failed_streams: usize,
    pub is_expired: bool,
    pub uptime_seconds: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation_and_activation() {
        let mut session = StreamSession::new(SessionConfig::default());

        assert_eq!(session.state(), &SessionState::Initializing);
        assert!(!session.is_active());

        assert!(session.activate().is_ok());
        assert_eq!(session.state(), &SessionState::Active);
        assert!(session.is_active());
    }

    #[test]
    fn test_stream_management() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        let source_data = serde_json::json!({
            "test": "data"
        });

        // Create stream
        let stream_id = session.create_stream(source_data).unwrap();
        assert_eq!(session.streams().len(), 1);
        assert_eq!(session.stats().total_streams, 1);
        assert_eq!(session.stats().active_streams, 1);

        // Start stream
        assert!(session.start_stream(stream_id).is_ok());

        // Complete stream
        assert!(session.complete_stream(stream_id).is_ok());
        assert_eq!(session.stats().active_streams, 0);
        assert_eq!(session.stats().completed_streams, 1);
    }

    #[test]
    fn test_concurrent_stream_limit() {
        let config = SessionConfig {
            max_concurrent_streams: 2,
            ..Default::default()
        };
        let mut session = StreamSession::new(config);
        assert!(session.activate().is_ok());

        let source_data = serde_json::json!({});

        // Create max streams
        assert!(session.create_stream(source_data.clone()).is_ok());
        assert!(session.create_stream(source_data.clone()).is_ok());

        // Should fail to create third stream
        assert!(session.create_stream(source_data).is_err());
    }

    #[test]
    fn test_session_expiration() {
        let config = SessionConfig {
            session_timeout_seconds: 1,
            ..Default::default()
        };
        let session = StreamSession::new(config);

        // Session should not be expired immediately
        assert!(!session.is_expired());

        // Would need to sleep for 1+ seconds to test expiration in real scenario
        // For unit test, we verify the expiration logic exists
        assert!(session.expires_at > session.created_at);
    }

    #[test]
    fn test_domain_events() {
        let mut session = StreamSession::new(SessionConfig::default());

        // Events should be generated for state transitions
        assert!(session.activate().is_ok());
        assert!(!session.pending_events().is_empty());

        let events = session.take_events();
        assert_eq!(events.len(), 1);

        // Events queue should be empty after taking
        assert!(session.pending_events().is_empty());
    }

    #[test]
    fn test_session_health() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        let health = session.health_check();
        assert!(health.is_healthy);
        assert_eq!(health.active_streams, 0);
        assert_eq!(health.failed_streams, 0);
        assert!(!health.is_expired);
        assert!(health.uptime_seconds >= 0);
    }
}
