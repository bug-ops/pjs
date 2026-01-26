//! Common test utilities and mock implementations
//!
//! Provides reusable GAT-based mocks for testing across multiple test files

#![allow(dead_code)]

use pjson_rs::domain::{
    aggregates::StreamSession,
    entities::Stream,
    events::DomainEvent,
    ports::{
        EventPublisherGat, Pagination, PriorityDistribution, SessionHealthSnapshot,
        SessionQueryCriteria, SessionQueryResult, StreamFilter, StreamRepositoryGat,
        StreamStatistics, StreamStatus, StreamStoreGat,
    },
    value_objects::{SessionId, StreamId},
};
use std::collections::HashMap;
use std::sync::Arc;

/// Thread-safe mock repository for StreamSession entities
pub struct MockRepository {
    sessions: parking_lot::Mutex<HashMap<SessionId, StreamSession>>,
}

impl MockRepository {
    pub fn new() -> Self {
        Self {
            sessions: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    pub fn with_session(session: StreamSession) -> Self {
        let mut sessions = HashMap::new();
        sessions.insert(session.id(), session);
        Self {
            sessions: parking_lot::Mutex::new(sessions),
        }
    }

    pub fn with_sessions(sessions_vec: Vec<StreamSession>) -> Self {
        let mut sessions = HashMap::new();
        for session in sessions_vec {
            sessions.insert(session.id(), session);
        }
        Self {
            sessions: parking_lot::Mutex::new(sessions),
        }
    }

    pub fn session_count(&self) -> usize {
        self.sessions.lock().len()
    }

    pub fn clear(&self) {
        self.sessions.lock().clear();
    }
}

impl StreamRepositoryGat for MockRepository {
    type FindSessionFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<Option<StreamSession>>>
        + Send
        + 'a
    where
        Self: 'a;

    type SaveSessionFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type RemoveSessionFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type FindActiveSessionsFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<Vec<StreamSession>>>
        + Send
        + 'a
    where
        Self: 'a;

    type FindSessionsByCriteriaFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<SessionQueryResult>>
        + Send
        + 'a
    where
        Self: 'a;

    type GetSessionHealthFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<SessionHealthSnapshot>>
        + Send
        + 'a
    where
        Self: 'a;

    type SessionExistsFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<bool>> + Send + 'a
    where
        Self: 'a;

    fn find_session(&self, session_id: SessionId) -> Self::FindSessionFuture<'_> {
        async move { Ok(self.sessions.lock().get(&session_id).cloned()) }
    }

    fn save_session(&self, session: StreamSession) -> Self::SaveSessionFuture<'_> {
        async move {
            self.sessions.lock().insert(session.id(), session);
            Ok(())
        }
    }

    fn remove_session(&self, session_id: SessionId) -> Self::RemoveSessionFuture<'_> {
        async move {
            self.sessions.lock().remove(&session_id);
            Ok(())
        }
    }

    fn find_active_sessions(&self) -> Self::FindActiveSessionsFuture<'_> {
        async move { Ok(self.sessions.lock().values().cloned().collect()) }
    }

    fn find_sessions_by_criteria(
        &self,
        _criteria: SessionQueryCriteria,
        pagination: Pagination,
    ) -> Self::FindSessionsByCriteriaFuture<'_> {
        async move {
            let sessions: Vec<_> = self.sessions.lock().values().cloned().collect();
            let total_count = sessions.len();
            let paginated: Vec<_> = sessions
                .into_iter()
                .skip(pagination.offset)
                .take(pagination.limit)
                .collect();
            let has_more = pagination.offset + paginated.len() < total_count;
            Ok(SessionQueryResult {
                sessions: paginated,
                total_count,
                has_more,
                query_duration_ms: 0,
                scan_limit_reached: false,
            })
        }
    }

    fn get_session_health(&self, session_id: SessionId) -> Self::GetSessionHealthFuture<'_> {
        async move {
            Ok(SessionHealthSnapshot {
                session_id,
                is_healthy: true,
                active_streams: 0,
                total_frames: 0,
                last_activity: chrono::Utc::now(),
                error_rate: 0.0,
                metrics: HashMap::new(),
            })
        }
    }

    fn session_exists(&self, session_id: SessionId) -> Self::SessionExistsFuture<'_> {
        async move { Ok(self.sessions.lock().contains_key(&session_id)) }
    }
}

/// Mock event publisher that tracks published events
pub struct MockEventPublisher {
    events: parking_lot::Mutex<Vec<DomainEvent>>,
}

impl MockEventPublisher {
    pub fn new() -> Self {
        Self {
            events: parking_lot::Mutex::new(Vec::new()),
        }
    }

    pub fn event_count(&self) -> usize {
        self.events.lock().len()
    }

    pub fn clear(&self) {
        self.events.lock().clear();
    }

    pub fn get_events(&self) -> Vec<DomainEvent> {
        self.events.lock().clone()
    }
}

impl EventPublisherGat for MockEventPublisher {
    type PublishFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type PublishBatchFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    fn publish(&self, event: DomainEvent) -> Self::PublishFuture<'_> {
        async move {
            self.events.lock().push(event);
            Ok(())
        }
    }

    fn publish_batch(&self, events: Vec<DomainEvent>) -> Self::PublishBatchFuture<'_> {
        async move {
            self.events.lock().extend(events);
            Ok(())
        }
    }
}

/// Mock stream store for Stream entities
pub struct MockStreamStore {
    streams: parking_lot::Mutex<HashMap<StreamId, Stream>>,
}

impl MockStreamStore {
    pub fn new() -> Self {
        Self {
            streams: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    pub fn with_stream(stream: Stream) -> Self {
        let mut streams = HashMap::new();
        streams.insert(stream.id(), stream);
        Self {
            streams: parking_lot::Mutex::new(streams),
        }
    }

    pub fn with_streams(streams_vec: Vec<Stream>) -> Self {
        let mut streams = HashMap::new();
        for stream in streams_vec {
            streams.insert(stream.id(), stream);
        }
        Self {
            streams: parking_lot::Mutex::new(streams),
        }
    }

    pub fn stream_count(&self) -> usize {
        self.streams.lock().len()
    }

    pub fn clear(&self) {
        self.streams.lock().clear();
    }
}

impl StreamStoreGat for MockStreamStore {
    type StoreStreamFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type GetStreamFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<Option<Stream>>>
        + Send
        + 'a
    where
        Self: 'a;

    type DeleteStreamFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type ListStreamsForSessionFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<Vec<Stream>>> + Send + 'a
    where
        Self: 'a;

    type FindStreamsBySessionFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<Vec<Stream>>> + Send + 'a
    where
        Self: 'a;

    type UpdateStreamStatusFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type GetStreamStatisticsFuture<'a>
        = impl std::future::Future<Output = pjson_rs::domain::DomainResult<StreamStatistics>>
        + Send
        + 'a
    where
        Self: 'a;

    fn store_stream(&self, stream: Stream) -> Self::StoreStreamFuture<'_> {
        async move {
            self.streams.lock().insert(stream.id(), stream);
            Ok(())
        }
    }

    fn get_stream(&self, stream_id: StreamId) -> Self::GetStreamFuture<'_> {
        async move { Ok(self.streams.lock().get(&stream_id).cloned()) }
    }

    fn delete_stream(&self, stream_id: StreamId) -> Self::DeleteStreamFuture<'_> {
        async move {
            self.streams.lock().remove(&stream_id);
            Ok(())
        }
    }

    fn list_streams_for_session(
        &self,
        _session_id: SessionId,
    ) -> Self::ListStreamsForSessionFuture<'_> {
        async move { Ok(self.streams.lock().values().cloned().collect()) }
    }

    fn find_streams_by_session(
        &self,
        _session_id: SessionId,
        _filter: StreamFilter,
    ) -> Self::FindStreamsBySessionFuture<'_> {
        async move { Ok(self.streams.lock().values().cloned().collect()) }
    }

    fn update_stream_status(
        &self,
        _stream_id: StreamId,
        _status: StreamStatus,
    ) -> Self::UpdateStreamStatusFuture<'_> {
        async move { Ok(()) }
    }

    fn get_stream_statistics(&self, _stream_id: StreamId) -> Self::GetStreamStatisticsFuture<'_> {
        async move {
            Ok(StreamStatistics {
                total_frames: 0,
                total_bytes: 0,
                priority_distribution: PriorityDistribution::default(),
                avg_frame_size: 0.0,
                creation_time: chrono::Utc::now(),
                completion_time: None,
                processing_duration: None,
            })
        }
    }
}

/// Builder for creating test sessions with custom configuration
pub struct SessionBuilder {
    max_concurrent_streams: usize,
    timeout_seconds: u64,
    enable_compression: bool,
}

impl SessionBuilder {
    pub fn new() -> Self {
        Self {
            max_concurrent_streams: 10,
            timeout_seconds: 3600,
            enable_compression: true,
        }
    }

    pub fn max_concurrent_streams(mut self, value: usize) -> Self {
        self.max_concurrent_streams = value;
        self
    }

    pub fn timeout_seconds(mut self, value: u64) -> Self {
        self.timeout_seconds = value;
        self
    }

    pub fn enable_compression(mut self, value: bool) -> Self {
        self.enable_compression = value;
        self
    }

    pub fn build(self) -> StreamSession {
        use pjson_rs::domain::aggregates::stream_session::SessionConfig;

        let config = SessionConfig {
            max_concurrent_streams: self.max_concurrent_streams,
            session_timeout_seconds: self.timeout_seconds,
            default_stream_config: Default::default(),
            enable_compression: self.enable_compression,
            metadata: Default::default(),
        };

        let mut session = StreamSession::new(config);
        session.activate().expect("Failed to activate test session");
        session
    }
}

/// Builder for creating test streams with custom data
pub struct StreamBuilder {
    data: serde_json::Value,
    session_id: SessionId,
}

impl StreamBuilder {
    pub fn new() -> Self {
        Self {
            data: serde_json::json!({}),
            session_id: SessionId::new(),
        }
    }

    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = data;
        self
    }

    pub fn with_session_id(mut self, session_id: SessionId) -> Self {
        self.session_id = session_id;
        self
    }

    pub fn build(self) -> Stream {
        use pjson_rs::domain::entities::stream::StreamConfig;
        Stream::new(self.session_id, self.data.into(), StreamConfig::default())
    }
}

/// Creates a test app state with mock dependencies
#[cfg(feature = "http-server")]
pub fn create_test_app_state() -> pjson_rs::infrastructure::http::axum_adapter::PjsAppState<
    MockRepository,
    MockEventPublisher,
    MockStreamStore,
> {
    use pjson_rs::infrastructure::http::axum_adapter::PjsAppState;

    let repository = Arc::new(MockRepository::new());
    let event_publisher = Arc::new(MockEventPublisher::new());
    let stream_store = Arc::new(MockStreamStore::new());

    PjsAppState::new(repository, event_publisher, stream_store)
}

/// Creates a test app state with pre-populated session
#[cfg(feature = "http-server")]
pub fn create_test_app_state_with_session(
    session: StreamSession,
) -> pjson_rs::infrastructure::http::axum_adapter::PjsAppState<
    MockRepository,
    MockEventPublisher,
    MockStreamStore,
> {
    use pjson_rs::infrastructure::http::axum_adapter::PjsAppState;

    let repository = Arc::new(MockRepository::with_session(session));
    let event_publisher = Arc::new(MockEventPublisher::new());
    let stream_store = Arc::new(MockStreamStore::new());

    PjsAppState::new(repository, event_publisher, stream_store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_repository_save_and_find() {
        let repo = MockRepository::new();
        let session = SessionBuilder::new().build();
        let session_id = session.id();

        repo.save_session(session).await.unwrap();
        let found = repo.find_session(session_id).await.unwrap();

        assert!(found.is_some());
    }

    #[tokio::test]
    async fn test_mock_event_publisher_tracks_events() {
        use pjson_rs::domain::events::DomainEvent;
        use pjson_rs::domain::value_objects::SessionId;

        let publisher = MockEventPublisher::new();
        let event = DomainEvent::SessionClosed {
            session_id: SessionId::new(),
            timestamp: chrono::Utc::now(),
        };

        publisher.publish(event).await.unwrap();
        assert_eq!(publisher.event_count(), 1);
    }

    #[tokio::test]
    async fn test_session_builder() {
        let session = SessionBuilder::new()
            .max_concurrent_streams(5)
            .timeout_seconds(1800)
            .build();

        assert_eq!(session.config().max_concurrent_streams, 5);
        assert_eq!(session.config().session_timeout_seconds, 1800);
    }

    #[tokio::test]
    async fn test_stream_builder() {
        let stream = StreamBuilder::new()
            .with_data(serde_json::json!({"test": "value"}))
            .build();

        assert!(!stream.id().to_string().is_empty());
    }
}
