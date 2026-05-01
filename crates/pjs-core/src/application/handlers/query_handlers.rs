//! Query handlers for read operations

use crate::{
    application::{ApplicationError, ApplicationResult, handlers::QueryHandlerGat, queries::*},
    domain::{
        aggregates::StreamSession,
        entities::Stream,
        ports::{
            Pagination, SessionQueryCriteria, SortOrder as RepoSortOrder, StreamRepositoryGat,
            StreamStoreGat,
        },
    },
};
use std::{marker::PhantomData, sync::Arc, time::Instant};

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
            const MAX_PAGE_SIZE: usize = 100;
            let limit = query.limit.unwrap_or(MAX_PAGE_SIZE).min(MAX_PAGE_SIZE);
            let offset = query.offset.unwrap_or(0);

            let pagination = Pagination {
                offset,
                limit,
                sort_by: None,
                sort_order: RepoSortOrder::Ascending,
            };

            let result = self
                .repository
                .find_sessions_by_criteria(SessionQueryCriteria::default(), pagination)
                .await
                .map_err(ApplicationError::Domain)?;

            Ok(SessionsResponse {
                sessions: result.sessions,
                total_count: result.total_count,
                has_more: result.has_more,
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

            // Apply pagination with bounded page size
            const MAX_PAGE_SIZE: usize = 100;
            let total_count = sessions.len();
            let offset = query.offset.unwrap_or(0);
            let limit = query.limit.unwrap_or(MAX_PAGE_SIZE).min(MAX_PAGE_SIZE);

            let sessions: Vec<_> = sessions.into_iter().skip(offset).take(limit).collect();
            let has_more = offset + sessions.len() < total_count;

            Ok(SessionsResponse {
                sessions,
                total_count,
                has_more,
            })
        }
    }
}

impl<R> QueryHandlerGat<GetSessionStatsQuery> for SessionQueryHandler<R>
where
    R: StreamRepositoryGat + Send + Sync,
{
    type Response = SessionStatsResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, query: GetSessionStatsQuery) -> Self::HandleFuture<'_> {
        async move {
            let session = self
                .repository
                .find_session(query.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", query.session_id))
                })?;

            let streams = session.streams();
            let active_stream_count = streams.values().filter(|s| s.is_active()).count();

            Ok(SessionStatsResponse {
                session_id: session.id().into(),
                stats: session.stats().clone(),
                stream_count: streams.len(),
                active_stream_count,
                created_at: session.created_at(),
                updated_at: session.updated_at(),
                duration_ms: session.duration().map(|d| d.num_milliseconds()),
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

        // Client info filter — case-insensitive substring match, consistent with the
        // state filter above.
        if let Some(ref client_filter) = filters.client_info {
            match session.client_info() {
                Some(info) => {
                    if !info
                        .to_lowercase()
                        .contains(&client_filter.to_lowercase() as &str)
                    {
                        return false;
                    }
                }
                None => return false,
            }
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
    _phantom: PhantomData<S>,
}

impl<R, S> StreamQueryHandler<R, S>
where
    R: StreamRepositoryGat + 'static,
    S: StreamStoreGat + 'static,
{
    pub fn new(session_repository: Arc<R>, _stream_store: Arc<S>) -> Self {
        Self {
            session_repository,
            _phantom: PhantomData,
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

impl<R, S> QueryHandlerGat<GetStreamFramesQuery> for StreamQueryHandler<R, S>
where
    R: StreamRepositoryGat + Send + Sync,
    S: StreamStoreGat + Send + Sync,
{
    type Response = FramesResponse;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, query: GetStreamFramesQuery) -> Self::HandleFuture<'_> {
        async move {
            let session = self
                .session_repository
                .find_session(query.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", query.session_id))
                })?;

            // Validate stream exists within the session.
            let _ = session.get_stream(query.stream_id.into()).ok_or_else(|| {
                ApplicationError::NotFound(format!("Stream {} not found", query.stream_id))
            })?;

            // The Stream entity does not persist generated frames — StreamStats only
            // tracks counts. Return an empty validated page until a FrameStore exists.
            // Filters are acknowledged but cannot be applied against an empty set.
            let _ = (query.since_sequence, query.priority_filter, query.limit);
            Ok(FramesResponse {
                frames: vec![],
                total_count: 0,
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
    started_at: Instant,
}

impl<R> SystemQueryHandler<R>
where
    R: StreamRepositoryGat + 'static,
{
    /// Create a new handler, recording `Instant::now()` as the startup time.
    pub fn new(repository: Arc<R>) -> Self {
        Self {
            repository,
            started_at: Instant::now(),
        }
    }

    /// Create a handler with an explicit startup instant.
    ///
    /// Useful when multiple handlers share a single process-start time.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let started_at = std::time::Instant::now();
    /// let handler = SystemQueryHandler::with_start_time(repo, started_at);
    /// ```
    pub fn with_start_time(repository: Arc<R>, started_at: Instant) -> Self {
        Self {
            repository,
            started_at,
        }
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

            // Floor to 1 to avoid divide-by-zero when the query runs immediately on startup.
            let uptime_seconds = self.started_at.elapsed().as_secs().max(1);
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
                uptime_seconds,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        aggregates::{StreamSession, stream_session::SessionConfig},
        ports::{
            Pagination, PriorityDistribution, SessionHealthSnapshot, SessionQueryCriteria,
            SessionQueryResult, StreamFilter, StreamStatistics, StreamStatus,
        },
        value_objects::{SessionId, StreamId},
    };
    use chrono::Utc;
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

        type FindSessionsByCriteriaFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<SessionQueryResult>>
            + Send
            + 'a
        where
            Self: 'a;

        type GetSessionHealthFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<SessionHealthSnapshot>>
            + Send
            + 'a
        where
            Self: 'a;

        type SessionExistsFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<bool>> + Send + 'a
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
                    last_activity: Utc::now(),
                    error_rate: 0.0,
                    metrics: HashMap::new(),
                })
            }
        }

        fn session_exists(&self, session_id: SessionId) -> Self::SessionExistsFuture<'_> {
            async move { Ok(self.sessions.lock().contains_key(&session_id)) }
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

        type ListStreamsForSessionFuture<'a>
            = impl std::future::Future<
                Output = crate::domain::DomainResult<Vec<crate::domain::entities::Stream>>,
            > + Send
            + 'a
        where
            Self: 'a;

        type FindStreamsBySessionFuture<'a>
            = impl std::future::Future<
                Output = crate::domain::DomainResult<Vec<crate::domain::entities::Stream>>,
            > + Send
            + 'a
        where
            Self: 'a;

        type UpdateStreamStatusFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type GetStreamStatisticsFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<StreamStatistics>>
            + Send
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

        fn list_streams_for_session(
            &self,
            _session_id: SessionId,
        ) -> Self::ListStreamsForSessionFuture<'_> {
            async move { Ok(vec![]) }
        }

        fn find_streams_by_session(
            &self,
            _session_id: SessionId,
            _filter: StreamFilter,
        ) -> Self::FindStreamsBySessionFuture<'_> {
            async move { Ok(vec![]) }
        }

        fn update_stream_status(
            &self,
            _stream_id: StreamId,
            _status: StreamStatus,
        ) -> Self::UpdateStreamStatusFuture<'_> {
            async move { Ok(()) }
        }

        fn get_stream_statistics(
            &self,
            _stream_id: StreamId,
        ) -> Self::GetStreamStatisticsFuture<'_> {
            async move {
                Ok(StreamStatistics {
                    total_frames: 0,
                    total_bytes: 0,
                    priority_distribution: PriorityDistribution::default(),
                    avg_frame_size: 0.0,
                    creation_time: Utc::now(),
                    completion_time: None,
                    processing_duration: None,
                })
            }
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
        assert!(response.has_more);
    }

    #[tokio::test]
    async fn test_get_active_sessions_last_page_has_more_false() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository.clone());

        for _ in 0..5 {
            let mut session = StreamSession::new(SessionConfig::default());
            let _ = session.activate();
            repository.add_session(session);
        }

        // offset=3, limit=4 → only 2 remain → last page
        let query = GetActiveSessionsQuery {
            offset: Some(3),
            limit: Some(4),
        };
        let response = handler.handle(query).await.unwrap();
        assert_eq!(response.sessions.len(), 2);
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_get_active_sessions_page_cap() {
        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository.clone());

        for _ in 0..110 {
            let mut session = StreamSession::new(SessionConfig::default());
            let _ = session.activate();
            repository.add_session(session);
        }

        // limit=200 must be capped to 100
        let query = GetActiveSessionsQuery {
            offset: Some(0),
            limit: Some(200),
        };
        let response = handler.handle(query).await.unwrap();
        assert!(response.sessions.len() <= 100);
        assert!(response.has_more);
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
    async fn test_system_handler_creation() {
        let repository = Arc::new(MockRepository::new());
        let handler = SystemQueryHandler::new(repository.clone());

        // Test that handlers can be created successfully
        assert!(std::ptr::eq(
            handler.repository.as_ref(),
            repository.as_ref()
        ));
    }

    #[tokio::test]
    async fn test_system_handler_real_uptime() {
        use std::time::{Duration, Instant};

        let repository = Arc::new(MockRepository::new());
        // Simulate a handler that started 10 seconds ago.
        let started_at = Instant::now() - Duration::from_secs(10);
        let handler = SystemQueryHandler::with_start_time(repository, started_at);

        let query = GetSystemStatsQuery {
            include_historical: false,
        };
        let result = QueryHandlerGat::handle(&handler, query).await.unwrap();

        assert!(
            result.uptime_seconds >= 10,
            "uptime_seconds should be at least 10, got {}",
            result.uptime_seconds
        );
    }

    #[tokio::test]
    async fn test_get_stream_frames_session_not_found() {
        use crate::domain::value_objects::{SessionId, StreamId};

        let session_repository = Arc::new(MockRepository::new());
        let stream_store = Arc::new(MockStreamStore);
        let handler = StreamQueryHandler::new(session_repository, stream_store);

        let query = GetStreamFramesQuery {
            session_id: SessionId::new().into(),
            stream_id: StreamId::new().into(),
            since_sequence: None,
            priority_filter: None,
            limit: None,
        };

        let result: ApplicationResult<FramesResponse> =
            QueryHandlerGat::handle(&handler, query).await;
        assert!(matches!(result, Err(ApplicationError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_get_stream_frames_stream_not_found() {
        use crate::domain::value_objects::StreamId;

        let session_repository = Arc::new(MockRepository::new());
        let mut session = StreamSession::new(SessionConfig::default());
        let _ = session.activate();
        let session_id = session.id();
        session_repository.add_session(session);

        let stream_store = Arc::new(MockStreamStore);
        let handler = StreamQueryHandler::new(session_repository, stream_store);

        let query = GetStreamFramesQuery {
            session_id: session_id.into(),
            stream_id: StreamId::new().into(),
            since_sequence: None,
            priority_filter: None,
            limit: None,
        };

        let result: ApplicationResult<FramesResponse> =
            QueryHandlerGat::handle(&handler, query).await;
        assert!(matches!(result, Err(ApplicationError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_get_stream_frames_returns_empty() {
        use crate::domain::value_objects::JsonData;

        let session_repository = Arc::new(MockRepository::new());
        let mut session = StreamSession::new(SessionConfig::default());
        let _ = session.activate();
        let session_id = session.id();
        let stream_id = session
            .create_stream(JsonData::from(serde_json::json!({"k": "v"})))
            .unwrap();
        session_repository.add_session(session);

        let stream_store = Arc::new(MockStreamStore);
        let handler = StreamQueryHandler::new(session_repository, stream_store);

        let query = GetStreamFramesQuery {
            session_id: session_id.into(),
            stream_id: stream_id.into(),
            since_sequence: None,
            priority_filter: None,
            limit: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await.unwrap();
        assert_eq!(result.frames.len(), 0);
        assert_eq!(result.total_count, 0);
    }

    #[tokio::test]
    async fn test_get_session_stats_not_found() {
        use crate::domain::value_objects::SessionId;

        let repository = Arc::new(MockRepository::new());
        let handler = SessionQueryHandler::new(repository);

        let query = GetSessionStatsQuery {
            session_id: SessionId::new().into(),
        };

        let result: ApplicationResult<SessionStatsResponse> =
            QueryHandlerGat::handle(&handler, query).await;
        assert!(matches!(result, Err(ApplicationError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_get_session_stats_returns_metadata() {
        use crate::domain::value_objects::JsonData;

        let repository = Arc::new(MockRepository::new());
        let mut session = StreamSession::new(SessionConfig::default());
        let _ = session.activate();
        let session_id = session.id();
        let created_at = session.created_at();
        // Add two streams so we can assert stream_count.
        let _ = session.create_stream(JsonData::from(serde_json::json!({"a": 1})));
        let _ = session.create_stream(JsonData::from(serde_json::json!({"b": 2})));
        repository.add_session(session);

        let handler = SessionQueryHandler::new(repository);

        let query = GetSessionStatsQuery {
            session_id: session_id.into(),
        };

        let result = QueryHandlerGat::handle(&handler, query).await.unwrap();
        assert_eq!(result.stream_count, 2);
        assert_eq!(result.created_at, created_at);
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

    // Helper to build an active session with optional client_info
    fn make_session(client_info: Option<&str>) -> StreamSession {
        let mut session = StreamSession::new(SessionConfig::default());
        let _ = session.activate();
        if let Some(info) = client_info {
            session.set_client_info(info.to_owned(), None, None);
        }
        session
    }

    #[tokio::test]
    async fn test_client_info_filter_matching_passes() {
        let repository = Arc::new(MockRepository::new());
        repository.add_session(make_session(Some("Mozilla/5.0 (compatible; TestBot/1.0)")));
        let handler = SessionQueryHandler::new(repository);

        // Use mixed-case filter to verify case-insensitive matching.
        let query = SearchSessionsQuery {
            filters: SessionFilters {
                client_info: Some("testbot".to_owned()),
                ..Default::default()
            },
            sort_by: None,
            sort_order: None,
            limit: None,
            offset: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 1);
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_client_info_filter_non_matching_rejected() {
        let repository = Arc::new(MockRepository::new());
        repository.add_session(make_session(Some("Mozilla/5.0 (compatible; TestBot/1.0)")));
        let handler = SessionQueryHandler::new(repository);

        let query = SearchSessionsQuery {
            filters: SessionFilters {
                client_info: Some("OtherClient".to_owned()),
                ..Default::default()
            },
            sort_by: None,
            sort_order: None,
            limit: None,
            offset: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 0);
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_client_info_filter_no_info_rejected() {
        let repository = Arc::new(MockRepository::new());
        repository.add_session(make_session(None));
        let handler = SessionQueryHandler::new(repository);

        let query = SearchSessionsQuery {
            filters: SessionFilters {
                client_info: Some("TestBot".to_owned()),
                ..Default::default()
            },
            sort_by: None,
            sort_order: None,
            limit: None,
            offset: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 0);
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_client_info_filter_none_passes_all() {
        let repository = Arc::new(MockRepository::new());
        repository.add_session(make_session(Some("SomeAgent/2.0")));
        repository.add_session(make_session(None));
        let handler = SessionQueryHandler::new(repository);

        let query = SearchSessionsQuery {
            filters: SessionFilters::default(),
            sort_by: None,
            sort_order: None,
            limit: None,
            offset: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 2);
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_client_info_filter_case_insensitive() {
        let repository = Arc::new(MockRepository::new());
        repository.add_session(make_session(Some("Mozilla/5.0 (compatible; TESTBOT/2.0)")));
        let handler = SessionQueryHandler::new(repository);

        // Filter uses lowercase while session value is uppercase — must still match.
        let query = SearchSessionsQuery {
            filters: SessionFilters {
                client_info: Some("testbot".to_owned()),
                ..Default::default()
            },
            sort_by: None,
            sort_order: None,
            limit: None,
            offset: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 1);
        assert!(!response.has_more);
    }
}
