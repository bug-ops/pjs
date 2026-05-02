//! Command handlers implementing business use cases

use crate::{
    application::{
        ApplicationError, ApplicationResult, commands::*, dto::JsonDataDto,
        handlers::CommandHandlerGat,
    },
    domain::{
        aggregates::StreamSession,
        entities::Frame,
        ports::{
            DictionaryStore, EventPublisherGat, FrameStoreGat, NoopDictionaryStore,
            StreamRepositoryGat,
        },
        value_objects::{JsonData, SessionId, StreamId},
    },
    infrastructure::adapters::InMemoryFrameStore,
};
use std::sync::Arc;

/// Handler for session management commands.
///
/// Holds an optional [`DictionaryStore`] (defaulting to [`NoopDictionaryStore`])
/// so that frame-generating commands can feed accepted frame payloads into the
/// per-session training corpus. Without this wiring the
/// `GET /pjs/sessions/{id}/dictionary` endpoint would be unreachable end-to-end.
///
/// Also holds a [`FrameStoreGat`] (defaulting to [`InMemoryFrameStore`]) so
/// frames produced by `GenerateFramesCommand` / `BatchGenerateFramesCommand`
/// remain queryable through `GET /pjs/sessions/{id}/streams/{id}/frames`.
pub struct SessionCommandHandler<R, P, F = InMemoryFrameStore>
where
    R: StreamRepositoryGat + 'static,
    P: EventPublisherGat + 'static,
    F: FrameStoreGat + 'static,
{
    repository: Arc<R>,
    event_publisher: Arc<P>,
    dictionary_store: Arc<dyn DictionaryStore>,
    frame_store: Arc<F>,
}

impl<R, P, F> std::fmt::Debug for SessionCommandHandler<R, P, F>
where
    R: StreamRepositoryGat + 'static,
    P: EventPublisherGat + 'static,
    F: FrameStoreGat + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionCommandHandler")
            .finish_non_exhaustive()
    }
}

impl<R, P> SessionCommandHandler<R, P, InMemoryFrameStore>
where
    R: StreamRepositoryGat + 'static,
    P: EventPublisherGat + 'static,
{
    /// Create a handler with the no-op [`DictionaryStore`] and a fresh
    /// [`InMemoryFrameStore`].
    ///
    /// The dictionary endpoint will return `404 Not Found` until the handler is
    /// constructed with [`SessionCommandHandler::with_stores`] and a concrete
    /// [`DictionaryStore`] such as
    /// [`crate::infrastructure::repositories::InMemoryDictionaryStore`].
    pub fn new(repository: Arc<R>, event_publisher: Arc<P>) -> Self {
        Self::with_dictionary_store(repository, event_publisher, Arc::new(NoopDictionaryStore))
    }

    /// Create a handler with a custom [`DictionaryStore`] and a fresh
    /// [`InMemoryFrameStore`].
    pub fn with_dictionary_store(
        repository: Arc<R>,
        event_publisher: Arc<P>,
        dictionary_store: Arc<dyn DictionaryStore>,
    ) -> Self {
        Self::with_stores(
            repository,
            event_publisher,
            dictionary_store,
            Arc::new(InMemoryFrameStore::new()),
        )
    }
}

impl<R, P, F> SessionCommandHandler<R, P, F>
where
    R: StreamRepositoryGat + 'static,
    P: EventPublisherGat + 'static,
    F: FrameStoreGat + 'static,
{
    /// Create a handler that feeds accepted frames into both the dictionary
    /// training corpus and the [`FrameStoreGat`] used by the frames query
    /// endpoint.
    pub fn with_stores(
        repository: Arc<R>,
        event_publisher: Arc<P>,
        dictionary_store: Arc<dyn DictionaryStore>,
        frame_store: Arc<F>,
    ) -> Self {
        Self {
            repository,
            event_publisher,
            dictionary_store,
            frame_store,
        }
    }

    /// Feed each accepted frame's serialized payload into the per-session
    /// training corpus.
    ///
    /// Errors are intentionally swallowed: training is best-effort and a
    /// transient failure must not poison the frame-generation response. The
    /// `OnceCell` inside `InMemoryDictionaryStore` is not poisoned on error
    /// either, so the next sample will retry.
    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    async fn train_from_frames(&self, session_id: SessionId, frames: &[Frame]) {
        for frame in frames {
            if let Ok(bytes) = serde_json::to_vec(frame.payload()) {
                let _ = self
                    .dictionary_store
                    .train_if_ready(session_id, bytes)
                    .await;
            }
        }
    }

    /// Persist generated frames into the [`FrameStoreGat`], grouping by
    /// `stream_id` so a single batch may span multiple streams.
    async fn persist_frames_grouped_by_stream(&self, frames: &[Frame]) -> ApplicationResult<()>
    where
        F: FrameStoreGat + Send + Sync,
    {
        if frames.is_empty() {
            return Ok(());
        }
        let mut buckets: std::collections::HashMap<StreamId, Vec<Frame>> =
            std::collections::HashMap::new();
        for frame in frames {
            buckets
                .entry(frame.stream_id())
                .or_default()
                .push(frame.clone());
        }
        for (stream_id, group) in buckets {
            self.frame_store
                .append_frames(stream_id, group)
                .await
                .map_err(ApplicationError::Domain)?;
        }
        Ok(())
    }
}

impl<R, P, F> CommandHandlerGat<CreateSessionCommand> for SessionCommandHandler<R, P, F>
where
    R: StreamRepositoryGat + Send + Sync,
    P: EventPublisherGat + Send + Sync,
    F: FrameStoreGat + Send + Sync,
{
    type Response = SessionId;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, command: CreateSessionCommand) -> Self::HandleFuture<'_> {
        async move {
            // Create new session
            let mut session = StreamSession::new(command.config);

            // Set client information
            if let (Some(client_info), user_agent, ip_address) =
                (command.client_info, command.user_agent, command.ip_address)
            {
                session.set_client_info(client_info, user_agent, ip_address);
            }

            // Activate session
            session.activate().map_err(ApplicationError::Domain)?;

            let session_id = session.id();

            // Save to repository
            self.repository
                .save_session(session.clone())
                .await
                .map_err(ApplicationError::Domain)?;

            // Publish events in batch for better performance
            let events: Vec<_> = session.take_events().into_iter().collect();
            self.event_publisher
                .publish_batch(events)
                .await
                .map_err(ApplicationError::Domain)?;

            Ok(session_id)
        }
    }
}

impl<R, P, F> CommandHandlerGat<CreateStreamCommand> for SessionCommandHandler<R, P, F>
where
    R: StreamRepositoryGat + Send + Sync,
    P: EventPublisherGat + Send + Sync,
    F: FrameStoreGat + Send + Sync,
{
    type Response = StreamId;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, command: CreateStreamCommand) -> Self::HandleFuture<'_> {
        async move {
            // Load session
            let mut session = self
                .repository
                .find_session(command.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", command.session_id))
                })?;

            // Convert at application boundary (DTO -> Domain)
            let domain_data: JsonData = JsonDataDto::from(command.source_data).into();
            let stream_id = session
                .create_stream(domain_data)
                .map_err(ApplicationError::Domain)?;

            // Update stream configuration if provided
            if let Some(config) = command.config {
                session
                    .update_stream_config(stream_id, config)
                    .map_err(ApplicationError::Domain)?;
            }

            // Save updated session
            self.repository
                .save_session(session.clone())
                .await
                .map_err(ApplicationError::Domain)?;

            // Publish events in batch for better performance
            let events: Vec<_> = session.take_events().into_iter().collect();
            self.event_publisher
                .publish_batch(events)
                .await
                .map_err(ApplicationError::Domain)?;

            Ok(stream_id)
        }
    }
}

impl<R, P, F> CommandHandlerGat<StartStreamCommand> for SessionCommandHandler<R, P, F>
where
    R: StreamRepositoryGat + Send + Sync,
    P: EventPublisherGat + Send + Sync,
    F: FrameStoreGat + Send + Sync,
{
    type Response = ();

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, command: StartStreamCommand) -> Self::HandleFuture<'_> {
        async move {
            // Load session
            let mut session = self
                .repository
                .find_session(command.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", command.session_id))
                })?;

            // Start stream
            session
                .start_stream(command.stream_id.into())
                .map_err(ApplicationError::Domain)?;

            // Save updated session
            self.repository
                .save_session(session.clone())
                .await
                .map_err(ApplicationError::Domain)?;

            // Publish events in batch for better performance
            let events: Vec<_> = session.take_events().into_iter().collect();
            self.event_publisher
                .publish_batch(events)
                .await
                .map_err(ApplicationError::Domain)?;

            Ok(())
        }
    }
}

impl<R, P, F> CommandHandlerGat<CompleteStreamCommand> for SessionCommandHandler<R, P, F>
where
    R: StreamRepositoryGat + Send + Sync,
    P: EventPublisherGat + Send + Sync,
    F: FrameStoreGat + Send + Sync,
{
    type Response = ();

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, command: CompleteStreamCommand) -> Self::HandleFuture<'_> {
        async move {
            // Load session
            let mut session = self
                .repository
                .find_session(command.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", command.session_id))
                })?;

            // Complete stream
            session
                .complete_stream(command.stream_id.into())
                .map_err(ApplicationError::Domain)?;

            // Save updated session
            self.repository
                .save_session(session.clone())
                .await
                .map_err(ApplicationError::Domain)?;

            // Publish events in batch for better performance
            let events: Vec<_> = session.take_events().into_iter().collect();
            self.event_publisher
                .publish_batch(events)
                .await
                .map_err(ApplicationError::Domain)?;

            Ok(())
        }
    }
}

impl<R, P, F> CommandHandlerGat<GenerateFramesCommand> for SessionCommandHandler<R, P, F>
where
    R: StreamRepositoryGat + Send + Sync,
    P: EventPublisherGat + Send + Sync,
    F: FrameStoreGat + Send + Sync,
{
    type Response = Vec<Frame>;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, command: GenerateFramesCommand) -> Self::HandleFuture<'_> {
        async move {
            // Load session
            let mut session = self
                .repository
                .find_session(command.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", command.session_id))
                })?;

            // Generate frames through the aggregate root so session-level
            // stats and events stay consistent with the child stream mutation.
            let priority = command
                .priority_threshold
                .try_into()
                .map_err(ApplicationError::Domain)?;
            let frames = session
                .create_stream_patch_frames(command.stream_id.into(), priority, command.max_frames)
                .map_err(|e| match e {
                    crate::domain::DomainError::StreamNotFound(_) => ApplicationError::NotFound(
                        format!("Stream {} not found", command.stream_id),
                    ),
                    other => ApplicationError::Domain(other),
                })?;

            // WebSocket frame production runs through a disjoint session model
            // (`infrastructure/websocket`) and does not increment this counter, so
            // `pjs_frames_total` reflects HTTP throughput only — see #239.
            #[cfg(feature = "metrics")]
            metrics::counter!("pjs_frames_total").increment(frames.len() as u64);

            // Feed accepted frame payloads into the per-session training corpus
            // so the dictionary endpoint becomes reachable end-to-end.
            #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
            self.train_from_frames(command.session_id.into(), &frames)
                .await;

            // Persist generated frames so GET /streams/{id}/frames can return them.
            self.frame_store
                .append_frames(command.stream_id.into(), frames.clone())
                .await
                .map_err(ApplicationError::Domain)?;

            // Save updated session
            self.repository
                .save_session(session.clone())
                .await
                .map_err(ApplicationError::Domain)?;

            // Publish events in batch for better performance
            let events: Vec<_> = session.take_events().into_iter().collect();
            self.event_publisher
                .publish_batch(events)
                .await
                .map_err(ApplicationError::Domain)?;

            Ok(frames)
        }
    }
}

impl<R, P, F> CommandHandlerGat<BatchGenerateFramesCommand> for SessionCommandHandler<R, P, F>
where
    R: StreamRepositoryGat + Send + Sync,
    P: EventPublisherGat + Send + Sync,
    F: FrameStoreGat + Send + Sync,
{
    type Response = Vec<Frame>;

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, command: BatchGenerateFramesCommand) -> Self::HandleFuture<'_> {
        async move {
            // Load session
            let mut session = self
                .repository
                .find_session(command.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", command.session_id))
                })?;

            // Generate priority frames across all streams
            let frames = session
                .create_priority_frames(command.max_frames)
                .map_err(ApplicationError::Domain)?;

            // WebSocket frame production runs through a disjoint session model
            // (`infrastructure/websocket`) and does not increment this counter, so
            // `pjs_frames_total` reflects HTTP throughput only — see #239.
            #[cfg(feature = "metrics")]
            metrics::counter!("pjs_frames_total").increment(frames.len() as u64);

            // Feed accepted frame payloads into the per-session training corpus
            // so the dictionary endpoint becomes reachable end-to-end.
            #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
            self.train_from_frames(command.session_id.into(), &frames)
                .await;

            // Persist generated frames so GET /streams/{id}/frames can return them.
            // Frames may span multiple streams in a batch; group by stream_id.
            self.persist_frames_grouped_by_stream(&frames).await?;

            // Save updated session
            self.repository
                .save_session(session.clone())
                .await
                .map_err(ApplicationError::Domain)?;

            // Publish events in batch for better performance
            let events: Vec<_> = session.take_events().into_iter().collect();
            self.event_publisher
                .publish_batch(events)
                .await
                .map_err(ApplicationError::Domain)?;

            Ok(frames)
        }
    }
}

impl<R, P, F> CommandHandlerGat<CloseSessionCommand> for SessionCommandHandler<R, P, F>
where
    R: StreamRepositoryGat + Send + Sync,
    P: EventPublisherGat + Send + Sync,
    F: FrameStoreGat + Send + Sync,
{
    type Response = ();

    type HandleFuture<'a>
        = impl std::future::Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn handle(&self, command: CloseSessionCommand) -> Self::HandleFuture<'_> {
        async move {
            // Load session
            let mut session = self
                .repository
                .find_session(command.session_id.into())
                .await
                .map_err(ApplicationError::Domain)?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!("Session {} not found", command.session_id))
                })?;

            // Close session
            session.close().map_err(ApplicationError::Domain)?;

            // Save updated session
            self.repository
                .save_session(session.clone())
                .await
                .map_err(ApplicationError::Domain)?;

            // Publish events in batch for better performance
            let events: Vec<_> = session.take_events().into_iter().collect();
            self.event_publisher
                .publish_batch(events)
                .await
                .map_err(ApplicationError::Domain)?;

            Ok(())
        }
    }
}

/// Validation helper for commands
pub struct CommandValidator;

impl CommandValidator {
    /// Validate CreateSessionCommand
    pub fn validate_create_session(command: &CreateSessionCommand) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if command.config.max_concurrent_streams == 0 {
            errors.push("max_concurrent_streams must be greater than 0".to_string());
        }

        if command.config.session_timeout_seconds == 0 {
            errors.push("session_timeout_seconds must be greater than 0".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate CreateStreamCommand
    pub fn validate_create_stream(command: &CreateStreamCommand) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if command.source_data.is_null() {
            errors.push("source_data cannot be null".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate GenerateFramesCommand
    pub fn validate_generate_frames(command: &GenerateFramesCommand) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if command.max_frames == 0 {
            errors.push("max_frames must be greater than 0".to_string());
        }

        if command.max_frames > 1000 {
            errors.push("max_frames cannot exceed 1000".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        aggregates::stream_session::SessionConfig,
        events::DomainEvent,
        ports::{
            EventPublisherGat, Pagination, SessionHealthSnapshot, SessionQueryCriteria,
            SessionQueryResult, StreamRepositoryGat,
        },
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

    struct MockEventPublisher;

    impl EventPublisherGat for MockEventPublisher {
        type PublishFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type PublishBatchFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        fn publish(&self, _event: DomainEvent) -> Self::PublishFuture<'_> {
            async move { Ok(()) }
        }

        fn publish_batch(&self, _events: Vec<DomainEvent>) -> Self::PublishBatchFuture<'_> {
            async move { Ok(()) }
        }
    }

    #[tokio::test]
    async fn test_create_session_command() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher);

        let command = CreateSessionCommand {
            config: SessionConfig::default(),
            client_info: Some("test-client".to_string()),
            user_agent: None,
            ip_address: None,
        };

        let result = handler.handle(command).await;
        assert!(result.is_ok());

        let session_id = result.unwrap();

        // Verify session was saved
        let saved_session = repository.find_session(session_id).await.unwrap();
        assert!(saved_session.is_some());
    }

    #[tokio::test]
    async fn test_session_command_handler_creation() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher.clone());

        assert!(std::ptr::eq(
            handler.repository.as_ref(),
            repository.as_ref()
        ));
        assert!(std::ptr::eq(
            handler.event_publisher.as_ref(),
            event_publisher.as_ref()
        ));
    }

    #[tokio::test]
    async fn test_create_session_with_full_client_info() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher);

        let command = CreateSessionCommand {
            config: SessionConfig::default(),
            client_info: Some("test-client".to_string()),
            user_agent: Some("Mozilla/5.0".to_string()),
            ip_address: Some("192.168.1.1".to_string()),
        };

        let result = handler.handle(command).await;
        assert!(result.is_ok());

        let session_id = result.unwrap();
        let saved_session = repository.find_session(session_id).await.unwrap();
        assert!(saved_session.is_some());
    }

    #[tokio::test]
    async fn test_create_session_without_client_info() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository, event_publisher);

        let command = CreateSessionCommand {
            config: SessionConfig::default(),
            client_info: None,
            user_agent: None,
            ip_address: None,
        };

        let result = handler.handle(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_stream_command() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher);

        // First create a session
        let create_session_cmd = CreateSessionCommand {
            config: SessionConfig::default(),
            client_info: None,
            user_agent: None,
            ip_address: None,
        };

        let session_id = handler.handle(create_session_cmd).await.unwrap();

        // Then create a stream
        let create_stream_cmd = CreateStreamCommand {
            session_id: session_id.into(),
            source_data: serde_json::json!({"test": "data"}),
            config: None,
        };

        let result = handler.handle(create_stream_cmd).await;
        assert!(result.is_ok());

        let stream_id = result.unwrap();
        assert_ne!(stream_id, StreamId::new()); // Should be a valid stream ID
    }

    #[tokio::test]
    async fn test_create_stream_with_config() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher);

        // Create session first
        let session_id = handler
            .handle(CreateSessionCommand {
                config: SessionConfig::default(),
                client_info: None,
                user_agent: None,
                ip_address: None,
            })
            .await
            .unwrap();

        // Create stream with config
        let stream_config = crate::domain::entities::stream::StreamConfig::default();
        let create_stream_cmd = CreateStreamCommand {
            session_id: session_id.into(),
            source_data: serde_json::json!({"test": "data"}),
            config: Some(stream_config),
        };

        let result = handler.handle(create_stream_cmd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_stream_session_not_found() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository, event_publisher);

        let non_existent_session_id = SessionId::new();
        let create_stream_cmd = CreateStreamCommand {
            session_id: non_existent_session_id.into(),
            source_data: serde_json::json!({"test": "data"}),
            config: None,
        };

        let result = handler.handle(create_stream_cmd).await;
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            ApplicationError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_start_stream_command() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher);

        // Create session and stream first
        let session_id = handler
            .handle(CreateSessionCommand {
                config: SessionConfig::default(),
                client_info: None,
                user_agent: None,
                ip_address: None,
            })
            .await
            .unwrap();

        let stream_id = handler
            .handle(CreateStreamCommand {
                session_id: session_id.into(),
                source_data: serde_json::json!({"test": "data"}),
                config: None,
            })
            .await
            .unwrap();

        // Start the stream
        let start_stream_cmd = StartStreamCommand {
            session_id: session_id.into(),
            stream_id: stream_id.into(),
        };

        let result = handler.handle(start_stream_cmd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_stream_session_not_found() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository, event_publisher);

        let start_stream_cmd = StartStreamCommand {
            session_id: SessionId::new().into(),
            stream_id: StreamId::new().into(),
        };

        let result = handler.handle(start_stream_cmd).await;
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            ApplicationError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_complete_stream_command() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher);

        // Create session, stream, and start it
        let session_id = handler
            .handle(CreateSessionCommand {
                config: SessionConfig::default(),
                client_info: None,
                user_agent: None,
                ip_address: None,
            })
            .await
            .unwrap();

        let stream_id = handler
            .handle(CreateStreamCommand {
                session_id: session_id.into(),
                source_data: serde_json::json!({"test": "data"}),
                config: None,
            })
            .await
            .unwrap();

        handler
            .handle(StartStreamCommand {
                session_id: session_id.into(),
                stream_id: stream_id.into(),
            })
            .await
            .unwrap();

        // Complete the stream
        let complete_stream_cmd = CompleteStreamCommand {
            session_id: session_id.into(),
            stream_id: stream_id.into(),
            checksum: Some("abc123".to_string()),
        };

        let result = handler.handle(complete_stream_cmd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_complete_stream_without_checksum() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher);

        // Create and start stream
        let session_id = handler
            .handle(CreateSessionCommand {
                config: SessionConfig::default(),
                client_info: None,
                user_agent: None,
                ip_address: None,
            })
            .await
            .unwrap();

        let stream_id = handler
            .handle(CreateStreamCommand {
                session_id: session_id.into(),
                source_data: serde_json::json!({"test": "data"}),
                config: None,
            })
            .await
            .unwrap();

        handler
            .handle(StartStreamCommand {
                session_id: session_id.into(),
                stream_id: stream_id.into(),
            })
            .await
            .unwrap();

        // Complete without checksum
        let complete_stream_cmd = CompleteStreamCommand {
            session_id: session_id.into(),
            stream_id: stream_id.into(),
            checksum: None,
        };

        let result = handler.handle(complete_stream_cmd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_close_session_command() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher);

        // Create session first
        let session_id = handler
            .handle(CreateSessionCommand {
                config: SessionConfig::default(),
                client_info: None,
                user_agent: None,
                ip_address: None,
            })
            .await
            .unwrap();

        // Close the session
        let close_session_cmd = CloseSessionCommand {
            session_id: session_id.into(),
        };

        let result = handler.handle(close_session_cmd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_close_session_not_found() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository, event_publisher);

        let close_session_cmd = CloseSessionCommand {
            session_id: SessionId::new().into(),
        };

        let result = handler.handle(close_session_cmd).await;
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            ApplicationError::NotFound(_)
        ));
    }

    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    mod dictionary_wiring {
        //! Regression tests for issue #224 — frame-ingest must feed the per-session
        //! training corpus so that `GET /pjs/sessions/{id}/dictionary` becomes
        //! reachable end-to-end.
        use super::*;
        use crate::{
            compression::zstd::N_TRAIN,
            domain::{
                entities::Frame,
                ports::{DictionaryFuture, DictionaryStore},
            },
            infrastructure::repositories::InMemoryDictionaryStore,
            security::CompressionBombDetector,
        };
        use pjson_rs_domain::value_objects::{JsonData, StreamId};
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// Counts every `train_if_ready` invocation so a test can verify the
        /// command handler reaches the dictionary store at all.
        struct CountingDictionaryStore {
            inner: InMemoryDictionaryStore,
            calls: AtomicUsize,
        }

        impl CountingDictionaryStore {
            fn new() -> Self {
                Self {
                    inner: InMemoryDictionaryStore::new(
                        Arc::new(CompressionBombDetector::default()),
                        64 * 1024,
                    ),
                    calls: AtomicUsize::new(0),
                }
            }

            fn call_count(&self) -> usize {
                self.calls.load(Ordering::SeqCst)
            }
        }

        impl DictionaryStore for CountingDictionaryStore {
            fn get_dictionary<'a>(
                &'a self,
                session_id: SessionId,
            ) -> DictionaryFuture<'a, Option<Arc<crate::compression::zstd::ZstdDictionary>>>
            {
                self.inner.get_dictionary(session_id)
            }

            fn train_if_ready<'a>(
                &'a self,
                session_id: SessionId,
                sample: Vec<u8>,
            ) -> DictionaryFuture<'a, ()> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.inner.train_if_ready(session_id, sample)
            }
        }

        fn make_patch_frame(stream_id: StreamId, sequence: u64, n: usize) -> Frame {
            let patch = crate::domain::entities::frame::FramePatch::set(
                pjson_rs_domain::value_objects::JsonPath::new(format!("$.items[{n}]")).unwrap(),
                JsonData::Integer(n as i64),
            );
            Frame::patch(
                stream_id,
                sequence,
                pjson_rs_domain::value_objects::Priority::HIGH,
                vec![patch],
            )
            .unwrap()
        }

        #[tokio::test]
        async fn test_train_from_frames_records_each_payload() {
            let store = Arc::new(CountingDictionaryStore::new());
            let handler = SessionCommandHandler::with_dictionary_store(
                Arc::new(MockRepository::new()),
                Arc::new(MockEventPublisher),
                store.clone(),
            );

            let session_id = SessionId::new();
            let stream_id = StreamId::new();
            let frames: Vec<Frame> = (0..5)
                .map(|i| make_patch_frame(stream_id, i as u64, i))
                .collect();

            handler.train_from_frames(session_id, &frames).await;

            assert_eq!(
                store.call_count(),
                5,
                "every accepted frame must feed train_if_ready"
            );
        }

        #[tokio::test]
        async fn test_train_from_frames_fires_dictionary_after_threshold() {
            let store = Arc::new(InMemoryDictionaryStore::new(
                Arc::new(CompressionBombDetector::default()),
                64 * 1024,
            ));
            let handler = SessionCommandHandler::with_dictionary_store(
                Arc::new(MockRepository::new()),
                Arc::new(MockEventPublisher),
                store.clone(),
            );

            let session_id = SessionId::new();
            let stream_id = StreamId::new();
            let frames: Vec<Frame> = (0..N_TRAIN)
                .map(|i| make_patch_frame(stream_id, i as u64, i))
                .collect();

            handler.train_from_frames(session_id, &frames).await;

            let dict = store.get_dictionary(session_id).await.unwrap();
            assert!(
                dict.is_some(),
                "dictionary must be trained once N_TRAIN frame payloads have been ingested"
            );
        }
    }

    #[tokio::test]
    async fn test_generate_frames_persists_into_frame_store() {
        use crate::domain::ports::FrameStoreGat;
        use crate::infrastructure::adapters::InMemoryFrameStore;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let frame_store = Arc::new(InMemoryFrameStore::new());

        let handler = SessionCommandHandler::with_stores(
            repository.clone(),
            event_publisher,
            Arc::new(crate::domain::ports::NoopDictionaryStore),
            frame_store.clone(),
        );

        // Bring up an active session with one stream.
        let session_id = handler
            .handle(CreateSessionCommand {
                config: SessionConfig::default(),
                client_info: None,
                user_agent: None,
                ip_address: None,
            })
            .await
            .unwrap();

        let stream_id = handler
            .handle(CreateStreamCommand {
                session_id: session_id.into(),
                source_data: serde_json::json!({"items": [1, 2, 3, 4]}),
                config: None,
            })
            .await
            .unwrap();

        // Frames can only be produced from a streaming stream.
        handler
            .handle(StartStreamCommand {
                session_id: session_id.into(),
                stream_id: stream_id.into(),
            })
            .await
            .unwrap();

        // GenerateFrames must (a) return the frames and (b) leave them in the
        // frame store so the GET endpoint can find them.
        let frames = handler
            .handle(GenerateFramesCommand {
                session_id: session_id.into(),
                stream_id: stream_id.into(),
                priority_threshold: crate::application::dto::PriorityDto::new(1).unwrap(),
                max_frames: 8,
            })
            .await
            .unwrap();

        assert!(
            !frames.is_empty(),
            "command must produce at least one frame"
        );

        let page = frame_store
            .get_frames(stream_id, None, None, None)
            .await
            .unwrap();
        assert_eq!(
            page.frames.len(),
            frames.len(),
            "every frame returned by the command must be persisted",
        );
        assert_eq!(page.total_matching, frames.len());
    }
}
