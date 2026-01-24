//! Query handlers for read operations

use crate::{
    application::{ApplicationError, ApplicationResult, handlers::QueryHandlerGat, queries::*},
    domain::{
        aggregates::StreamSession,
        entities::Stream,
        events::EventStore,
        ports::{StreamRepositoryGat, StreamStoreGat},
    },
};
use std::sync::Arc;

/// Handler for session-related queries
#[derive(Debug)]
pub struct SessionQueryHandler<R>
where
    R: StreamRepositoryGat + 'static,
{
    repository: Arc<R>,
}

impl<R> SessionQueryHandler<R>
where
    R: StreamRepositoryGat + 'static,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

impl<R> QueryHandlerGat<GetSessionQuery> for SessionQueryHandler<R>
where
    R: StreamRepositoryGat + Send + Sync,
{
    type Response = SessionResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, query: GetSessionQuery) -> Self::HandleFuture<'_> {
        async move {
            let session = self
                .repository
                .find_session(query.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", query.session_id))
                })?;

            Ok(SessionResponse { session })
        }
    }
}

impl<R> QueryHandlerGat<GetActiveSessionsQuery> for SessionQueryHandler<R>
where
    R: StreamRepositoryGat + Send + Sync,
{
    type Response = SessionsResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, query: GetActiveSessionsQuery) -> Self::HandleFuture<'_> {
        async move {
            // TODO(CQ-010): Implement cursor-based pagination at repository layer
            // Current implementation loads all sessions into memory - inefficient with large datasets
            let mut sessions = self
                .repository
                .find_active_sessions()
                .await
                .map_err(ApplicationError::Domain)?;

            // Apply pagination
            let total_count = sessions.len();

            if let Some(offset) = query.offset {
                if offset < sessions.len() {
                    sessions = sessions.into_iter().skip(offset).collect();
                } else {
                    sessions.clear();
                }
            }

            if let Some(limit) = query.limit {
                sessions.truncate(limit);
            }

            Ok(SessionsResponse {
                sessions,
                total_count,
            })
        }
    }
}

impl<R> QueryHandlerGat<GetSessionHealthQuery> for SessionQueryHandler<R>
where
    R: StreamRepositoryGat + Send + Sync,
{
    type Response = HealthResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, query: GetSessionHealthQuery) -> Self::HandleFuture<'_> {
        async move {
            let session = self
                .repository
                .find_session(query.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", query.session_id))
                })?;

            let health = session.health_check();

            Ok(HealthResponse { health })
        }
    }
}

impl<R> QueryHandlerGat<SearchSessionsQuery> for SessionQueryHandler<R>
where
    R: StreamRepositoryGat + Send + Sync,
{
    type Response = SessionsResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, query: SearchSessionsQuery) -> Self::HandleFuture<'_> {
        async move {
            // Load all active sessions (in production, this would be more efficient with database filtering)
            let mut sessions = self
                .repository
                .find_active_sessions()
                .await
                .map_err(ApplicationError::Domain)?;

            // Apply filters
            sessions.retain(|session| self.matches_filters(session, &query.filters));

            // Apply sorting
            if let Some(sort_field) = &query.sort_by {
                let ascending = query
                    .sort_order
                    .as_ref()
                    .is_none_or(|order| matches!(order, SortOrder::Ascending));

                sessions.sort_by(|a, b| {
                    let cmp = match sort_field {
                        SessionSortField::CreatedAt => a.created_at().cmp(&b.created_at()),
                        SessionSortField::UpdatedAt => a.updated_at().cmp(&b.updated_at()),
                        SessionSortField::StreamCount => a.streams().len().cmp(&b.streams().len()),
                        SessionSortField::TotalBytes => {
                            a.stats().total_bytes.cmp(&b.stats().total_bytes)
                        }
                    };

                    if ascending { cmp } else { cmp.reverse() }
                });
            }

            // Apply pagination
            let total_count = sessions.len();

            if let Some(offset) = query.offset {
                if offset < sessions.len() {
                    sessions = sessions.into_iter().skip(offset).collect();
                } else {
                    sessions.clear();
                }
            }

            if let Some(limit) = query.limit {
                sessions.truncate(limit);
            }

            Ok(SessionsResponse {
                sessions,
                total_count,
            })
        }
    }
}

impl<R> SessionQueryHandler<R>
where
    R: StreamRepositoryGat + 'static,
{
    fn matches_filters(&self, session: &StreamSession, filters: &SessionFilters) -> bool {
        // State filter
        if let Some(ref state_filter) = filters.state {
            let state_str = format!("{:?}", session.state()).to_lowercase();
            if !state_str.contains(&state_filter.to_lowercase()) {
                return false;
            }
        }

        // Date range filters
        if let Some(after) = filters.created_after
            && session.created_at() <= after
        {
            return false;
        }

        if let Some(before) = filters.created_before
            && session.created_at() >= before
        {
            return false;
        }

        // Client info filter
        if let Some(ref client_filter) = filters.client_info {
            // Note: In real implementation, we'd need to store and access client info
            // This is a placeholder for the filtering logic
            let _ = client_filter; // Suppress unused warning
        }

        // Active streams filter
        if let Some(has_active) = filters.has_active_streams {
            let has_active_streams = session.streams().values().any(|stream| stream.is_active());
            if has_active != has_active_streams {
                return false;
            }
        }

        true
    }
}

/// Handler for stream-related queries
#[derive(Debug)]
pub struct StreamQueryHandler<R, S>
where
    R: StreamRepositoryGat + 'static,
    S: StreamStoreGat + 'static,
{
    session_repository: Arc<R>,
    #[allow(dead_code)]
    stream_store: Arc<S>,
}

impl<R, S> StreamQueryHandler<R, S>
where
    R: StreamRepositoryGat + 'static,
    S: StreamStoreGat + 'static,
{
    pub fn new(session_repository: Arc<R>, stream_store: Arc<S>) -> Self {
        Self {
            session_repository,
            stream_store,
        }
    }
}

impl<R, S> QueryHandlerGat<GetStreamQuery> for StreamQueryHandler<R, S>
where
    R: StreamRepositoryGat + Send + Sync,
    S: StreamStoreGat + Send + Sync,
{
    type Response = StreamResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, query: GetStreamQuery) -> Self::HandleFuture<'_> {
        async move {
            let session = self
                .session_repository
                .find_session(query.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", query.session_id))
                })?;

            let stream = session
                .get_stream(query.stream_id.into())
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Stream {} not found", query.stream_id))
                })?
                .clone();

            Ok(StreamResponse { stream })
        }
    }
}

impl<R, S> QueryHandlerGat<GetStreamsForSessionQuery> for StreamQueryHandler<R, S>
where
    R: StreamRepositoryGat + Send + Sync,
    S: StreamStoreGat + Send + Sync,
{
    type Response = StreamsResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, query: GetStreamsForSessionQuery) -> Self::HandleFuture<'_> {
        async move {
            let session = self
                .session_repository
                .find_session(query.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", query.session_id))
                })?;

            let streams: Vec<Stream> = session
                .streams()
                .values()
                .filter(|stream| query.include_inactive || stream.is_active())
                .cloned()
                .collect();

            Ok(StreamsResponse { streams })
        }
    }
}

/// Handler for event-related queries
#[derive(Debug)]
pub struct EventQueryHandler<E>
where
    E: EventStore,
{
    event_store: Arc<E>,
}

impl<E> EventQueryHandler<E>
where
    E: EventStore,
{
    pub fn new(event_store: Arc<E>) -> Self {
        Self { event_store }
    }
}

impl<E> QueryHandlerGat<GetSessionEventsQuery> for EventQueryHandler<E>
where
    E: EventStore + Send + Sync,
{
    type Response = EventsResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, query: GetSessionEventsQuery) -> Self::HandleFuture<'_> {
        async move {
            let mut events = self
                .event_store
                .get_events_for_session(query.session_id.into())
                .map_err(ApplicationError::Logic)?;

            // Apply time filter
            if let Some(since) = query.since {
                events.retain(|event| event.timestamp() > since);
            }

            // Apply event type filter
            if let Some(ref event_types) = query.event_types {
                events.retain(|event| event_types.contains(&event.event_type().to_string()));
            }

            let total_count = events.len();

            // Apply limit
            if let Some(limit) = query.limit {
                events.truncate(limit);
            }

            Ok(EventsResponse {
                events,
                total_count,
            })
        }
    }
}

impl<E> QueryHandlerGat<GetStreamEventsQuery> for EventQueryHandler<E>
where
    E: EventStore + Send + Sync,
{
    type Response = EventsResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, query: GetStreamEventsQuery) -> Self::HandleFuture<'_> {
        async move {
            let mut events = self
                .event_store
                .get_events_for_stream(query.stream_id.into())
                .map_err(ApplicationError::Logic)?;

            // Apply time filter
            if let Some(since) = query.since {
                events.retain(|event| event.timestamp() > since);
            }

            let total_count = events.len();

            // Apply limit
            if let Some(limit) = query.limit {
                events.truncate(limit);
            }

            Ok(EventsResponse {
                events,
                total_count,
            })
        }
    }
}

/// Handler for system statistics
#[derive(Debug)]
pub struct SystemQueryHandler<R>
where
    R: StreamRepositoryGat + 'static,
{
    repository: Arc<R>,
}

impl<R> SystemQueryHandler<R>
where
    R: StreamRepositoryGat + 'static,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

impl<R> QueryHandlerGat<GetSystemStatsQuery> for SystemQueryHandler<R>
where
    R: StreamRepositoryGat + Send + Sync,
{
    type Response = SystemStatsResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, _query: GetSystemStatsQuery) -> Self::HandleFuture<'_> {
        async move {
            let sessions = self
                .repository
                .find_active_sessions()
                .await
                .map_err(ApplicationError::Domain)?;

            let total_sessions = sessions.len() as u64;
            let active_sessions = sessions.iter().filter(|s| s.is_active()).count() as u64;

            let mut total_streams = 0u64;
            let mut active_streams = 0u64;
            let mut total_frames = 0u64;
            let mut total_bytes = 0u64;
            let mut total_duration_ms = 0f64;
            let mut completed_sessions = 0u64;

            for session in &sessions {
                let stats = session.stats();
                total_streams += stats.total_streams;
                active_streams += stats.active_streams;
                total_frames += stats.total_frames;
                total_bytes += stats.total_bytes;

                if let Some(duration) = session.duration() {
                    total_duration_ms += duration.num_milliseconds() as f64;
                    completed_sessions += 1;
                }
            }

            let average_session_duration_seconds = if completed_sessions > 0 {
                total_duration_ms / completed_sessions as f64 / 1000.0
            } else {
                0.0
            };

            // Calculate rates (simplified - would need time-based tracking in production)
            let uptime_seconds = 3600; // Placeholder - would track actual uptime
            let frames_per_second = total_frames as f64 / uptime_seconds as f64;
            let bytes_per_second = total_bytes as f64 / uptime_seconds as f64;

            Ok(SystemStatsResponse {
                total_sessions,
                active_sessions,
                total_streams,
                active_streams,
                total_frames,
                total_bytes,
                average_session_duration_seconds,
                frames_per_second,
                bytes_per_second,
                uptime_seconds: uptime_seconds as u64,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        aggregates::{StreamSession, stream_session::SessionConfig},
        value_objects::{SessionId, StreamId},
    };
    use std::collections::HashMap;

    // Mock implementations for testing
    struct MockRepository {
        sessions: parking_lot::Mutex<HashMap<SessionId, StreamSession>>,
    }

    impl MockRepository {
        fn new() -> Self {
            Self {
                sessions: parking_lot::Mutex::new(HashMap::new()),
            }
        }

        fn add_session(&self, session: StreamSession) {
            self.sessions.lock().insert(session.id(), session);
        }
    }

    impl StreamRepositoryGat for MockRepository {
        type FindSessionFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<Option<StreamSession>>>
            + Send
            + 'a
        where
            Self: 'a;

        type SaveSessionFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type RemoveSessionFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type FindActiveSessionsFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<Vec<StreamSession>>>
            + Send
            + 'a
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
    }

    struct MockStreamStore;

    impl StreamStoreGat for MockStreamStore {
        type StoreStreamFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type GetStreamFuture<'a>
            = impl std::future::Future<
                Output = crate::domain::DomainResult<Option<crate::domain::entities::Stream>>,
            > + Send
            + 'a
        where
            Self: 'a;

        type DeleteStreamFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type ListStreamsFuture<'a>
            = impl std::future::Future<
                Output = crate::domain::DomainResult<Vec<crate::domain::entities::Stream>>,
            > + Send
            + 'a
        where
            Self: 'a;

        fn store_stream(
            &self,
            _stream: crate::domain::entities::Stream,
        ) -> Self::StoreStreamFuture<'_> {
            async move { Ok(()) }
        }

        fn get_stream(&self, _stream_id: StreamId) -> Self::GetStreamFuture<'_> {
            async move { Ok(None) }
        }

        fn delete_stream(&self, _stream_id: StreamId) -> Self::DeleteStreamFuture<'_> {
            async move { Ok(()) }
        }

        fn list_streams_for_session(&self, _session_id: SessionId) -> Self::ListStreamsFuture<'_> {
            async move { Ok(vec![]) }
        }
    }

    struct MockEventStore;

    impl MockEventStore {
        fn new() -> Self {
            Self
        }
    }

    impl EventStore for MockEventStore {
        fn append_events(
            &mut self,
            _events: Vec<crate::domain::events::DomainEvent>,
        ) -> Result<(), String> {
            Ok(())
        }

        fn get_events_for_session(
            &self,
            _session_id: SessionId,
        ) -> Result<Vec<crate::domain::events::DomainEvent>, String> {
            Ok(vec![])
        }

        fn get_events_for_stream(
            &self,
            _stream_id: StreamId,
        ) -> Result<Vec<crate::domain::events::DomainEvent>, String> {
            Ok(vec![])
        }

        fn get_events_since(
            &self,
            _since: chrono::DateTime<chrono::Utc>,
        ) -> Result<Vec<crate::domain::events::DomainEvent>, String> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_get_session_query() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository.clone());

        // Create and add a session
        let mut session = StreamSession::new(SessionConfig::default());
        let _ = session.activate();
        let session_id = session.id();
        repository.add_session(session);

        // Query the session
        let query = GetSessionQuery {
            session_id: session_id.into(),
        };
        let result = handler.handle(query).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.session.id(), session_id);
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository);

        let query = GetSessionQuery {
            session_id: SessionId::new().into(),
        };
        let result = handler.handle(query).await;

        assert!(result.is_err());
        match result.err().unwrap() {
            ApplicationError::NotFound(_) => {}
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_get_active_sessions_query() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository.clone());

        // Add multiple sessions
        for i in 0..5 {
            let mut session = StreamSession::new(SessionConfig::default());
            if i < 3 {
                let _ = session.activate();
            }
            repository.add_session(session);
        }

        // Query active sessions
        let query = GetActiveSessionsQuery {
            offset: None,
            limit: None,
        };
        let result = handler.handle(query).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 5);
        assert_eq!(response.total_count, 5);
    }

    #[tokio::test]
    async fn test_get_active_sessions_with_pagination() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository.clone());

        // Add 10 sessions
        for _ in 0..10 {
            let mut session = StreamSession::new(SessionConfig::default());
            let _ = session.activate();
            repository.add_session(session);
        }

        // Query with pagination
        let query = GetActiveSessionsQuery {
            offset: Some(3),
            limit: Some(4),
        };
        let result = handler.handle(query).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 4);
        assert_eq!(response.total_count, 10);
    }

    #[tokio::test]
    async fn test_get_session_health_query() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository.clone());

        // Create and add a session
        let mut session = StreamSession::new(SessionConfig::default());
        let _ = session.activate();
        let session_id = session.id();
        repository.add_session(session);

        // Query session health
        let query = GetSessionHealthQuery {
            session_id: session_id.into(),
        };
        let result = handler.handle(query).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.health.is_healthy);
    }

    #[tokio::test]
    async fn test_session_handler_creation() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository.clone());

        // Test that handlers can be created successfully
        assert!(std::ptr::eq(
            handler.repository.as_ref(),
            repository.as_ref()
        ));
    }

    #[tokio::test]
    async fn test_stream_handler_creation() {
        let session_repository = Arc::new(MockRepository::new());
        let stream_store = Arc::new(MockStreamStore);
        let handler = StreamQueryHandler::new(session_repository.clone(), stream_store.clone());

        // Test that handlers can be created successfully
        assert!(std::ptr::eq(
            handler.session_repository.as_ref(),
            session_repository.as_ref()
        ));
    }

    #[tokio::test]
    async fn test_event_handler_creation() {
        let event_store = Arc::new(MockEventStore::new());
        let handler = EventQueryHandler::new(event_store.clone());

        // Test that handlers can be created successfully
        assert!(std::ptr::eq(
            handler.event_store.as_ref(),
            event_store.as_ref()
        ));
    }

    #[tokio::test]
    async fn test_system_handler_creation() {
        let repository = Arc::new(MockRepository::new());
        let handler = SystemQueryHandler::new(repository.clone());

        // Test that handlers can be created successfully
        assert!(std::ptr::eq(
            handler.repository.as_ref(),
            repository.as_ref()
        ));
    }

    // ===== Additional Query Handler Tests for CQ-003 (Coverage Improvement) =====

    #[tokio::test]
    async fn test_get_active_sessions_empty() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository);

        let query = GetActiveSessionsQuery {
            limit: Some(10),
            offset: Some(0),
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 0);
        assert_eq!(response.total_count, 0);
    }

    #[tokio::test]
    async fn test_get_active_sessions_with_limit() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository);

        let query = GetActiveSessionsQuery {
            limit: Some(5),
            offset: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_active_sessions_with_offset() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository);

        let query = GetActiveSessionsQuery {
            limit: None,
            offset: Some(10),
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_active_sessions_offset_beyond_count() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository);

        let query = GetActiveSessionsQuery {
            limit: Some(10),
            offset: Some(1000),
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 0);
    }

    #[tokio::test]
    async fn test_get_stream_not_found() {
        use crate::domain::value_objects::{SessionId, StreamId};

        let session_repository = Arc::new(MockRepository::new());
        let stream_store = Arc::new(MockStreamStore);
        let handler = StreamQueryHandler::new(session_repository, stream_store);

        let query = GetStreamQuery {
            session_id: SessionId::new().into(),
            stream_id: StreamId::new().into(),
        };

        let result: ApplicationResult<StreamResponse> =
            QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_session_query_not_found() {
        use crate::domain::value_objects::SessionId;

        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository);

        let query = GetSessionQuery {
            session_id: SessionId::new().into(),
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_err());
    }
}
