//! Query handlers for read operations

use crate::{
    application::{ApplicationError, ApplicationResult, handlers::QueryHandlerGat, queries::*},
    domain::{
        SessionState,
        entities::Stream,
        ports::{
            FrameStoreGat, Pagination, SessionQueryCriteria, SortOrder as RepoSortOrder,
            StreamRepositoryGat, StreamStoreGat,
        },
    },
};
use std::{marker::PhantomData, sync::Arc, time::Instant};

/// Hard cap on frames returned by `GET /streams/{id}/frames`, matching the
/// shared pagination ceiling. Defends the HTTP layer against a client that
/// passes an oversized `limit`.
const MAX_FRAMES_PAGE_SIZE: usize = crate::domain::config::MAX_PAGINATION_LIMIT;

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
    /// Construct a handler that reads sessions from `repository`.
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
            const MAX_PAGE_SIZE: usize = 100;
            let limit = query.limit.unwrap_or(MAX_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
            let offset = query
                .offset
                .unwrap_or(0)
                .min(crate::domain::config::MAX_PAGINATION_OFFSET);

            // With no explicit state filter, default to the same active-only
            // scope the legacy `find_active_sessions()` provided (state ==
            // Active and not expired), so the endpoint's default result set
            // is unchanged. An explicit `filters.state` opts out of that
            // default and matches only the requested state(s), expired or not.
            let (states, exclude_expired) = match query.filters.state {
                Some(state) => (Some(vec![state.as_str().to_string()]), false),
                None => (Some(vec![SessionState::Active.as_str().to_string()]), true),
            };

            let criteria = SessionQueryCriteria {
                states,
                exclude_expired,
                created_after: query.filters.created_after,
                created_before: query.filters.created_before,
                client_info_pattern: query.filters.client_info,
                has_active_streams: query.filters.has_active_streams,
                ..SessionQueryCriteria::default()
            };

            let sort_by = query.sort_by.map(|field| {
                match field {
                    SessionSortField::CreatedAt => "created_at",
                    SessionSortField::UpdatedAt => "updated_at",
                    SessionSortField::StreamCount => "stream_count",
                    SessionSortField::TotalBytes => "total_bytes",
                }
                .to_string()
            });
            let sort_order = match query.sort_order {
                Some(SortOrder::Descending) => RepoSortOrder::Descending,
                _ => RepoSortOrder::Ascending,
            };

            let pagination = Pagination {
                offset,
                limit,
                sort_by,
                sort_order,
            };

            let result = self
                .repository
                .find_sessions_by_criteria(criteria, pagination)
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

/// Handler for stream-related queries
#[derive(Debug)]
pub struct StreamQueryHandler<R, S, F>
where
    R: StreamRepositoryGat + 'static,
    S: StreamStoreGat + 'static,
    F: FrameStoreGat + 'static,
{
    session_repository: Arc<R>,
    frame_store: Arc<F>,
    _phantom: PhantomData<S>,
}

impl<R, S, F> StreamQueryHandler<R, S, F>
where
    R: StreamRepositoryGat + 'static,
    S: StreamStoreGat + 'static,
    F: FrameStoreGat + 'static,
{
    /// Construct a system-level query handler from the supplied dependencies.
    pub fn new(session_repository: Arc<R>, _stream_store: Arc<S>, frame_store: Arc<F>) -> Self {
        Self {
            session_repository,
            frame_store,
            _phantom: PhantomData,
        }
    }
}

impl<R, S, F> QueryHandlerGat<GetStreamQuery> for StreamQueryHandler<R, S, F>
where
    R: StreamRepositoryGat + Send + Sync,
    S: StreamStoreGat + Send + Sync,
    F: FrameStoreGat + Send + Sync,
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
                .stream(query.stream_id.into())
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Stream {} not found", query.stream_id))
                })?
                .clone();

            Ok(StreamResponse { stream })
        }
    }
}

impl<R, S, F> QueryHandlerGat<GetStreamsForSessionQuery> for StreamQueryHandler<R, S, F>
where
    R: StreamRepositoryGat + Send + Sync,
    S: StreamStoreGat + Send + Sync,
    F: FrameStoreGat + Send + Sync,
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

impl<R, S, F> QueryHandlerGat<GetStreamFramesQuery> for StreamQueryHandler<R, S, F>
where
    R: StreamRepositoryGat + Send + Sync,
    S: StreamStoreGat + Send + Sync,
    F: FrameStoreGat + Send + Sync,
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
            session.stream(query.stream_id.into()).ok_or_else(|| {
                ApplicationError::NotFound(format!("Stream {} not found", query.stream_id))
            })?;

            // Cap requested page size to MAX_FRAMES_PAGE_SIZE so a client cannot
            // ask for an unbounded response. Honor None as "give me everything
            // up to the cap".
            let limit = Some(
                query
                    .limit
                    .map(|l| l.min(MAX_FRAMES_PAGE_SIZE))
                    .unwrap_or(MAX_FRAMES_PAGE_SIZE),
            );
            let priority_filter = query
                .priority_filter
                .map(crate::domain::value_objects::Priority::try_from)
                .transpose()
                .map_err(ApplicationError::Domain)?;

            let page = self
                .frame_store
                .get_frames(
                    query.stream_id.into(),
                    query.since_sequence,
                    priority_filter,
                    limit,
                )
                .await
                .map_err(ApplicationError::Domain)?;

            Ok(FramesResponse {
                frames: page.frames,
                total_count: page.total_matching,
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
            SessionQueryResult, StreamFilter, StreamStatistics, StreamStatus, TimeProvider,
        },
        value_objects::{SessionId, StreamId},
    };
    use chrono::Utc;
    use std::collections::HashMap;

    /// Mirrors `GatInMemoryStreamRepository::matches_criteria` so mocked
    /// `find_sessions_by_criteria` tests exercise real filtering semantics.
    fn session_matches_criteria(session: &StreamSession, criteria: &SessionQueryCriteria) -> bool {
        if let Some(states) = &criteria.states {
            let state_str = session.state().as_str();
            if !states.iter().any(|s| s.eq_ignore_ascii_case(state_str)) {
                return false;
            }
        }

        if criteria.exclude_expired && session.is_expired() {
            return false;
        }

        if let Some(after) = criteria.created_after
            && session.created_at() < after
        {
            return false;
        }
        if let Some(before) = criteria.created_before
            && session.created_at() > before
        {
            return false;
        }

        if let Some(has_active) = criteria.has_active_streams {
            let has_active_streams = session.streams().values().any(|stream| stream.is_active());
            if has_active != has_active_streams {
                return false;
            }
        }

        let stream_count = session.streams().len();
        if let Some(min) = criteria.min_stream_count
            && stream_count < min
        {
            return false;
        }
        if let Some(max) = criteria.max_stream_count
            && stream_count > max
        {
            return false;
        }

        if let Some(pattern) = &criteria.client_info_pattern {
            match session.client_info() {
                Some(info) => {
                    if !info.to_lowercase().contains(&pattern.to_lowercase()) {
                        return false;
                    }
                }
                None => return false,
            }
        }

        true
    }

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
            criteria: SessionQueryCriteria,
            pagination: Pagination,
        ) -> Self::FindSessionsByCriteriaFuture<'_> {
            async move {
                let mut sessions: Vec<_> = self
                    .sessions
                    .lock()
                    .values()
                    .filter(|session| session_matches_criteria(session, &criteria))
                    .cloned()
                    .collect();
                let total_count = sessions.len();

                if let Some(sort_field) = &pagination.sort_by {
                    sessions.sort_by(|a, b| {
                        let cmp = match sort_field.as_str() {
                            "created_at" => a.created_at().cmp(&b.created_at()),
                            "updated_at" => a.updated_at().cmp(&b.updated_at()),
                            "stream_count" => a.streams().len().cmp(&b.streams().len()),
                            "total_bytes" => a.stats().total_bytes.cmp(&b.stats().total_bytes),
                            _ => std::cmp::Ordering::Equal,
                        };
                        match pagination.sort_order {
                            RepoSortOrder::Ascending => cmp,
                            RepoSortOrder::Descending => cmp.reverse(),
                        }
                    });
                }

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
        let frame_store = Arc::new(crate::infrastructure::adapters::InMemoryFrameStore::new());
        let handler =
            StreamQueryHandler::new(session_repository.clone(), stream_store, frame_store);

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
        let frame_store = Arc::new(crate::infrastructure::adapters::InMemoryFrameStore::new());
        let handler = StreamQueryHandler::new(session_repository, stream_store, frame_store);

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
        let frame_store = Arc::new(crate::infrastructure::adapters::InMemoryFrameStore::new());
        let handler = StreamQueryHandler::new(session_repository, stream_store, frame_store);

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
        let frame_store = Arc::new(crate::infrastructure::adapters::InMemoryFrameStore::new());
        let handler = StreamQueryHandler::new(session_repository, stream_store, frame_store);

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
    async fn test_get_stream_frames_returns_persisted_frames() {
        use crate::domain::{
            entities::frame::FramePatch,
            ports::FrameStoreGat,
            value_objects::{JsonData, JsonPath, Priority},
        };
        use crate::infrastructure::adapters::InMemoryFrameStore;

        let session_repository = Arc::new(MockRepository::new());
        let mut session = StreamSession::new(SessionConfig::default());
        let _ = session.activate();
        let session_id = session.id();
        let stream_id = session
            .create_stream(JsonData::from(serde_json::json!({"k": "v"})))
            .unwrap();
        session_repository.add_session(session);

        let stream_store = Arc::new(MockStreamStore);
        let frame_store = Arc::new(InMemoryFrameStore::new());

        // Simulate the command handler having appended frames.
        let frames = (1..=3)
            .map(|seq| {
                let patch = FramePatch::set(
                    JsonPath::new(format!("$.field_{seq}")).unwrap(),
                    JsonData::Integer(seq as i64),
                );
                crate::domain::entities::Frame::patch(stream_id, seq, Priority::HIGH, vec![patch])
                    .unwrap()
            })
            .collect();
        frame_store.append_frames(stream_id, frames).await.unwrap();

        let handler = StreamQueryHandler::new(session_repository, stream_store, frame_store);

        let query = GetStreamFramesQuery {
            session_id: session_id.into(),
            stream_id: stream_id.into(),
            since_sequence: None,
            priority_filter: None,
            limit: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await.unwrap();
        assert_eq!(result.frames.len(), 3);
        assert_eq!(result.total_count, 3);
        assert_eq!(
            result
                .frames
                .iter()
                .map(crate::domain::entities::Frame::sequence)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[tokio::test]
    async fn test_get_stream_frames_applies_filters() {
        use crate::domain::{
            entities::frame::FramePatch,
            ports::FrameStoreGat,
            value_objects::{JsonData, JsonPath, Priority},
        };
        use crate::infrastructure::adapters::InMemoryFrameStore;

        let session_repository = Arc::new(MockRepository::new());
        let mut session = StreamSession::new(SessionConfig::default());
        let _ = session.activate();
        let session_id = session.id();
        let stream_id = session
            .create_stream(JsonData::from(serde_json::json!({"k": "v"})))
            .unwrap();
        session_repository.add_session(session);

        let stream_store = Arc::new(MockStreamStore);
        let frame_store = Arc::new(InMemoryFrameStore::new());

        // Three frames with mixed priorities and sequences.
        let frames: Vec<_> = [
            (1u64, Priority::LOW),
            (2, Priority::HIGH),
            (3, Priority::CRITICAL),
        ]
        .into_iter()
        .map(|(seq, prio)| {
            let patch = FramePatch::set(
                JsonPath::new(format!("$.field_{seq}")).unwrap(),
                JsonData::Integer(seq as i64),
            );
            crate::domain::entities::Frame::patch(stream_id, seq, prio, vec![patch]).unwrap()
        })
        .collect();
        frame_store.append_frames(stream_id, frames).await.unwrap();

        let handler = StreamQueryHandler::new(session_repository, stream_store, frame_store);

        // since_sequence=1 + priority>=HIGH should leave seq 2 and 3.
        let query = GetStreamFramesQuery {
            session_id: session_id.into(),
            stream_id: stream_id.into(),
            since_sequence: Some(1),
            priority_filter: Some(Priority::HIGH.into()),
            limit: Some(10),
        };

        let result = QueryHandlerGat::handle(&handler, query).await.unwrap();
        assert_eq!(result.frames.len(), 2);
        assert_eq!(result.total_count, 2);
        assert_eq!(
            result
                .frames
                .iter()
                .map(crate::domain::entities::Frame::sequence)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
    }

    #[tokio::test]
    async fn test_get_stream_frames_caps_limit_to_max() {
        use crate::domain::{
            entities::frame::FramePatch,
            ports::FrameStoreGat,
            value_objects::{JsonData, JsonPath, Priority},
        };
        use crate::infrastructure::adapters::InMemoryFrameStore;

        let session_repository = Arc::new(MockRepository::new());
        let mut session = StreamSession::new(SessionConfig::default());
        let _ = session.activate();
        let session_id = session.id();
        let stream_id = session
            .create_stream(JsonData::from(serde_json::json!({"k": "v"})))
            .unwrap();
        session_repository.add_session(session);

        let frame_store = Arc::new(InMemoryFrameStore::new());
        let frames: Vec<_> = (1..=5)
            .map(|seq| {
                let patch = FramePatch::set(
                    JsonPath::new(format!("$.field_{seq}")).unwrap(),
                    JsonData::Integer(seq as i64),
                );
                crate::domain::entities::Frame::patch(stream_id, seq, Priority::HIGH, vec![patch])
                    .unwrap()
            })
            .collect();
        frame_store.append_frames(stream_id, frames).await.unwrap();

        let handler =
            StreamQueryHandler::new(session_repository, Arc::new(MockStreamStore), frame_store);

        // limit=999_999 must be capped to MAX_FRAMES_PAGE_SIZE without erroring.
        let query = GetStreamFramesQuery {
            session_id: session_id.into(),
            stream_id: stream_id.into(),
            since_sequence: None,
            priority_filter: None,
            limit: Some(999_999),
        };

        let result = QueryHandlerGat::handle(&handler, query).await.unwrap();
        assert_eq!(result.frames.len(), 5);
        assert_eq!(result.total_count, 5);
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
        let frame_store = Arc::new(crate::infrastructure::adapters::InMemoryFrameStore::new());
        let handler = StreamQueryHandler::new(session_repository, stream_store, frame_store);

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

    #[tokio::test]
    async fn test_search_sessions_with_pagination_has_more_true() {
        let repository = Arc::new(MockRepository::new());
        for _ in 0..10 {
            repository.add_session(make_session(None));
        }
        let handler = SessionQueryHandler::new(repository);

        let query = SearchSessionsQuery {
            filters: SessionFilters::default(),
            sort_by: None,
            sort_order: None,
            limit: Some(4),
            offset: Some(3),
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 4);
        assert_eq!(response.total_count, 10);
        assert!(response.has_more);
    }

    #[tokio::test]
    async fn test_search_sessions_last_page_has_more_false() {
        let repository = Arc::new(MockRepository::new());
        for _ in 0..7 {
            repository.add_session(make_session(None));
        }
        let handler = SessionQueryHandler::new(repository);

        // offset=3, limit=4 → page of 4 exactly reaches total_count=7 → full last page
        let query = SearchSessionsQuery {
            filters: SessionFilters::default(),
            sort_by: None,
            sort_order: None,
            limit: Some(4),
            offset: Some(3),
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 4);
        assert_eq!(response.total_count, 7);
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_search_sessions_filter_and_pagination_combined() {
        let repository = Arc::new(MockRepository::new());
        // 8 sessions matching the "active" state filter.
        for _ in 0..8 {
            repository.add_session(make_session(None));
        }
        // 3 sessions in a non-matching state (never activated).
        for _ in 0..3 {
            repository.add_session(StreamSession::new(SessionConfig::default()));
        }
        let handler = SessionQueryHandler::new(repository);

        let query = SearchSessionsQuery {
            filters: SessionFilters {
                state: Some(SessionState::Active),
                ..Default::default()
            },
            sort_by: None,
            sort_order: None,
            limit: Some(3),
            offset: Some(2),
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 3);
        assert_eq!(response.total_count, 8);
        assert!(response.has_more);
    }

    #[tokio::test]
    async fn test_search_sessions_max_page_size_clamped() {
        let repository = Arc::new(MockRepository::new());
        for _ in 0..150 {
            repository.add_session(make_session(None));
        }
        let handler = SessionQueryHandler::new(repository);

        // Requested limit exceeds MAX_PAGE_SIZE (100); the handler must clamp it.
        let query = SearchSessionsQuery {
            filters: SessionFilters::default(),
            sort_by: None,
            sort_order: None,
            limit: Some(500),
            offset: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 100);
        assert_eq!(response.total_count, 150);
        assert!(response.has_more);
    }

    #[tokio::test]
    async fn test_search_sessions_sort_by_stream_count_descending() {
        use crate::domain::value_objects::JsonData;

        let repository = Arc::new(MockRepository::new());

        let mut few = make_session(None);
        few.create_stream(JsonData::from(serde_json::json!({"k": "v"})))
            .unwrap();
        let few_id = few.id();

        let mut many = make_session(None);
        for _ in 0..3 {
            many.create_stream(JsonData::from(serde_json::json!({"k": "v"})))
                .unwrap();
        }
        let many_id = many.id();

        let none = make_session(None);
        let none_id = none.id();

        repository.add_session(few);
        repository.add_session(many);
        repository.add_session(none);
        let handler = SessionQueryHandler::new(repository);

        let query = SearchSessionsQuery {
            filters: SessionFilters::default(),
            sort_by: Some(SessionSortField::StreamCount),
            sort_order: Some(SortOrder::Descending),
            limit: None,
            offset: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let ids: Vec<_> = response.sessions.iter().map(|s| s.id()).collect();
        assert_eq!(ids, vec![many_id, few_id, none_id]);
    }

    #[tokio::test]
    async fn test_search_sessions_sort_by_total_bytes_descending() {
        use crate::domain::value_objects::{JsonData, Priority};

        let repository = Arc::new(MockRepository::new());

        // Session with a real byte-carrying patch frame batch.
        let mut heavy = make_session(None);
        let heavy_stream = heavy
            .create_stream(JsonData::String(
                "hello world payload, quite a few bytes here".to_owned(),
            ))
            .unwrap();
        heavy.start_stream(heavy_stream).unwrap();
        heavy
            .create_stream_patch_frames(heavy_stream, Priority::LOW, 100)
            .unwrap();
        let heavy_id = heavy.id();

        // Session with no streams, so total_bytes stays zero.
        let empty = make_session(None);
        let empty_id = empty.id();

        assert!(heavy.stats().total_bytes > empty.stats().total_bytes);

        repository.add_session(heavy);
        repository.add_session(empty);
        let handler = SessionQueryHandler::new(repository);

        let query = SearchSessionsQuery {
            filters: SessionFilters::default(),
            sort_by: Some(SessionSortField::TotalBytes),
            sort_order: Some(SortOrder::Descending),
            limit: None,
            offset: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let ids: Vec<_> = response.sessions.iter().map(|s| s.id()).collect();
        assert_eq!(ids, vec![heavy_id, empty_id]);
    }

    /// Deterministic clock letting tests control `created_at`/`updated_at`
    /// ordering without relying on wall-clock sleeps. Each call to `now()`
    /// advances the clock so successive sessions/mutations get distinct,
    /// strictly increasing timestamps.
    struct FixedTimeProvider {
        counter: std::sync::atomic::AtomicI64,
    }

    impl FixedTimeProvider {
        fn new() -> Self {
            Self {
                counter: std::sync::atomic::AtomicI64::new(0),
            }
        }
    }

    impl TimeProvider for FixedTimeProvider {
        fn now(&self) -> chrono::DateTime<Utc> {
            let offset = self
                .counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Utc::now() + chrono::Duration::seconds(offset)
        }
    }

    #[tokio::test]
    async fn test_search_sessions_sort_by_created_at_descending() {
        let clock = std::sync::Arc::new(FixedTimeProvider::new());
        let repository = Arc::new(MockRepository::new());

        let mut first = StreamSession::with_time_provider(SessionConfig::default(), clock.clone());
        first.activate().unwrap();
        let first_id = first.id();

        let mut second = StreamSession::with_time_provider(SessionConfig::default(), clock.clone());
        second.activate().unwrap();
        let second_id = second.id();

        repository.add_session(first);
        repository.add_session(second);
        let handler = SessionQueryHandler::new(repository);

        let query = SearchSessionsQuery {
            filters: SessionFilters::default(),
            sort_by: Some(SessionSortField::CreatedAt),
            sort_order: Some(SortOrder::Descending),
            limit: None,
            offset: None,
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        let ids: Vec<_> = response.sessions.iter().map(|s| s.id()).collect();
        assert_eq!(ids, vec![second_id, first_id]);
    }

    #[tokio::test]
    async fn test_search_sessions_created_after_before_filter() {
        let clock = std::sync::Arc::new(FixedTimeProvider::new());
        let repository = Arc::new(MockRepository::new());

        // Three sessions created at strictly increasing timestamps (t=0,1,2).
        let mut early = StreamSession::with_time_provider(SessionConfig::default(), clock.clone());
        early.activate().unwrap();

        let mut middle = StreamSession::with_time_provider(SessionConfig::default(), clock.clone());
        middle.activate().unwrap();
        let middle_id = middle.id();
        let middle_created_at = middle.created_at();

        let mut late = StreamSession::with_time_provider(SessionConfig::default(), clock.clone());
        late.activate().unwrap();

        repository.add_session(early);
        repository.add_session(middle);
        repository.add_session(late);
        let handler = SessionQueryHandler::new(repository);

        let query = SearchSessionsQuery {
            filters: SessionFilters {
                created_after: Some(middle_created_at - chrono::Duration::milliseconds(1)),
                created_before: Some(middle_created_at + chrono::Duration::milliseconds(1)),
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
        assert_eq!(response.sessions[0].id(), middle_id);
    }

    #[tokio::test]
    async fn test_search_sessions_has_active_streams_filter() {
        use crate::domain::value_objects::JsonData;

        let repository = Arc::new(MockRepository::new());

        let mut with_active = make_session(None);
        let stream_id = with_active
            .create_stream(JsonData::from(serde_json::json!({"k": "v"})))
            .unwrap();
        with_active.start_stream(stream_id).unwrap();
        let with_active_id = with_active.id();

        let without_active = make_session(None);
        let without_active_id = without_active.id();

        repository.add_session(with_active);
        repository.add_session(without_active);
        let handler = SessionQueryHandler::new(repository);

        let query_active_only = SearchSessionsQuery {
            filters: SessionFilters {
                has_active_streams: Some(true),
                ..Default::default()
            },
            sort_by: None,
            sort_order: None,
            limit: None,
            offset: None,
        };
        let result = QueryHandlerGat::handle(&handler, query_active_only).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 1);
        assert_eq!(response.sessions[0].id(), with_active_id);

        let query_inactive_only = SearchSessionsQuery {
            filters: SessionFilters {
                has_active_streams: Some(false),
                ..Default::default()
            },
            sort_by: None,
            sort_order: None,
            limit: None,
            offset: None,
        };
        let result = QueryHandlerGat::handle(&handler, query_inactive_only).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.sessions.len(), 1);
        assert_eq!(response.sessions[0].id(), without_active_id);
    }

    /// Locks in the exact-match state filter semantics that replaced the old
    /// in-process substring match (see `matches_filters`, removed in #391).
    /// `SessionFilters::state` is now typed as `SessionState` (#414), so a
    /// partial word like "complet" is rejected at compile time rather than
    /// silently substring-matching a "Completed" session; only a fellow
    /// non-matching variant can be exercised as the negative case here.
    /// The HTTP-boundary rejection of an actual malformed/partial `state`
    /// string (e.g. `?state=complet`) is covered separately by
    /// `axum_adapter::tests::search_sessions_route_rejects_unknown_state`.
    #[tokio::test]
    async fn test_search_sessions_state_filter_matches_only_specified_variant() {
        let repository = Arc::new(MockRepository::new());
        let mut session = make_session(None);
        session.close().unwrap(); // Active -> Completed
        repository.add_session(session);
        let handler = SessionQueryHandler::new(repository);

        // Non-matching variant: must not match the Completed session.
        let non_matching_query = SearchSessionsQuery {
            filters: SessionFilters {
                state: Some(SessionState::Failed),
                ..Default::default()
            },
            sort_by: None,
            sort_order: None,
            limit: None,
            offset: None,
        };
        let result = QueryHandlerGat::handle(&handler, non_matching_query).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().sessions.len(), 0);

        // Matching variant: matches exactly.
        let exact_query = SearchSessionsQuery {
            filters: SessionFilters {
                state: Some(SessionState::Completed),
                ..Default::default()
            },
            sort_by: None,
            sort_order: None,
            limit: None,
            offset: None,
        };
        let result = QueryHandlerGat::handle(&handler, exact_query).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().sessions.len(), 1);
    }

    /// Regression test for the search endpoint's default scope: with no
    /// explicit `filters.state`, results must stay limited to active,
    /// non-expired sessions, matching the legacy `find_active_sessions()`
    /// contract (`state == Active && !is_expired()`). Uses the real
    /// `GatInMemoryStreamRepository` (not `MockRepository`) so the criteria
    /// actually flow through `matches_criteria`, the same path `GET
    /// /pjs/sessions/search` exercises in production.
    #[tokio::test]
    async fn test_search_sessions_default_scope_excludes_non_active_and_expired() {
        use crate::infrastructure::GatInMemoryStreamRepository;

        let repository = Arc::new(GatInMemoryStreamRepository::new());

        let mut active = StreamSession::new(SessionConfig::default());
        active.activate().unwrap();
        let active_id = active.id();
        repository.save_session(active).await.unwrap();

        let mut completed = StreamSession::new(SessionConfig::default());
        completed.activate().unwrap();
        completed.close().unwrap();
        repository.save_session(completed).await.unwrap();

        let mut expired = StreamSession::new(SessionConfig {
            session_timeout_seconds: 0,
            ..SessionConfig::default()
        });
        expired.activate().unwrap();
        // Give the real clock a moment to move past `expires_at`.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        repository.save_session(expired).await.unwrap();

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
        assert_eq!(response.sessions.len(), 1);
        assert_eq!(response.sessions[0].id(), active_id);
    }

    /// Regression test: `limit=0` and an offset beyond `MAX_PAGINATION_OFFSET`
    /// must not surface `Pagination::validate()`'s `DomainError::InvalidInput`
    /// (which the HTTP layer maps to a 500). The handler must clamp both
    /// before they reach the repository. Uses the real
    /// `GatInMemoryStreamRepository` so `Pagination::validate()` actually runs.
    #[tokio::test]
    async fn test_search_sessions_limit_zero_and_excessive_offset_do_not_error() {
        use crate::infrastructure::GatInMemoryStreamRepository;

        let repository = Arc::new(GatInMemoryStreamRepository::new());
        let mut session = StreamSession::new(SessionConfig::default());
        session.activate().unwrap();
        repository.save_session(session).await.unwrap();
        let handler = SessionQueryHandler::new(repository);

        let query = SearchSessionsQuery {
            filters: SessionFilters::default(),
            sort_by: None,
            sort_order: None,
            limit: Some(0),
            offset: Some(usize::MAX),
        };

        let result = QueryHandlerGat::handle(&handler, query).await;
        assert!(
            result.is_ok(),
            "expected clamped pagination, got {result:?}"
        );
        let response = result.unwrap();
        assert_eq!(response.total_count, 1);
        assert!(response.sessions.is_empty());
    }
}
