//! Event Data Transfer Objects for serialization
//!
//! Handles serialization/deserialization of domain events while keeping
//! the domain layer clean of serialization concerns.

use crate::{
    application::dto::{
        id_dto::{SessionIdDto, StreamIdDto},
        priority_dto::{FromDto, ToDto},
    },
    domain::{
        DomainError,
        events::{DomainEvent, EventId, PerformanceMetrics},
        value_objects::{SessionId, StreamId},
    },
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Serializable representation of domain events
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum DomainEventDto {
    /// Session was activated and is ready to accept streams
    SessionActivated {
        session_id: SessionIdDto,
        timestamp: DateTime<Utc>,
    },

    /// Session was closed gracefully
    SessionClosed {
        session_id: SessionIdDto,
        timestamp: DateTime<Utc>,
    },

    /// Session expired due to timeout
    SessionExpired {
        session_id: SessionIdDto,
        timestamp: DateTime<Utc>,
    },

    /// Session was forcefully closed due to timeout
    SessionTimedOut {
        session_id: SessionIdDto,
        original_state: String, // SessionState serialized as string
        timeout_duration: u64,
        timestamp: DateTime<Utc>,
    },

    /// Session timeout was extended
    SessionTimeoutExtended {
        session_id: SessionIdDto,
        additional_seconds: u64,
        new_expires_at: DateTime<Utc>,
        timestamp: DateTime<Utc>,
    },

    /// New stream was created in the session
    StreamCreated {
        session_id: SessionIdDto,
        stream_id: StreamIdDto,
        timestamp: DateTime<Utc>,
    },

    /// Stream started sending data
    StreamStarted {
        session_id: SessionIdDto,
        stream_id: StreamIdDto,
        timestamp: DateTime<Utc>,
    },

    /// Stream completed successfully
    StreamCompleted {
        session_id: SessionIdDto,
        stream_id: StreamIdDto,
        timestamp: DateTime<Utc>,
    },

    /// Stream failed with error
    StreamFailed {
        session_id: SessionIdDto,
        stream_id: StreamIdDto,
        error: String,
        timestamp: DateTime<Utc>,
    },

    /// Stream was cancelled
    StreamCancelled {
        session_id: SessionIdDto,
        stream_id: StreamIdDto,
        timestamp: DateTime<Utc>,
    },

    /// Skeleton frame was generated for a stream
    SkeletonGenerated {
        session_id: SessionIdDto,
        stream_id: StreamIdDto,
        frame_size_bytes: u64,
        timestamp: DateTime<Utc>,
    },

    /// Patch frames were generated for a stream
    PatchFramesGenerated {
        session_id: SessionIdDto,
        stream_id: StreamIdDto,
        frame_count: usize,
        total_bytes: u64,
        highest_priority: u8,
        timestamp: DateTime<Utc>,
    },

    /// Multiple frames were batched for efficient sending
    FramesBatched {
        session_id: SessionIdDto,
        frame_count: usize,
        timestamp: DateTime<Utc>,
    },

    /// Priority threshold was adjusted for adaptive streaming
    PriorityThresholdAdjusted {
        session_id: SessionIdDto,
        old_threshold: u8,
        new_threshold: u8,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    /// Stream configuration was updated
    StreamConfigUpdated {
        session_id: SessionIdDto,
        stream_id: StreamIdDto,
        timestamp: DateTime<Utc>,
    },

    /// Performance metrics were recorded
    PerformanceMetricsRecorded {
        session_id: SessionIdDto,
        metrics: PerformanceMetricsDto,
        timestamp: DateTime<Utc>,
    },

    /// Backpressure signal received from client
    BackpressureReceived {
        session_id: SessionIdDto,
        stream_id: Option<StreamIdDto>,
        signal: String,
        timestamp: DateTime<Utc>,
    },

    /// Stream was paused due to flow control
    StreamPaused {
        session_id: SessionIdDto,
        stream_id: StreamIdDto,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    /// Stream was resumed after pause
    StreamResumed {
        session_id: SessionIdDto,
        stream_id: StreamIdDto,
        paused_duration_ms: u64,
        timestamp: DateTime<Utc>,
    },

    /// Flow control credits were updated
    CreditsUpdated {
        session_id: SessionIdDto,
        available_credits: usize,
        max_credits: usize,
        timestamp: DateTime<Utc>,
    },
}

/// Serializable representation of performance metrics
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerformanceMetricsDto {
    pub frames_per_second: f64,
    pub bytes_per_second: f64,
    pub average_frame_size: f64,
    pub priority_distribution: PriorityDistributionDto,
    pub latency_ms: Option<u64>,
}

/// Serializable representation of priority distribution
/// This is just an alias to the main PriorityDistribution from domain events
pub type PriorityDistributionDto = crate::domain::events::PriorityDistribution;

/// Serializable representation of event ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventIdDto {
    uuid: uuid::Uuid,
}

impl EventIdDto {
    /// Create from UUID
    pub fn new(uuid: uuid::Uuid) -> Self {
        Self { uuid }
    }

    /// Get UUID value
    pub fn uuid(self) -> uuid::Uuid {
        self.uuid
    }

    /// Generate new unique event ID
    pub fn generate() -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
        }
    }
}

impl std::fmt::Display for EventIdDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.uuid)
    }
}

// Conversion implementations

impl From<DomainEvent> for DomainEventDto {
    fn from(event: DomainEvent) -> Self {
        match event {
            DomainEvent::SessionActivated {
                session_id,
                timestamp,
            } => Self::SessionActivated {
                session_id: session_id.to_dto(),
                timestamp,
            },
            DomainEvent::SessionClosed {
                session_id,
                timestamp,
            } => Self::SessionClosed {
                session_id: session_id.to_dto(),
                timestamp,
            },
            DomainEvent::SessionExpired {
                session_id,
                timestamp,
            } => Self::SessionExpired {
                session_id: session_id.to_dto(),
                timestamp,
            },
            DomainEvent::StreamCreated {
                session_id,
                stream_id,
                timestamp,
            } => Self::StreamCreated {
                session_id: session_id.to_dto(),
                stream_id: stream_id.to_dto(),
                timestamp,
            },
            DomainEvent::StreamStarted {
                session_id,
                stream_id,
                timestamp,
            } => Self::StreamStarted {
                session_id: session_id.to_dto(),
                stream_id: stream_id.to_dto(),
                timestamp,
            },
            DomainEvent::StreamCompleted {
                session_id,
                stream_id,
                timestamp,
            } => Self::StreamCompleted {
                session_id: session_id.to_dto(),
                stream_id: stream_id.to_dto(),
                timestamp,
            },
            DomainEvent::StreamFailed {
                session_id,
                stream_id,
                error,
                timestamp,
            } => Self::StreamFailed {
                session_id: session_id.to_dto(),
                stream_id: stream_id.to_dto(),
                error,
                timestamp,
            },
            DomainEvent::StreamCancelled {
                session_id,
                stream_id,
                timestamp,
            } => Self::StreamCancelled {
                session_id: session_id.to_dto(),
                stream_id: stream_id.to_dto(),
                timestamp,
            },
            DomainEvent::SkeletonGenerated {
                session_id,
                stream_id,
                frame_size_bytes,
                timestamp,
            } => Self::SkeletonGenerated {
                session_id: session_id.to_dto(),
                stream_id: stream_id.to_dto(),
                frame_size_bytes,
                timestamp,
            },
            DomainEvent::PatchFramesGenerated {
                session_id,
                stream_id,
                frame_count,
                total_bytes,
                highest_priority,
                timestamp,
            } => Self::PatchFramesGenerated {
                session_id: session_id.to_dto(),
                stream_id: stream_id.to_dto(),
                frame_count,
                total_bytes,
                highest_priority,
                timestamp,
            },
            DomainEvent::FramesBatched {
                session_id,
                frame_count,
                timestamp,
            } => Self::FramesBatched {
                session_id: session_id.to_dto(),
                frame_count,
                timestamp,
            },
            DomainEvent::PriorityThresholdAdjusted {
                session_id,
                old_threshold,
                new_threshold,
                reason,
                timestamp,
            } => Self::PriorityThresholdAdjusted {
                session_id: session_id.to_dto(),
                old_threshold,
                new_threshold,
                reason,
                timestamp,
            },
            DomainEvent::StreamConfigUpdated {
                session_id,
                stream_id,
                timestamp,
            } => Self::StreamConfigUpdated {
                session_id: session_id.to_dto(),
                stream_id: stream_id.to_dto(),
                timestamp,
            },
            DomainEvent::PerformanceMetricsRecorded {
                session_id,
                metrics,
                timestamp,
            } => Self::PerformanceMetricsRecorded {
                session_id: session_id.to_dto(),
                metrics: metrics.into(),
                timestamp,
            },
            DomainEvent::SessionTimedOut {
                session_id,
                original_state,
                timeout_duration,
                timestamp,
            } => Self::SessionTimedOut {
                session_id: session_id.to_dto(),
                original_state: format!("{:?}", original_state),
                timeout_duration,
                timestamp,
            },
            DomainEvent::SessionTimeoutExtended {
                session_id,
                additional_seconds,
                new_expires_at,
                timestamp,
            } => Self::SessionTimeoutExtended {
                session_id: session_id.to_dto(),
                additional_seconds,
                new_expires_at,
                timestamp,
            },
            DomainEvent::BackpressureReceived {
                session_id,
                stream_id,
                signal,
                timestamp,
            } => Self::BackpressureReceived {
                session_id: session_id.to_dto(),
                stream_id: stream_id.map(|id| id.to_dto()),
                signal: format!("{:?}", signal),
                timestamp,
            },
            DomainEvent::StreamPaused {
                session_id,
                stream_id,
                reason,
                timestamp,
            } => Self::StreamPaused {
                session_id: session_id.to_dto(),
                stream_id: stream_id.to_dto(),
                reason,
                timestamp,
            },
            DomainEvent::StreamResumed {
                session_id,
                stream_id,
                paused_duration_ms,
                timestamp,
            } => Self::StreamResumed {
                session_id: session_id.to_dto(),
                stream_id: stream_id.to_dto(),
                paused_duration_ms,
                timestamp,
            },
            DomainEvent::CreditsUpdated {
                session_id,
                available_credits,
                max_credits,
                timestamp,
            } => Self::CreditsUpdated {
                session_id: session_id.to_dto(),
                available_credits,
                max_credits,
                timestamp,
            },
        }
    }
}

impl ToDto<DomainEventDto> for DomainEvent {
    fn to_dto(self) -> DomainEventDto {
        DomainEventDto::from(self)
    }
}

impl FromDto<DomainEventDto> for DomainEvent {
    type Error = DomainError;

    fn from_dto(dto: DomainEventDto) -> Result<Self, Self::Error> {
        match dto {
            DomainEventDto::SessionActivated {
                session_id,
                timestamp,
            } => Ok(Self::SessionActivated {
                session_id: SessionId::from_dto(session_id)?,
                timestamp,
            }),
            DomainEventDto::SessionClosed {
                session_id,
                timestamp,
            } => Ok(Self::SessionClosed {
                session_id: SessionId::from_dto(session_id)?,
                timestamp,
            }),
            DomainEventDto::SessionExpired {
                session_id,
                timestamp,
            } => Ok(Self::SessionExpired {
                session_id: SessionId::from_dto(session_id)?,
                timestamp,
            }),
            DomainEventDto::StreamCreated {
                session_id,
                stream_id,
                timestamp,
            } => Ok(Self::StreamCreated {
                session_id: SessionId::from_dto(session_id)?,
                stream_id: StreamId::from_dto(stream_id)?,
                timestamp,
            }),
            DomainEventDto::StreamStarted {
                session_id,
                stream_id,
                timestamp,
            } => Ok(Self::StreamStarted {
                session_id: SessionId::from_dto(session_id)?,
                stream_id: StreamId::from_dto(stream_id)?,
                timestamp,
            }),
            DomainEventDto::StreamCompleted {
                session_id,
                stream_id,
                timestamp,
            } => Ok(Self::StreamCompleted {
                session_id: SessionId::from_dto(session_id)?,
                stream_id: StreamId::from_dto(stream_id)?,
                timestamp,
            }),
            DomainEventDto::StreamFailed {
                session_id,
                stream_id,
                error,
                timestamp,
            } => Ok(Self::StreamFailed {
                session_id: SessionId::from_dto(session_id)?,
                stream_id: StreamId::from_dto(stream_id)?,
                error,
                timestamp,
            }),
            DomainEventDto::StreamCancelled {
                session_id,
                stream_id,
                timestamp,
            } => Ok(Self::StreamCancelled {
                session_id: SessionId::from_dto(session_id)?,
                stream_id: StreamId::from_dto(stream_id)?,
                timestamp,
            }),
            DomainEventDto::SkeletonGenerated {
                session_id,
                stream_id,
                frame_size_bytes,
                timestamp,
            } => Ok(Self::SkeletonGenerated {
                session_id: SessionId::from_dto(session_id)?,
                stream_id: StreamId::from_dto(stream_id)?,
                frame_size_bytes,
                timestamp,
            }),
            DomainEventDto::PatchFramesGenerated {
                session_id,
                stream_id,
                frame_count,
                total_bytes,
                highest_priority,
                timestamp,
            } => Ok(Self::PatchFramesGenerated {
                session_id: SessionId::from_dto(session_id)?,
                stream_id: StreamId::from_dto(stream_id)?,
                frame_count,
                total_bytes,
                highest_priority,
                timestamp,
            }),
            DomainEventDto::FramesBatched {
                session_id,
                frame_count,
                timestamp,
            } => Ok(Self::FramesBatched {
                session_id: SessionId::from_dto(session_id)?,
                frame_count,
                timestamp,
            }),
            DomainEventDto::PriorityThresholdAdjusted {
                session_id,
                old_threshold,
                new_threshold,
                reason,
                timestamp,
            } => Ok(Self::PriorityThresholdAdjusted {
                session_id: SessionId::from_dto(session_id)?,
                old_threshold,
                new_threshold,
                reason,
                timestamp,
            }),
            DomainEventDto::StreamConfigUpdated {
                session_id,
                stream_id,
                timestamp,
            } => Ok(Self::StreamConfigUpdated {
                session_id: SessionId::from_dto(session_id)?,
                stream_id: StreamId::from_dto(stream_id)?,
                timestamp,
            }),
            DomainEventDto::PerformanceMetricsRecorded {
                session_id,
                metrics,
                timestamp,
            } => Ok(Self::PerformanceMetricsRecorded {
                session_id: SessionId::from_dto(session_id)?,
                metrics: metrics.try_into().map_err(|_| {
                    DomainError::InvalidInput("Invalid performance metrics".to_string())
                })?,
                timestamp,
            }),
            DomainEventDto::SessionTimedOut {
                session_id,
                original_state,
                timeout_duration,
                timestamp,
            } => {
                // Parse SessionState from string - basic implementation
                let state = match original_state.as_str() {
                    "Initializing" => crate::domain::events::SessionState::Initializing,
                    "Active" => crate::domain::events::SessionState::Active,
                    "Closing" => crate::domain::events::SessionState::Closing,
                    "Completed" => crate::domain::events::SessionState::Completed,
                    "Failed" => crate::domain::events::SessionState::Failed,
                    _ => {
                        return Err(DomainError::InvalidInput(format!(
                            "Invalid session state: {}",
                            original_state
                        )));
                    }
                };
                Ok(Self::SessionTimedOut {
                    session_id: SessionId::from_dto(session_id)?,
                    original_state: state,
                    timeout_duration,
                    timestamp,
                })
            }
            DomainEventDto::SessionTimeoutExtended {
                session_id,
                additional_seconds,
                new_expires_at,
                timestamp,
            } => Ok(Self::SessionTimeoutExtended {
                session_id: SessionId::from_dto(session_id)?,
                additional_seconds,
                new_expires_at,
                timestamp,
            }),
            DomainEventDto::BackpressureReceived {
                session_id,
                stream_id,
                signal,
                timestamp,
            } => {
                let backpressure_signal = match signal.as_str() {
                    "Ok" => crate::domain::value_objects::BackpressureSignal::Ok,
                    "SlowDown" => crate::domain::value_objects::BackpressureSignal::SlowDown,
                    "Pause" => crate::domain::value_objects::BackpressureSignal::Pause,
                    _ => {
                        return Err(DomainError::InvalidInput(format!(
                            "Invalid backpressure signal: {}",
                            signal
                        )));
                    }
                };
                Ok(Self::BackpressureReceived {
                    session_id: SessionId::from_dto(session_id)?,
                    stream_id: stream_id.map(StreamId::from_dto).transpose()?,
                    signal: backpressure_signal,
                    timestamp,
                })
            }
            DomainEventDto::StreamPaused {
                session_id,
                stream_id,
                reason,
                timestamp,
            } => Ok(Self::StreamPaused {
                session_id: SessionId::from_dto(session_id)?,
                stream_id: StreamId::from_dto(stream_id)?,
                reason,
                timestamp,
            }),
            DomainEventDto::StreamResumed {
                session_id,
                stream_id,
                paused_duration_ms,
                timestamp,
            } => Ok(Self::StreamResumed {
                session_id: SessionId::from_dto(session_id)?,
                stream_id: StreamId::from_dto(stream_id)?,
                paused_duration_ms,
                timestamp,
            }),
            DomainEventDto::CreditsUpdated {
                session_id,
                available_credits,
                max_credits,
                timestamp,
            } => Ok(Self::CreditsUpdated {
                session_id: SessionId::from_dto(session_id)?,
                available_credits,
                max_credits,
                timestamp,
            }),
        }
    }
}

impl From<PerformanceMetrics> for PerformanceMetricsDto {
    fn from(metrics: PerformanceMetrics) -> Self {
        Self {
            frames_per_second: metrics.frames_per_second,
            bytes_per_second: metrics.bytes_per_second,
            average_frame_size: metrics.average_frame_size,
            priority_distribution: metrics.priority_distribution,
            latency_ms: metrics.latency_ms,
        }
    }
}

impl TryFrom<PerformanceMetricsDto> for PerformanceMetrics {
    type Error = DomainError;

    fn try_from(dto: PerformanceMetricsDto) -> Result<Self, Self::Error> {
        Ok(Self {
            frames_per_second: dto.frames_per_second,
            bytes_per_second: dto.bytes_per_second,
            average_frame_size: dto.average_frame_size,
            priority_distribution: dto.priority_distribution,
            latency_ms: dto.latency_ms,
        })
    }
}

// No conversion needed - PriorityDistributionDto is now an alias

impl From<EventId> for EventIdDto {
    fn from(event_id: EventId) -> Self {
        Self::new(event_id.inner())
    }
}

impl From<EventIdDto> for EventId {
    fn from(dto: EventIdDto) -> Self {
        EventId::from_uuid(dto.uuid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_domain_event_dto_conversion() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let domain_event = DomainEvent::StreamCreated {
            session_id,
            stream_id,
            timestamp,
        };

        // Convert to DTO
        let dto = domain_event.clone().to_dto();

        // Convert back to domain
        let converted = DomainEvent::from_dto(dto).unwrap();

        assert_eq!(domain_event, converted);
    }

    #[test]
    fn test_performance_metrics_dto_conversion() {
        let metrics = PerformanceMetrics {
            frames_per_second: 60.0,
            bytes_per_second: 1024.0,
            average_frame_size: 512.0,
            priority_distribution: PriorityDistributionDto::default(),
            latency_ms: Some(100),
        };

        let dto = PerformanceMetricsDto::from(metrics.clone());
        let converted = PerformanceMetrics::try_from(dto).unwrap();

        assert_eq!(metrics, converted);
    }

    #[test]
    fn test_event_dto_serialization() {
        let session_id = SessionId::new();
        let event_dto = DomainEventDto::SessionActivated {
            session_id: session_id.to_dto(),
            timestamp: Utc::now(),
        };

        let serialized = serde_json::to_string(&event_dto).unwrap();
        let deserialized: DomainEventDto = serde_json::from_str(&serialized).unwrap();

        assert_eq!(event_dto, deserialized);
    }

    #[test]
    fn test_event_id_dto_creation() {
        let uuid = uuid::Uuid::new_v4();
        let event_id_dto = EventIdDto::new(uuid);

        assert_eq!(event_id_dto.uuid(), uuid);
    }

    #[test]
    fn test_event_id_dto_generate() {
        let event_id1 = EventIdDto::generate();
        let event_id2 = EventIdDto::generate();

        assert_ne!(event_id1, event_id2);
    }

    #[test]
    fn test_event_id_dto_display() {
        let uuid = uuid::Uuid::new_v4();
        let event_id_dto = EventIdDto::new(uuid);

        assert_eq!(event_id_dto.to_string(), uuid.to_string());
    }

    #[test]
    fn test_event_id_dto_conversion() {
        let event_id = EventId::new();
        let dto = EventIdDto::from(event_id);
        let converted_back = EventId::from(dto);

        assert_eq!(event_id, converted_back);
    }

    #[test]
    fn test_all_domain_event_dto_variants() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        // Test SessionActivated
        let event1 = DomainEvent::SessionActivated {
            session_id,
            timestamp,
        };
        let dto1 = DomainEventDto::from(event1.clone());
        let converted1 = DomainEvent::from_dto(dto1).unwrap();
        assert_eq!(event1, converted1);

        // Test SessionClosed
        let event2 = DomainEvent::SessionClosed {
            session_id,
            timestamp,
        };
        let dto2 = DomainEventDto::from(event2.clone());
        let converted2 = DomainEvent::from_dto(dto2).unwrap();
        assert_eq!(event2, converted2);

        // Test SessionExpired
        let event3 = DomainEvent::SessionExpired {
            session_id,
            timestamp,
        };
        let dto3 = DomainEventDto::from(event3.clone());
        let converted3 = DomainEvent::from_dto(dto3).unwrap();
        assert_eq!(event3, converted3);

        // Test StreamCreated
        let event4 = DomainEvent::StreamCreated {
            session_id,
            stream_id,
            timestamp,
        };
        let dto4 = DomainEventDto::from(event4.clone());
        let converted4 = DomainEvent::from_dto(dto4).unwrap();
        assert_eq!(event4, converted4);

        // Test StreamStarted
        let event5 = DomainEvent::StreamStarted {
            session_id,
            stream_id,
            timestamp,
        };
        let dto5 = DomainEventDto::from(event5.clone());
        let converted5 = DomainEvent::from_dto(dto5).unwrap();
        assert_eq!(event5, converted5);

        // Test StreamCompleted
        let event6 = DomainEvent::StreamCompleted {
            session_id,
            stream_id,
            timestamp,
        };
        let dto6 = DomainEventDto::from(event6.clone());
        let converted6 = DomainEvent::from_dto(dto6).unwrap();
        assert_eq!(event6, converted6);
    }

    #[test]
    fn test_stream_failed_event_conversion() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();
        let error = "Test error message".to_string();

        let event = DomainEvent::StreamFailed {
            session_id,
            stream_id,
            error: error.clone(),
            timestamp,
        };

        let dto = DomainEventDto::from(event.clone());
        let converted = DomainEvent::from_dto(dto).unwrap();

        assert_eq!(event, converted);
    }

    #[test]
    fn test_performance_metrics_with_none_latency() {
        let metrics = PerformanceMetrics {
            frames_per_second: 30.0,
            bytes_per_second: 2048.0,
            average_frame_size: 1024.0,
            priority_distribution: PriorityDistributionDto::default(),
            latency_ms: None,
        };

        let dto = PerformanceMetricsDto::from(metrics.clone());
        let converted = PerformanceMetrics::try_from(dto).unwrap();

        assert_eq!(metrics, converted);
        assert_eq!(converted.latency_ms, None);
    }

    #[test]
    fn test_complex_event_serialization() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let event_dto = DomainEventDto::PatchFramesGenerated {
            session_id: session_id.to_dto(),
            stream_id: stream_id.to_dto(),
            frame_count: 42,
            total_bytes: 1024,
            highest_priority: 100,
            timestamp,
        };

        let serialized = serde_json::to_string(&event_dto).unwrap();
        let deserialized: DomainEventDto = serde_json::from_str(&serialized).unwrap();

        assert_eq!(event_dto, deserialized);
    }

    #[test]
    fn test_performance_metrics_dto_fields() {
        let dto = PerformanceMetricsDto {
            frames_per_second: 60.0,
            bytes_per_second: 4096.0,
            average_frame_size: 256.0,
            priority_distribution: PriorityDistributionDto::default(),
            latency_ms: Some(50),
        };

        assert_eq!(dto.frames_per_second, 60.0);
        assert_eq!(dto.bytes_per_second, 4096.0);
        assert_eq!(dto.average_frame_size, 256.0);
        assert_eq!(dto.latency_ms, Some(50));
    }

    #[test]
    fn test_priority_threshold_adjusted_event() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::PriorityThresholdAdjusted {
            session_id,
            old_threshold: 50,
            new_threshold: 75,
            reason: "Performance optimization".to_string(),
            timestamp,
        };

        let dto = DomainEventDto::from(event.clone());
        let converted = DomainEvent::from_dto(dto).unwrap();

        assert_eq!(event, converted);
    }

    #[test]
    fn test_event_id_dto_hash() {
        let uuid1 = uuid::Uuid::new_v4();
        let uuid2 = uuid::Uuid::new_v4();

        let event_id1 = EventIdDto::new(uuid1);
        let event_id2 = EventIdDto::new(uuid2);
        let event_id3 = EventIdDto::new(uuid1); // Same UUID as event_id1

        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(event_id1.clone());
        set.insert(event_id2);
        set.insert(event_id3);

        assert_eq!(set.len(), 2); // Only 2 unique UUIDs
    }

    #[test]
    fn test_event_dto_clone() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let original = DomainEventDto::SessionActivated {
            session_id: session_id.to_dto(),
            timestamp,
        };

        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_event_dto_debug() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEventDto::SessionActivated {
            session_id: session_id.to_dto(),
            timestamp,
        };

        let debug_str = format!("{:?}", event);
        assert!(debug_str.contains("SessionActivated"));
    }
}
