//! Query handlers for read operations

use crate::{
    application::{ApplicationError, ApplicationResult, handlers::QueryHandler, queries::*},
    domain::{
        aggregates::StreamSession,
        entities::Stream,
        events::EventStore,
        ports::{StreamRepository, StreamStore},
    },
};
use async_trait::async_trait;
use std::sync::Arc;

/// Handler for session-related queries
#[derive(Debug)]
pub struct SessionQueryHandler<R>
where
    R: StreamRepository,
{
    repository: Arc<R>,
}

impl<R> SessionQueryHandler<R>
where
    R: StreamRepository,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<R> QueryHandler<GetSessionQuery, SessionResponse> for SessionQueryHandler<R>
where
    R: StreamRepository + Send + Sync,
{
    async fn handle(&self, query: GetSessionQuery) -> ApplicationResult<SessionResponse> {
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

#[async_trait]
impl<R> QueryHandler<GetActiveSessionsQuery, SessionsResponse> for SessionQueryHandler<R>
where
    R: StreamRepository + Send + Sync,
{
    async fn handle(&self, query: GetActiveSessionsQuery) -> ApplicationResult<SessionsResponse> {
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

#[async_trait]
impl<R> QueryHandler<GetSessionHealthQuery, HealthResponse> for SessionQueryHandler<R>
where
    R: StreamRepository + Send + Sync,
{
    async fn handle(&self, query: GetSessionHealthQuery) -> ApplicationResult<HealthResponse> {
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

#[async_trait]
impl<R> QueryHandler<SearchSessionsQuery, SessionsResponse> for SessionQueryHandler<R>
where
    R: StreamRepository + Send + Sync,
{
    async fn handle(&self, query: SearchSessionsQuery) -> ApplicationResult<SessionsResponse> {
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

impl<R> SessionQueryHandler<R>
where
    R: StreamRepository,
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
            && session.created_at() <= after {
                return false;
            }

        if let Some(before) = filters.created_before
            && session.created_at() >= before {
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
    R: StreamRepository,
    S: StreamStore,
{
    session_repository: Arc<R>,
    #[allow(dead_code)]
    stream_store: Arc<S>,
}

impl<R, S> StreamQueryHandler<R, S>
where
    R: StreamRepository,
    S: StreamStore,
{
    pub fn new(session_repository: Arc<R>, stream_store: Arc<S>) -> Self {
        Self {
            session_repository,
            stream_store,
        }
    }
}

#[async_trait]
impl<R, S> QueryHandler<GetStreamQuery, StreamResponse> for StreamQueryHandler<R, S>
where
    R: StreamRepository + Send + Sync,
    S: StreamStore + Send + Sync,
{
    async fn handle(&self, query: GetStreamQuery) -> ApplicationResult<StreamResponse> {
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

#[async_trait]
impl<R, S> QueryHandler<GetStreamsForSessionQuery, StreamsResponse> for StreamQueryHandler<R, S>
where
    R: StreamRepository + Send + Sync,
    S: StreamStore + Send + Sync,
{
    async fn handle(&self, query: GetStreamsForSessionQuery) -> ApplicationResult<StreamsResponse> {
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

#[async_trait]
impl<E> QueryHandler<GetSessionEventsQuery, EventsResponse> for EventQueryHandler<E>
where
    E: EventStore + Send + Sync,
{
    async fn handle(&self, query: GetSessionEventsQuery) -> ApplicationResult<EventsResponse> {
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

#[async_trait]
impl<E> QueryHandler<GetStreamEventsQuery, EventsResponse> for EventQueryHandler<E>
where
    E: EventStore + Send + Sync,
{
    async fn handle(&self, query: GetStreamEventsQuery) -> ApplicationResult<EventsResponse> {
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

/// Handler for system statistics
#[derive(Debug)]
pub struct SystemQueryHandler<R>
where
    R: StreamRepository,
{
    repository: Arc<R>,
}

impl<R> SystemQueryHandler<R>
where
    R: StreamRepository,
{
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<R> QueryHandler<GetSystemStatsQuery, SystemStatsResponse> for SystemQueryHandler<R>
where
    R: StreamRepository + Send + Sync,
{
    async fn handle(&self, _query: GetSystemStatsQuery) -> ApplicationResult<SystemStatsResponse> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        aggregates::{StreamSession, stream_session::SessionConfig},
        value_objects::SessionId,
    };
    use std::collections::HashMap;

    // Mock implementations for testing
    struct MockRepository {
        sessions: std::sync::Mutex<HashMap<SessionId, StreamSession>>,
    }

    impl MockRepository {
        fn new() -> Self {
            Self {
                sessions: std::sync::Mutex::new(HashMap::new()),
            }
        }

        fn add_session(&self, session: StreamSession) {
            self.sessions.lock().unwrap().insert(session.id(), session);
        }
    }

    #[async_trait]
    impl StreamRepository for MockRepository {
        async fn find_session(
            &self,
            session_id: SessionId,
        ) -> crate::domain::DomainResult<Option<StreamSession>> {
            Ok(self.sessions.lock().unwrap().get(&session_id).cloned())
        }

        async fn save_session(&self, session: StreamSession) -> crate::domain::DomainResult<()> {
            self.sessions.lock().unwrap().insert(session.id(), session);
            Ok(())
        }

        async fn remove_session(&self, session_id: SessionId) -> crate::domain::DomainResult<()> {
            self.sessions.lock().unwrap().remove(&session_id);
            Ok(())
        }

        async fn find_active_sessions(&self) -> crate::domain::DomainResult<Vec<StreamSession>> {
            Ok(self.sessions.lock().unwrap().values().cloned().collect())
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
        let query = GetSessionQuery { session_id: session_id.into() };
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
}
