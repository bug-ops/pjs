//! Shared test-only mocks for domain ports.
//!
//! Consolidates the `StreamRepositoryGat` mock previously duplicated across
//! the unit test modules of `application::handlers::command_handlers`,
//! `application::handlers::query_handlers`, and `infrastructure::http::axum_adapter`
//! (#478). Not used by the `tests/` integration-test target, which is a
//! separate compilation unit with its own mocks in `tests/common/mod.rs`.

use crate::domain::{
    DomainError, DomainResult,
    aggregates::StreamSession,
    entities::{Frame, stream::StreamConfig},
    events::DomainEvent,
    ports::{
        Pagination, SessionHealthSnapshot, SessionQueryCriteria, SessionQueryResult,
        SessionSortField, SortOrder, StreamRepositoryGat,
    },
    value_objects::{JsonData, Priority, SessionId, StreamId},
};
use std::collections::HashMap;
use std::future::Future;

/// Mirrors `GatInMemoryStreamRepository::matches_criteria` so mocked
/// `find_sessions_by_criteria` calls exercise real filtering semantics.
pub(crate) fn session_matches_criteria(
    session: &StreamSession,
    criteria: &SessionQueryCriteria,
) -> bool {
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

/// In-memory `StreamRepositoryGat` mock shared by unit tests across the crate.
///
/// Locks the whole session map per call rather than per-key like the
/// production `GatInMemoryStreamRepository`, since unit tests exercising
/// this mock don't need per-key concurrency.
pub(crate) struct MockRepository {
    sessions: parking_lot::Mutex<HashMap<SessionId, StreamSession>>,
}

impl MockRepository {
    pub(crate) fn new() -> Self {
        Self {
            sessions: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn add_session(&self, session: StreamSession) {
        self.sessions.lock().insert(session.id(), session);
    }
}

impl StreamRepositoryGat for MockRepository {
    type FindSessionFuture<'a>
        = impl Future<Output = DomainResult<Option<StreamSession>>> + Send + 'a
    where
        Self: 'a;

    type SaveSessionFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type CreateStreamAtomicFuture<'a>
        = impl Future<Output = DomainResult<(StreamId, Vec<DomainEvent>)>> + Send + 'a
    where
        Self: 'a;

    type StartStreamAtomicFuture<'a>
        = impl Future<Output = DomainResult<Vec<DomainEvent>>> + Send + 'a
    where
        Self: 'a;

    type CompleteStreamAtomicFuture<'a>
        = impl Future<Output = DomainResult<Vec<DomainEvent>>> + Send + 'a
    where
        Self: 'a;

    type CreateStreamPatchFramesAtomicFuture<'a>
        = impl Future<Output = DomainResult<(Vec<Frame>, Vec<DomainEvent>)>> + Send + 'a
    where
        Self: 'a;

    type BatchGenerateFramesAtomicFuture<'a>
        = impl Future<Output = DomainResult<(Vec<Frame>, Vec<DomainEvent>)>> + Send + 'a
    where
        Self: 'a;

    type CloseSessionAtomicFuture<'a>
        = impl Future<Output = DomainResult<Vec<DomainEvent>>> + Send + 'a
    where
        Self: 'a;

    type RemoveSessionFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type FindActiveSessionsFuture<'a>
        = impl Future<Output = DomainResult<Vec<StreamSession>>> + Send + 'a
    where
        Self: 'a;

    type FindSessionsByCriteriaFuture<'a>
        = impl Future<Output = DomainResult<SessionQueryResult>> + Send + 'a
    where
        Self: 'a;

    type GetSessionHealthFuture<'a>
        = impl Future<Output = DomainResult<SessionHealthSnapshot>> + Send + 'a
    where
        Self: 'a;

    type SessionExistsFuture<'a>
        = impl Future<Output = DomainResult<bool>> + Send + 'a
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

    fn create_stream_atomic(
        &self,
        session_id: SessionId,
        source_data: JsonData,
        config: Option<StreamConfig>,
    ) -> Self::CreateStreamAtomicFuture<'_> {
        async move {
            let mut sessions = self.sessions.lock();
            let session = sessions.get_mut(&session_id).ok_or_else(|| {
                DomainError::SessionNotFound(format!("Session {session_id} not found"))
            })?;
            let stream_id = session.create_stream_with_config(source_data, config)?;
            Ok((stream_id, session.take_events().into_iter().collect()))
        }
    }

    fn start_stream_atomic(
        &self,
        session_id: SessionId,
        stream_id: StreamId,
    ) -> Self::StartStreamAtomicFuture<'_> {
        async move {
            let mut sessions = self.sessions.lock();
            let session = sessions.get_mut(&session_id).ok_or_else(|| {
                DomainError::SessionNotFound(format!("Session {session_id} not found"))
            })?;
            session.start_stream(stream_id)?;
            Ok(session.take_events().into_iter().collect())
        }
    }

    fn complete_stream_atomic(
        &self,
        session_id: SessionId,
        stream_id: StreamId,
    ) -> Self::CompleteStreamAtomicFuture<'_> {
        async move {
            let mut sessions = self.sessions.lock();
            let session = sessions.get_mut(&session_id).ok_or_else(|| {
                DomainError::SessionNotFound(format!("Session {session_id} not found"))
            })?;
            session.complete_stream(stream_id)?;
            Ok(session.take_events().into_iter().collect())
        }
    }

    fn create_stream_patch_frames_atomic(
        &self,
        session_id: SessionId,
        stream_id: StreamId,
        priority_threshold: Priority,
        max_frames: usize,
    ) -> Self::CreateStreamPatchFramesAtomicFuture<'_> {
        async move {
            let mut sessions = self.sessions.lock();
            let session = sessions.get_mut(&session_id).ok_or_else(|| {
                DomainError::SessionNotFound(format!("Session {session_id} not found"))
            })?;
            let frames =
                session.create_stream_patch_frames(stream_id, priority_threshold, max_frames)?;
            Ok((frames, session.take_events().into_iter().collect()))
        }
    }

    fn batch_generate_frames_atomic(
        &self,
        session_id: SessionId,
        max_frames: usize,
    ) -> Self::BatchGenerateFramesAtomicFuture<'_> {
        async move {
            let mut sessions = self.sessions.lock();
            let session = sessions.get_mut(&session_id).ok_or_else(|| {
                DomainError::SessionNotFound(format!("Session {session_id} not found"))
            })?;
            let extracted =
                session.extract_prioritized_patches_for_active_streams(Priority::BACKGROUND);
            let frames = session.commit_priority_frames(extracted, max_frames)?;
            Ok((frames, session.take_events().into_iter().collect()))
        }
    }

    fn close_session_atomic(&self, session_id: SessionId) -> Self::CloseSessionAtomicFuture<'_> {
        async move {
            let mut sessions = self.sessions.lock();
            let session = sessions.get_mut(&session_id).ok_or_else(|| {
                DomainError::SessionNotFound(format!("Session {session_id} not found"))
            })?;
            session.close()?;
            Ok(session.take_events().into_iter().collect())
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

            if let Some(sort_field) = pagination.sort_by {
                sessions.sort_by(|a, b| {
                    let cmp = match sort_field {
                        SessionSortField::CreatedAt => a.created_at().cmp(&b.created_at()),
                        SessionSortField::UpdatedAt => a.updated_at().cmp(&b.updated_at()),
                        SessionSortField::StreamCount => a.streams().len().cmp(&b.streams().len()),
                        SessionSortField::TotalBytes => {
                            a.stats().total_bytes.cmp(&b.stats().total_bytes)
                        }
                    };
                    match pagination.sort_order {
                        SortOrder::Ascending => cmp,
                        SortOrder::Descending => cmp.reverse(),
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
