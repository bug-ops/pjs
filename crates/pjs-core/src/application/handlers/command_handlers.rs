//! Command handlers implementing business use cases

use crate::{
    application::{ApplicationError, ApplicationResult, commands::*, handlers::CommandHandler},
    domain::{
        aggregates::StreamSession,
        entities::Frame,
        ports::{EventPublisherGat, StreamRepositoryGat},
        value_objects::{SessionId, StreamId},
    },
};
// async_trait removed - using GAT traits
use std::sync::Arc;

/// Handler for session management commands
#[derive(Debug)]
pub struct SessionCommandHandler<R, P>
where
    R: StreamRepositoryGat + 'static,
    P: EventPublisherGat + 'static,
{
    repository: Arc<R>,
    event_publisher: Arc<P>,
}

impl<R, P> SessionCommandHandler<R, P>
where
    R: StreamRepositoryGat + 'static,
    P: EventPublisherGat + 'static,
{
    pub fn new(repository: Arc<R>, event_publisher: Arc<P>) -> Self {
        Self {
            repository,
            event_publisher,
        }
    }
}

#[async_trait]
impl<R, P> CommandHandler<CreateSessionCommand, SessionId> for SessionCommandHandler<R, P>
where
    R: StreamRepository + Send + Sync,
    P: EventPublisher + Send + Sync,
{
    async fn handle(&self, command: CreateSessionCommand) -> ApplicationResult<SessionId> {
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

        // Publish events
        let events = session.take_events();
        for event in events {
            self.event_publisher
                .publish(event)
                .await
                .map_err(ApplicationError::Domain)?;
        }

        Ok(session_id)
    }
}

#[async_trait]
impl<R, P> CommandHandler<CreateStreamCommand, StreamId> for SessionCommandHandler<R, P>
where
    R: StreamRepository + Send + Sync,
    P: EventPublisher + Send + Sync,
{
    async fn handle(&self, command: CreateStreamCommand) -> ApplicationResult<StreamId> {
        // Load session
        let mut session = self
            .repository
            .find_session(command.session_id.into())
            .await
            .map_err(ApplicationError::Domain)?
            .ok_or_else(|| {
                ApplicationError::NotFound(format!("Session {} not found", command.session_id))
            })?;

        // Create stream in session
        let stream_id = session
            .create_stream(command.source_data)
            .map_err(ApplicationError::Domain)?;

        // Update stream configuration if provided
        if let Some(config) = command.config
            && let Some(stream) = session.get_stream_mut(stream_id) {
                stream
                    .update_config(config)
                    .map_err(ApplicationError::Domain)?;
            }

        // Save updated session
        self.repository
            .save_session(session.clone())
            .await
            .map_err(ApplicationError::Domain)?;

        // Publish events
        let events = session.take_events();
        for event in events {
            self.event_publisher
                .publish(event)
                .await
                .map_err(ApplicationError::Domain)?;
        }

        Ok(stream_id)
    }
}

#[async_trait]
impl<R, P> CommandHandler<StartStreamCommand, ()> for SessionCommandHandler<R, P>
where
    R: StreamRepository + Send + Sync,
    P: EventPublisher + Send + Sync,
{
    async fn handle(&self, command: StartStreamCommand) -> ApplicationResult<()> {
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

        // Publish events
        let events = session.take_events();
        for event in events {
            self.event_publisher
                .publish(event)
                .await
                .map_err(ApplicationError::Domain)?;
        }

        Ok(())
    }
}

#[async_trait]
impl<R, P> CommandHandler<CompleteStreamCommand, ()> for SessionCommandHandler<R, P>
where
    R: StreamRepository + Send + Sync,
    P: EventPublisher + Send + Sync,
{
    async fn handle(&self, command: CompleteStreamCommand) -> ApplicationResult<()> {
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

        // Publish events
        let events = session.take_events();
        for event in events {
            self.event_publisher
                .publish(event)
                .await
                .map_err(ApplicationError::Domain)?;
        }

        Ok(())
    }
}

#[async_trait]
impl<R, P> CommandHandler<GenerateFramesCommand, Vec<Frame>> for SessionCommandHandler<R, P>
where
    R: StreamRepository + Send + Sync,
    P: EventPublisher + Send + Sync,
{
    async fn handle(&self, command: GenerateFramesCommand) -> ApplicationResult<Vec<Frame>> {
        // Load session
        let mut session = self
            .repository
            .find_session(command.session_id.into())
            .await
            .map_err(ApplicationError::Domain)?
            .ok_or_else(|| {
                ApplicationError::NotFound(format!("Session {} not found", command.session_id))
            })?;

        // Get stream
        let stream = session.get_stream_mut(command.stream_id.into()).ok_or_else(|| {
            ApplicationError::NotFound(format!("Stream {} not found", command.stream_id))
        })?;

        // Generate frames
        let priority = command.priority_threshold.try_into()
            .map_err(ApplicationError::Domain)?;
        let frames = stream
            .create_patch_frames(priority, command.max_frames)
            .map_err(ApplicationError::Domain)?;

        // Save updated session
        self.repository
            .save_session(session.clone())
            .await
            .map_err(ApplicationError::Domain)?;

        // Publish events
        let events = session.take_events();
        for event in events {
            self.event_publisher
                .publish(event)
                .await
                .map_err(ApplicationError::Domain)?;
        }

        Ok(frames)
    }
}

#[async_trait]
impl<R, P> CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> for SessionCommandHandler<R, P>
where
    R: StreamRepository + Send + Sync,
    P: EventPublisher + Send + Sync,
{
    async fn handle(&self, command: BatchGenerateFramesCommand) -> ApplicationResult<Vec<Frame>> {
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

        // Save updated session
        self.repository
            .save_session(session.clone())
            .await
            .map_err(ApplicationError::Domain)?;

        // Publish events
        let events = session.take_events();
        for event in events {
            self.event_publisher
                .publish(event)
                .await
                .map_err(ApplicationError::Domain)?;
        }

        Ok(frames)
    }
}

#[async_trait]
impl<R, P> CommandHandler<CloseSessionCommand, ()> for SessionCommandHandler<R, P>
where
    R: StreamRepository + Send + Sync,
    P: EventPublisher + Send + Sync,
{
    async fn handle(&self, command: CloseSessionCommand) -> ApplicationResult<()> {
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

        // Publish events
        let events = session.take_events();
        for event in events {
            self.event_publisher
                .publish(event)
                .await
                .map_err(ApplicationError::Domain)?;
        }

        Ok(())
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
        ports::{EventPublisherGat, StreamRepositoryGat},
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
    }

    #[async_trait]
    impl StreamRepository for MockRepository {
        async fn find_session(
            &self,
            session_id: SessionId,
        ) -> crate::domain::DomainResult<Option<StreamSession>> {
            // TODO: Handle unwrap() - add proper error handling for mutex poisoning
            Ok(self.sessions.lock().unwrap().get(&session_id).cloned())
        }

        async fn save_session(&self, session: StreamSession) -> crate::domain::DomainResult<()> {
            // TODO: Handle unwrap() - add proper error handling for mutex poisoning
            self.sessions.lock().unwrap().insert(session.id(), session);
            Ok(())
        }

        async fn remove_session(&self, session_id: SessionId) -> crate::domain::DomainResult<()> {
            // TODO: Handle unwrap() - add proper error handling for mutex poisoning
            self.sessions.lock().unwrap().remove(&session_id);
            Ok(())
        }

        async fn find_active_sessions(&self) -> crate::domain::DomainResult<Vec<StreamSession>> {
            // TODO: Handle unwrap() - add proper error handling for mutex poisoning
            Ok(self.sessions.lock().unwrap().values().cloned().collect())
        }
    }

    struct MockEventPublisher;

    #[async_trait]
    impl EventPublisher for MockEventPublisher {
        async fn publish(&self, _event: DomainEvent) -> crate::domain::DomainResult<()> {
            Ok(())
        }

        async fn publish_batch(
            &self,
            _events: Vec<DomainEvent>,
        ) -> crate::domain::DomainResult<()> {
            Ok(())
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

        assert!(std::ptr::eq(handler.repository.as_ref(), repository.as_ref()));
        assert!(std::ptr::eq(handler.event_publisher.as_ref(), event_publisher.as_ref()));
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
        let session_id = handler.handle(CreateSessionCommand {
            config: SessionConfig::default(),
            client_info: None,
            user_agent: None,
            ip_address: None,
        }).await.unwrap();

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
        assert!(matches!(result.err().unwrap(), ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_start_stream_command() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher);

        // Create session and stream first
        let session_id = handler.handle(CreateSessionCommand {
            config: SessionConfig::default(),
            client_info: None,
            user_agent: None,
            ip_address: None,
        }).await.unwrap();

        let stream_id = handler.handle(CreateStreamCommand {
            session_id: session_id.into(),
            source_data: serde_json::json!({"test": "data"}),
            config: None,
        }).await.unwrap();

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
        assert!(matches!(result.err().unwrap(), ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_complete_stream_command() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let handler = SessionCommandHandler::new(repository.clone(), event_publisher);

        // Create session, stream, and start it
        let session_id = handler.handle(CreateSessionCommand {
            config: SessionConfig::default(),
            client_info: None,
            user_agent: None,
            ip_address: None,
        }).await.unwrap();

        let stream_id = handler.handle(CreateStreamCommand {
            session_id: session_id.into(),
            source_data: serde_json::json!({"test": "data"}),
            config: None,
        }).await.unwrap();

        handler.handle(StartStreamCommand {
            session_id: session_id.into(),
            stream_id: stream_id.into(),
        }).await.unwrap();

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
        let session_id = handler.handle(CreateSessionCommand {
            config: SessionConfig::default(),
            client_info: None,
            user_agent: None,
            ip_address: None,
        }).await.unwrap();

        let stream_id = handler.handle(CreateStreamCommand {
            session_id: session_id.into(),
            source_data: serde_json::json!({"test": "data"}),
            config: None,
        }).await.unwrap();

        handler.handle(StartStreamCommand {
            session_id: session_id.into(),
            stream_id: stream_id.into(),
        }).await.unwrap();

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
        let session_id = handler.handle(CreateSessionCommand {
            config: SessionConfig::default(),
            client_info: None,
            user_agent: None,
            ip_address: None,
        }).await.unwrap();

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
        assert!(matches!(result.err().unwrap(), ApplicationError::NotFound(_)));
    }
}
