//! High-level session management service

use std::sync::Arc;
use chrono::{DateTime, Utc};
use crate::{
    application::{
        ApplicationResult,
        commands::*,
        queries::*,
        handlers::{CommandHandler, QueryHandler},
    },
    domain::{
        value_objects::{SessionId, StreamId, Priority},
        aggregates::{StreamSession, stream_session::SessionHealth},
        entities::{Stream, Frame},
        events::DomainEvent,
    },
};

/// High-level service for session management workflows
#[derive(Debug)]
pub struct SessionService<CH, QH> 
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()>,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse>,
{
    command_handler: Arc<CH>,
    query_handler: Arc<QH>,
}

impl<CH, QH> SessionService<CH, QH>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         Send + Sync,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Send + Sync,
{
    pub fn new(command_handler: Arc<CH>, query_handler: Arc<QH>) -> Self {
        Self {
            command_handler,
            query_handler,
        }
    }
    
    /// Create new session with automatic activation
    pub async fn create_and_activate_session(
        &self,
        config: crate::domain::aggregates::stream_session::SessionConfig,
        client_info: Option<String>,
        user_agent: Option<String>,
        ip_address: Option<String>,
    ) -> ApplicationResult<SessionId> {
        let create_command = CreateSessionCommand {
            config,
            client_info,
            user_agent,
            ip_address,
        };
        
        // Session is automatically activated in the command handler
        self.command_handler.handle(create_command).await
    }
    
    /// Create stream and immediately start it
    pub async fn create_and_start_stream(
        &self,
        session_id: SessionId,
        source_data: serde_json::Value,
        config: Option<crate::domain::entities::stream::StreamConfig>,
    ) -> ApplicationResult<StreamId> {
        // Create stream
        let create_command = CreateStreamCommand {
            session_id,
            source_data,
            config,
        };
        
        let stream_id = self.command_handler.handle(create_command).await?;
        
        // Start stream
        let start_command = StartStreamCommand {
            session_id,
            stream_id,
        };
        
        self.command_handler.handle(start_command).await?;
        
        Ok(stream_id)
    }
    
    /// Get session with health check
    pub async fn get_session_with_health(
        &self,
        session_id: SessionId,
    ) -> ApplicationResult<SessionWithHealth> {
        // Get session info
        let session_query = GetSessionQuery { session_id };
        let session_response = self.query_handler.handle(session_query).await?;
        
        // Get health status
        let health_query = GetSessionHealthQuery { session_id };
        let health_response = self.query_handler.handle(health_query).await?;
        
        Ok(SessionWithHealth {
            session: session_response.session,
            health: health_response.health,
        })
    }
    
    /// Complete stream and close session if no more active streams
    pub async fn complete_stream_and_maybe_close_session(
        &self,
        session_id: SessionId,
        stream_id: StreamId,
    ) -> ApplicationResult<SessionCompletionResult> {
        // Complete the stream
        let complete_command = CompleteStreamCommand {
            session_id,
            stream_id,
            checksum: None,
        };
        
        self.command_handler.handle(complete_command).await?;
        
        // Check if session should be closed
        let session_query = GetSessionQuery { session_id };
        let session_response = self.query_handler.handle(session_query).await?;
        
        let active_streams = session_response.session.streams()
            .values()
            .filter(|s| s.is_active())
            .count();
        
        let session_closed = if active_streams == 0 {
            // No more active streams, close the session
            let close_command = CloseSessionCommand { session_id };
            self.command_handler.handle(close_command).await?;
            true
        } else {
            false
        };
        
        Ok(SessionCompletionResult {
            stream_id,
            session_closed,
            remaining_active_streams: active_streams,
        })
    }
    
    /// Get overview of all active sessions
    pub async fn get_sessions_overview(
        &self,
        limit: Option<usize>,
    ) -> ApplicationResult<SessionsOverview> {
        let query = GetActiveSessionsQuery {
            limit,
            offset: None,
        };
        
        let response = self.query_handler.handle(query).await?;
        
        // Calculate aggregated statistics
        let mut total_streams = 0u64;
        let mut total_frames = 0u64;
        let mut total_bytes = 0u64;
        let mut healthy_sessions = 0usize;
        
        for session in &response.sessions {
            let stats = session.stats();
            total_streams += stats.total_streams;
            total_frames += stats.total_frames;
            total_bytes += stats.total_bytes;
            
            if session.health_check().is_healthy {
                healthy_sessions += 1;
            }
        }
        
        Ok(SessionsOverview {
            sessions: response.sessions,
            total_count: response.total_count,
            healthy_count: healthy_sessions,
            total_streams,
            total_frames,
            total_bytes,
        })
    }
    
    /// Gracefully shutdown session with all streams
    pub async fn graceful_shutdown_session(
        &self,
        session_id: SessionId,
    ) -> ApplicationResult<SessionShutdownResult> {
        // Get current session state
        let session_query = GetSessionQuery { session_id };
        let session_response = self.query_handler.handle(session_query).await?;
        
        let active_stream_ids: Vec<StreamId> = session_response.session.streams()
            .iter()
            .filter(|(_, stream)| stream.is_active())
            .map(|(id, _)| *id)
            .collect();
        
        // Complete all active streams
        let mut completed_streams = Vec::new();
        let mut failed_streams = Vec::new();
        
        for stream_id in &active_stream_ids {
            let complete_command = CompleteStreamCommand {
                session_id,
                stream_id: *stream_id,
                checksum: None,
            };
            
            match self.command_handler.handle(complete_command).await {
                Ok(_) => completed_streams.push(*stream_id),
                Err(_) => failed_streams.push(*stream_id),
            }
        }
        
        // Close the session
        let close_command = CloseSessionCommand { session_id };
        let session_closed = self.command_handler.handle(close_command).await.is_ok();
        
        Ok(SessionShutdownResult {
            session_id,
            session_closed,
            completed_streams,
            failed_streams,
        })
    }
}

/// Session with health information
#[derive(Debug, Clone)]
pub struct SessionWithHealth {
    pub session: StreamSession,
    pub health: SessionHealth,
}

/// Result of stream completion workflow
#[derive(Debug, Clone)]
pub struct SessionCompletionResult {
    pub stream_id: StreamId,
    pub session_closed: bool,
    pub remaining_active_streams: usize,
}

/// Overview of multiple sessions
#[derive(Debug, Clone)]
pub struct SessionsOverview {
    pub sessions: Vec<StreamSession>,
    pub total_count: usize,
    pub healthy_count: usize,
    pub total_streams: u64,
    pub total_frames: u64,
    pub total_bytes: u64,
}

/// Result of session shutdown workflow
#[derive(Debug, Clone)]
pub struct SessionShutdownResult {
    pub session_id: SessionId,
    pub session_closed: bool,
    pub completed_streams: Vec<StreamId>,
    pub failed_streams: Vec<StreamId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{ApplicationError, ApplicationResult},
        domain::aggregates::stream_session::SessionConfig,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    
    // Mock command handler for testing
    struct MockCommandHandler {
        sessions: std::sync::Mutex<HashMap<SessionId, StreamSession>>,
    }
    
    impl MockCommandHandler {
        fn new() -> Self {
            Self {
                sessions: std::sync::Mutex::new(HashMap::new()),
            }
        }
    }
    
    #[async_trait]
    impl CommandHandler<CreateSessionCommand, SessionId> for MockCommandHandler {
        async fn handle(&self, command: CreateSessionCommand) -> ApplicationResult<SessionId> {
            let mut session = StreamSession::new(command.config);
            let _ = session.activate();
            let session_id = session.id();
            self.sessions.lock().unwrap().insert(session_id, session);
            Ok(session_id)
        }
    }
    
    #[async_trait]
    impl CommandHandler<CreateStreamCommand, StreamId> for MockCommandHandler {
        async fn handle(&self, command: CreateStreamCommand) -> ApplicationResult<StreamId> {
            let mut sessions = self.sessions.lock().unwrap();
            if let Some(session) = sessions.get_mut(&command.session_id) {
                let stream_id = session.create_stream(command.source_data)
                    .map_err(ApplicationError::Domain)?;
                Ok(stream_id)
            } else {
                Err(ApplicationError::NotFound("Session not found".to_string()))
            }
        }
    }
    
    #[async_trait]
    impl CommandHandler<StartStreamCommand, ()> for MockCommandHandler {
        async fn handle(&self, command: StartStreamCommand) -> ApplicationResult<()> {
            let mut sessions = self.sessions.lock().unwrap();
            if let Some(session) = sessions.get_mut(&command.session_id) {
                session.start_stream(command.stream_id)
                    .map_err(ApplicationError::Domain)?;
                Ok(())
            } else {
                Err(ApplicationError::NotFound("Session not found".to_string()))
            }
        }
    }
    
    #[async_trait]
    impl CommandHandler<CompleteStreamCommand, ()> for MockCommandHandler {
        async fn handle(&self, command: CompleteStreamCommand) -> ApplicationResult<()> {
            let mut sessions = self.sessions.lock().unwrap();
            if let Some(session) = sessions.get_mut(&command.session_id) {
                session.complete_stream(command.stream_id)
                    .map_err(ApplicationError::Domain)?;
                Ok(())
            } else {
                Err(ApplicationError::NotFound("Session not found".to_string()))
            }
        }
    }
    
    #[async_trait]
    impl CommandHandler<CloseSessionCommand, ()> for MockCommandHandler {
        async fn handle(&self, command: CloseSessionCommand) -> ApplicationResult<()> {
            let mut sessions = self.sessions.lock().unwrap();
            if let Some(session) = sessions.get_mut(&command.session_id) {
                session.close().map_err(ApplicationError::Domain)?;
                Ok(())
            } else {
                Err(ApplicationError::NotFound("Session not found".to_string()))
            }
        }
    }
    
    // Mock query handler
    struct MockQueryHandler {
        sessions: std::sync::Mutex<HashMap<SessionId, StreamSession>>,
    }
    
    impl MockQueryHandler {
        fn new() -> Self {
            Self {
                sessions: std::sync::Mutex::new(HashMap::new()),
            }
        }
        
        fn sync_sessions(&self, sessions: &HashMap<SessionId, StreamSession>) {
            *self.sessions.lock().unwrap() = sessions.clone();
        }
    }
    
    #[async_trait]
    impl QueryHandler<GetSessionQuery, SessionResponse> for MockQueryHandler {
        async fn handle(&self, query: GetSessionQuery) -> ApplicationResult<SessionResponse> {
            let sessions = self.sessions.lock().unwrap();
            if let Some(session) = sessions.get(&query.session_id) {
                Ok(SessionResponse {
                    session: session.clone(),
                })
            } else {
                Err(ApplicationError::NotFound("Session not found".to_string()))
            }
        }
    }
    
    #[async_trait]
    impl QueryHandler<GetSessionHealthQuery, HealthResponse> for MockQueryHandler {
        async fn handle(&self, query: GetSessionHealthQuery) -> ApplicationResult<HealthResponse> {
            let sessions = self.sessions.lock().unwrap();
            if let Some(session) = sessions.get(&query.session_id) {
                Ok(HealthResponse {
                    health: session.health_check(),
                })
            } else {
                Err(ApplicationError::NotFound("Session not found".to_string()))
            }
        }
    }
    
    #[async_trait]
    impl QueryHandler<GetActiveSessionsQuery, SessionsResponse> for MockQueryHandler {
        async fn handle(&self, query: GetActiveSessionsQuery) -> ApplicationResult<SessionsResponse> {
            let sessions: Vec<_> = self.sessions.lock().unwrap().values().cloned().collect();
            let limited_sessions = if let Some(limit) = query.limit {
                sessions.into_iter().take(limit).collect()
            } else {
                sessions
            };
            
            Ok(SessionsResponse {
                sessions: limited_sessions.clone(),
                total_count: limited_sessions.len(),
            })
        }
    }
    
    #[tokio::test]
    async fn test_create_and_activate_session() {
        let command_handler = Arc::new(MockCommandHandler::new());
        let query_handler = Arc::new(MockQueryHandler::new());
        let service = SessionService::new(command_handler, query_handler);
        
        let result = service.create_and_activate_session(
            SessionConfig::default(),
            Some("test-client".to_string()),
            None,
            None,
        ).await;
        
        assert!(result.is_ok());
    }
}