//! Event service for handling domain events with DTO serialization
//!
//! This service provides a clean boundary between domain events and their
//! serialized representations, maintaining Clean Architecture principles.

use crate::{
    application::{
        ApplicationResult,
        dto::{DomainEventDto, ToDto, FromDto},
    },
    domain::{
        events::{DomainEvent, EventStore},
        value_objects::{SessionId, StreamId},
    },
};
use tracing;
use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};

/// Service for managing domain events with DTO conversion
/// Uses compile-time polymorphism for zero-cost abstractions
pub struct EventService<S, H>
where
    S: EventStore,
    H: EventHandler,
{
    event_store: Arc<Mutex<S>>,
    event_handler: H,
}

/// Zero-cost event handler trait with stack allocation
pub trait EventHandler {
    type HandleFuture<'a>: std::future::Future<Output = ApplicationResult<()>> + Send + 'a
    where
        Self: 'a;

    fn handle_event(&self, event: &DomainEvent) -> Self::HandleFuture<'_>;
}

/// No-op handler for when events don't need processing
pub struct NoOpEventHandler;

impl EventHandler for NoOpEventHandler {
    type HandleFuture<'a> = impl std::future::Future<Output = ApplicationResult<()>> + Send + 'a
    where
        Self: 'a;

    fn handle_event(&self, _event: &DomainEvent) -> Self::HandleFuture<'_> {
        async move { Ok(()) }
    }
}

/// Logging handler with zero allocations
pub struct LoggingEventHandler;

impl EventHandler for LoggingEventHandler {
    type HandleFuture<'a> = impl std::future::Future<Output = ApplicationResult<()>> + Send + 'a
    where
        Self: 'a;

    fn handle_event(&self, event: &DomainEvent) -> Self::HandleFuture<'_> {
        let event_type = event.event_type();
        let session_id = event.session_id();
        let stream_id = event.stream_id();
        
        async move {
            // Stack-allocated logging without heap allocation
            match event_type {
                "stream_completed" => {
                    if let Some(stream) = stream_id {
                        tracing::info!("Stream completed: session={}, stream={}", session_id, stream);
                    }
                }
                "session_activated" => {
                    tracing::info!("Session activated: {}", session_id);
                }
                _ => {
                    tracing::debug!("Domain event processed: {}", event_type);
                }
            }
            Ok(())
        }
    }
}

impl<S, H> EventService<S, H>
where
    S: EventStore,
    H: EventHandler,
{
    /// Create new event service with event store and handler
    pub fn new(event_store: Arc<Mutex<S>>, event_handler: H) -> Self {
        Self {
            event_store,
            event_handler,
        }
    }

    /// Publish domain event with zero-cost handling
    pub async fn publish_event(&self, event: DomainEvent) -> ApplicationResult<()> {
        // Handle event with zero-cost abstraction
        self.event_handler.handle_event(&event).await?;

        // Store event
        self.event_store
            .lock()
            .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire event store lock".to_string()))?
            .append_events(vec![event])
            .map_err(crate::application::ApplicationError::Logic)?;

        Ok(())
    }

    /// Publish multiple domain events
    pub async fn publish_events(&self, events: Vec<DomainEvent>) -> ApplicationResult<()> {
        // Handle events with zero-cost abstraction
        for event in &events {
            self.event_handler.handle_event(event).await?;
        }

        // Store all events
        self.event_store
            .lock()
            .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire event store lock".to_string()))?
            .append_events(events)
            .map_err(crate::application::ApplicationError::Logic)?;

        Ok(())
    }

    /// Get events for session as DTOs (for API responses)
    pub fn get_session_events_dto(&self, session_id: SessionId) -> ApplicationResult<Vec<DomainEventDto>> {
        let events = self.event_store
            .lock()
            .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire event store lock".to_string()))?
            .get_events_for_session(session_id)
            .map_err(crate::application::ApplicationError::Logic)?;

        Ok(events.into_iter().map(|e| e.to_dto()).collect())
    }

    /// Get events for stream as DTOs (for API responses)
    pub fn get_stream_events_dto(&self, stream_id: StreamId) -> ApplicationResult<Vec<DomainEventDto>> {
        let events = self.event_store
            .lock()
            .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire event store lock".to_string()))?
            .get_events_for_stream(stream_id)
            .map_err(crate::application::ApplicationError::Logic)?;

        Ok(events.into_iter().map(|e| e.to_dto()).collect())
    }

    /// Get events since timestamp as DTOs (for API responses)
    pub fn get_events_since_dto(&self, since: DateTime<Utc>) -> ApplicationResult<Vec<DomainEventDto>> {
        let events = self.event_store
            .lock()
            .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire event store lock".to_string()))?
            .get_events_since(since)
            .map_err(crate::application::ApplicationError::Logic)?;

        Ok(events.into_iter().map(|e| e.to_dto()).collect())
    }

    /// Get events for session (domain objects, for internal use)
    pub fn get_session_events(&self, session_id: SessionId) -> ApplicationResult<Vec<DomainEvent>> {
        self.event_store
            .lock()
            .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire event store lock".to_string()))?
            .get_events_for_session(session_id)
            .map_err(crate::application::ApplicationError::Logic)
    }

    /// Get events for stream (domain objects, for internal use)
    pub fn get_stream_events(&self, stream_id: StreamId) -> ApplicationResult<Vec<DomainEvent>> {
        self.event_store
            .lock()
            .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire event store lock".to_string()))?
            .get_events_for_stream(stream_id)
            .map_err(crate::application::ApplicationError::Logic)
    }

    /// Replay events from DTOs (for event sourcing reconstruction)
    pub fn replay_from_dtos(&self, event_dtos: Vec<DomainEventDto>) -> ApplicationResult<Vec<DomainEvent>> {
        let mut events = Vec::new();
        
        for dto in event_dtos {
            let event = DomainEvent::from_dto(dto)
                .map_err(crate::application::ApplicationError::Domain)?;
            events.push(event);
        }

        Ok(events)
    }
}

/// Convenience constructors for common configurations
impl<S> EventService<S, NoOpEventHandler>
where
    S: EventStore,
{
    /// Create event service with no-op handler (maximum performance)
    pub fn with_noop_handler(event_store: Arc<Mutex<S>>) -> Self {
        Self::new(event_store, NoOpEventHandler)
    }
}

impl<S> EventService<S, LoggingEventHandler>
where
    S: EventStore,
{
    /// Create event service with logging handler (zero-allocation logging)
    pub fn with_logging_handler(event_store: Arc<Mutex<S>>) -> Self {
        Self::new(event_store, LoggingEventHandler)
    }
}

/// Event publishing convenience methods
impl<S, H> EventService<S, H>
where
    S: EventStore,
    H: EventHandler,
{
    /// Publish session activated event  
    pub async fn publish_session_activated(&self, session_id: SessionId) -> ApplicationResult<()> {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: Utc::now(),
        };
        self.publish_event(event).await
    }

    /// Publish session closed event
    pub async fn publish_session_closed(&self, session_id: SessionId) -> ApplicationResult<()> {
        let event = DomainEvent::SessionClosed {
            session_id,
            timestamp: Utc::now(),
        };
        self.publish_event(event).await
    }

    /// Publish stream created event
    pub async fn publish_stream_created(
        &self, 
        session_id: SessionId, 
        stream_id: StreamId
    ) -> ApplicationResult<()> {
        let event = DomainEvent::StreamCreated {
            session_id,
            stream_id,
            timestamp: Utc::now(),
        };
        self.publish_event(event).await
    }

    /// Publish stream completed event
    pub async fn publish_stream_completed(
        &self, 
        session_id: SessionId, 
        stream_id: StreamId
    ) -> ApplicationResult<()> {
        let event = DomainEvent::StreamCompleted {
            session_id,
            stream_id,
            timestamp: Utc::now(),
        };
        self.publish_event(event).await
    }

    /// Publish stream failed event
    pub async fn publish_stream_failed(
        &self, 
        session_id: SessionId, 
        stream_id: StreamId,
        error: String
    ) -> ApplicationResult<()> {
        let event = DomainEvent::StreamFailed {
            session_id,
            stream_id,
            error,
            timestamp: Utc::now(),
        };
        self.publish_event(event).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        events::{InMemoryEventStore, EventSubscriber},
        value_objects::{SessionId, StreamId},
    };
    use std::sync::RwLock;

    // Mock subscriber for testing
    struct MockSubscriber {
        received_events: std::sync::Mutex<Vec<DomainEvent>>,
    }

    impl MockSubscriber {
        fn new() -> Self {
            Self {
                received_events: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn event_count(&self) -> usize {
            self.received_events.lock()
                .map(|events| events.len())
                .unwrap_or(0)
        }
    }

    impl EventSubscriber for MockSubscriber {
        type HandleFuture<'a> = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        fn handle(&self, event: &DomainEvent) -> Self::HandleFuture<'_> {
            let event = event.clone();
            async move {
                self.received_events.lock()
                    .map_err(|_| crate::domain::DomainError::Logic("Event subscriber lock poisoned".to_string()))?
                    .push(event);
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_event_service_creation() {
        let store = Arc::new(std::sync::Mutex::new(InMemoryEventStore::new()));
        let _service = EventService::with_noop_handler(store);
        
        // Service created successfully
    }

    #[tokio::test]
    async fn test_publish_event() {
        let store = Arc::new(std::sync::Mutex::new(InMemoryEventStore::new()));
        let service = EventService::with_logging_handler(store.clone());
        
        let session_id = SessionId::new();
        
        service.publish_session_activated(session_id).await.unwrap();
        
        // Verify event was stored
        let events = service.get_session_events(session_id).unwrap();
        assert_eq!(events.len(), 1);
        
        match &events[0] {
            DomainEvent::SessionActivated { session_id: stored_id, .. } => {
                assert_eq!(*stored_id, session_id);
            },
            _ => panic!("Expected SessionActivated event"),
        }
    }

    #[tokio::test]
    async fn test_event_handler() {
        let store = Arc::new(std::sync::Mutex::new(InMemoryEventStore::new()));
        let service = EventService::with_logging_handler(store);
        
        let session_id = SessionId::new();
        service.publish_session_activated(session_id).await.unwrap();
        
        // Verify event was handled and stored
        let events = service.get_session_events(session_id).unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn test_dto_conversion() {
        let store = Arc::new(std::sync::Mutex::new(InMemoryEventStore::new()));
        let service = EventService::with_logging_handler(store);
        
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        
        service.publish_stream_created(session_id, stream_id).await.unwrap();
        
        // Get events as DTOs
        let event_dtos = service.get_session_events_dto(session_id).unwrap();
        assert_eq!(event_dtos.len(), 1);
        
        // Verify DTO structure
        match &event_dtos[0] {
            DomainEventDto::StreamCreated { session_id: dto_session_id, stream_id: dto_stream_id, .. } => {
                assert_eq!(dto_session_id.uuid(), session_id.as_uuid());
                assert_eq!(dto_stream_id.uuid(), stream_id.as_uuid());
            },
            _ => panic!("Expected StreamCreated DTO"),
        }
        
        // Test replay from DTOs
        let replayed = service.replay_from_dtos(event_dtos).unwrap();
        assert_eq!(replayed.len(), 1);
    }

    #[tokio::test]
    async fn test_multiple_events() {
        let store = Arc::new(std::sync::Mutex::new(InMemoryEventStore::new()));
        let service = EventService::with_logging_handler(store);
        
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        
        let events = vec![
            DomainEvent::SessionActivated { session_id, timestamp: Utc::now() },
            DomainEvent::StreamCreated { session_id, stream_id, timestamp: Utc::now() },
            DomainEvent::StreamStarted { session_id, stream_id, timestamp: Utc::now() },
        ];
        
        service.publish_events(events).await.unwrap();
        
        let stored_events = service.get_session_events(session_id).unwrap();
        assert_eq!(stored_events.len(), 3);
    }
}