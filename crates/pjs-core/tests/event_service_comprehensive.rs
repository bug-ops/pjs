//! Comprehensive tests for EventService
//!
//! This test suite aims to achieve 70%+ coverage by testing:
//! - Event creation and publishing
//! - Event handlers (NoOp, Logging)
//! - Event filtering and querying (by session, stream, timestamp)
//! - DTO conversion and replay
//! - Error handling and edge cases
//! - Multiple event publishing

use chrono::{Duration, Utc};
use pjson_rs::application::services::event_service::{
    EventHandler, EventService, LoggingEventHandler, NoOpEventHandler,
};
use pjson_rs::domain::events::{DomainEvent, InMemoryEventStore};
use pjson_rs::domain::value_objects::{SessionId, StreamId};
use std::sync::{Arc, Mutex};

// === Test Fixtures and Helpers ===

fn create_test_event_store() -> Arc<Mutex<InMemoryEventStore>> {
    Arc::new(Mutex::new(InMemoryEventStore::new()))
}

fn create_session_activated_event(session_id: SessionId) -> DomainEvent {
    DomainEvent::SessionActivated {
        session_id,
        timestamp: Utc::now(),
    }
}

// === Service Creation Tests ===

#[tokio::test]
async fn test_event_service_creation_with_noop_handler() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);

    let session_id = SessionId::new();
    let result = service.publish_session_activated(session_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_event_service_creation_with_logging_handler() {
    let store = create_test_event_store();
    let service = EventService::with_logging_handler(store);

    let session_id = SessionId::new();
    let result = service.publish_session_activated(session_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_event_service_creation_with_custom_handler() {
    let store = create_test_event_store();
    let service = EventService::new(store, NoOpEventHandler);

    let session_id = SessionId::new();
    let result = service.publish_session_activated(session_id).await;
    assert!(result.is_ok());
}

// === Event Publishing Tests ===

#[tokio::test]
async fn test_publish_session_activated_event() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    let result = service.publish_session_activated(session_id).await;
    assert!(result.is_ok());

    let events = service.get_session_events(session_id).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        DomainEvent::SessionActivated { session_id: id, .. } => {
            assert_eq!(*id, session_id);
        }
        _ => panic!("Expected SessionActivated event"),
    }
}

#[tokio::test]
async fn test_publish_session_closed_event() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    let result = service.publish_session_closed(session_id).await;
    assert!(result.is_ok());

    let events = service.get_session_events(session_id).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        DomainEvent::SessionClosed { session_id: id, .. } => {
            assert_eq!(*id, session_id);
        }
        _ => panic!("Expected SessionClosed event"),
    }
}

#[tokio::test]
async fn test_publish_stream_created_event() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    let result = service.publish_stream_created(session_id, stream_id).await;
    assert!(result.is_ok());

    let events = service.get_session_events(session_id).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        DomainEvent::StreamCreated {
            session_id: sid,
            stream_id: stid,
            ..
        } => {
            assert_eq!(*sid, session_id);
            assert_eq!(*stid, stream_id);
        }
        _ => panic!("Expected StreamCreated event"),
    }
}

#[tokio::test]
async fn test_publish_stream_completed_event() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    let result = service
        .publish_stream_completed(session_id, stream_id)
        .await;
    assert!(result.is_ok());

    let events = service.get_stream_events(stream_id).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        DomainEvent::StreamCompleted { stream_id: id, .. } => {
            assert_eq!(*id, stream_id);
        }
        _ => panic!("Expected StreamCompleted event"),
    }
}

#[tokio::test]
async fn test_publish_stream_failed_event() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();
    let error_msg = "Test error".to_string();

    let result = service
        .publish_stream_failed(session_id, stream_id, error_msg.clone())
        .await;
    assert!(result.is_ok());

    let events = service.get_stream_events(stream_id).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        DomainEvent::StreamFailed {
            stream_id: id,
            error,
            ..
        } => {
            assert_eq!(*id, stream_id);
            assert_eq!(*error, error_msg);
        }
        _ => panic!("Expected StreamFailed event"),
    }
}

#[tokio::test]
async fn test_publish_single_event_directly() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    let event = create_session_activated_event(session_id);
    let result = service.publish_event(event).await;
    assert!(result.is_ok());

    let events = service.get_session_events(session_id).unwrap();
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn test_publish_multiple_events() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    let events = vec![
        DomainEvent::SessionActivated {
            session_id,
            timestamp: Utc::now(),
        },
        DomainEvent::StreamCreated {
            session_id,
            stream_id,
            timestamp: Utc::now(),
        },
        DomainEvent::StreamStarted {
            session_id,
            stream_id,
            timestamp: Utc::now(),
        },
    ];

    let result = service.publish_events(events).await;
    assert!(result.is_ok());

    let stored_events = service.get_session_events(session_id).unwrap();
    assert_eq!(stored_events.len(), 3);
}

#[tokio::test]
async fn test_publish_events_empty_vec() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);

    let events = vec![];
    let result = service.publish_events(events).await;
    assert!(result.is_ok());
}

// === Event Query Tests ===

#[tokio::test]
async fn test_get_session_events_empty() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    let events = service.get_session_events(session_id).unwrap();
    assert_eq!(events.len(), 0);
}

#[tokio::test]
async fn test_get_stream_events_empty() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let stream_id = StreamId::new();

    let events = service.get_stream_events(stream_id).unwrap();
    assert_eq!(events.len(), 0);
}

#[tokio::test]
async fn test_get_session_events_multiple() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    service.publish_session_activated(session_id).await.unwrap();
    service.publish_session_closed(session_id).await.unwrap();

    let events = service.get_session_events(session_id).unwrap();
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn test_get_stream_events_multiple() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    service
        .publish_stream_created(session_id, stream_id)
        .await
        .unwrap();
    service
        .publish_stream_completed(session_id, stream_id)
        .await
        .unwrap();

    let events = service.get_stream_events(stream_id).unwrap();
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn test_get_events_filters_by_session() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session1 = SessionId::new();
    let session2 = SessionId::new();

    service.publish_session_activated(session1).await.unwrap();
    service.publish_session_activated(session2).await.unwrap();

    let events1 = service.get_session_events(session1).unwrap();
    let events2 = service.get_session_events(session2).unwrap();

    assert_eq!(events1.len(), 1);
    assert_eq!(events2.len(), 1);
}

#[tokio::test]
async fn test_get_events_filters_by_stream() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream1 = StreamId::new();
    let stream2 = StreamId::new();

    service
        .publish_stream_created(session_id, stream1)
        .await
        .unwrap();
    service
        .publish_stream_created(session_id, stream2)
        .await
        .unwrap();

    let events1 = service.get_stream_events(stream1).unwrap();
    let events2 = service.get_stream_events(stream2).unwrap();

    assert_eq!(events1.len(), 1);
    assert_eq!(events2.len(), 1);
}

// === DTO Conversion Tests ===

#[tokio::test]
async fn test_get_session_events_dto() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    service.publish_session_activated(session_id).await.unwrap();

    let event_dtos = service.get_session_events_dto(session_id).unwrap();
    assert_eq!(event_dtos.len(), 1);
}

#[tokio::test]
async fn test_get_stream_events_dto() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    service
        .publish_stream_created(session_id, stream_id)
        .await
        .unwrap();

    let event_dtos = service.get_stream_events_dto(stream_id).unwrap();
    assert_eq!(event_dtos.len(), 1);
}

#[tokio::test]
async fn test_get_events_since_dto() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    let before = Utc::now() - Duration::seconds(10);
    service.publish_session_activated(session_id).await.unwrap();

    let event_dtos = service.get_events_since_dto(before).unwrap();
    assert_eq!(event_dtos.len(), 1);
}

#[tokio::test]
async fn test_get_events_since_dto_empty() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);

    let future = Utc::now() + Duration::seconds(100);
    let event_dtos = service.get_events_since_dto(future).unwrap();
    assert_eq!(event_dtos.len(), 0);
}

#[tokio::test]
async fn test_replay_from_dtos() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    service
        .publish_stream_created(session_id, stream_id)
        .await
        .unwrap();

    let event_dtos = service.get_session_events_dto(session_id).unwrap();
    let replayed = service.replay_from_dtos(event_dtos).unwrap();

    assert_eq!(replayed.len(), 1);
    match &replayed[0] {
        DomainEvent::StreamCreated {
            session_id: sid,
            stream_id: stid,
            ..
        } => {
            assert_eq!(*sid, session_id);
            assert_eq!(*stid, stream_id);
        }
        _ => panic!("Expected StreamCreated event"),
    }
}

#[tokio::test]
async fn test_replay_from_empty_dtos() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);

    let event_dtos = vec![];
    let replayed = service.replay_from_dtos(event_dtos).unwrap();

    assert_eq!(replayed.len(), 0);
}

#[tokio::test]
async fn test_dto_roundtrip() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    service
        .publish_stream_created(session_id, stream_id)
        .await
        .unwrap();
    service
        .publish_stream_completed(session_id, stream_id)
        .await
        .unwrap();

    let event_dtos = service.get_session_events_dto(session_id).unwrap();
    let replayed = service.replay_from_dtos(event_dtos).unwrap();

    assert_eq!(replayed.len(), 2);
}

// === Event Handler Tests ===

#[tokio::test]
async fn test_noop_handler() {
    let handler = NoOpEventHandler;
    let event = create_session_activated_event(SessionId::new());

    let result = handler.handle_event(&event).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_logging_handler_session_activated() {
    let handler = LoggingEventHandler;
    let event = DomainEvent::SessionActivated {
        session_id: SessionId::new(),
        timestamp: Utc::now(),
    };

    let result = handler.handle_event(&event).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_logging_handler_stream_completed() {
    let handler = LoggingEventHandler;
    let event = DomainEvent::StreamCompleted {
        session_id: SessionId::new(),
        stream_id: StreamId::new(),
        timestamp: Utc::now(),
    };

    let result = handler.handle_event(&event).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_logging_handler_stream_started() {
    let handler = LoggingEventHandler;
    let event = DomainEvent::StreamStarted {
        session_id: SessionId::new(),
        stream_id: StreamId::new(),
        timestamp: Utc::now(),
    };

    let result = handler.handle_event(&event).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_logging_handler_stream_failed() {
    let handler = LoggingEventHandler;
    let event = DomainEvent::StreamFailed {
        session_id: SessionId::new(),
        stream_id: StreamId::new(),
        error: "test error".to_string(),
        timestamp: Utc::now(),
    };

    let result = handler.handle_event(&event).await;
    assert!(result.is_ok());
}

// === Error Handling Tests ===

#[tokio::test]
async fn test_concurrent_event_publishing() {
    let store = create_test_event_store();
    let service = Arc::new(EventService::with_noop_handler(store));

    let mut handles = vec![];
    for _ in 0..10 {
        let service_clone = Arc::clone(&service);
        let handle = tokio::spawn(async move {
            let session_id = SessionId::new();
            service_clone
                .publish_session_activated(session_id)
                .await
                .unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_event_ordering_preservation() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    service.publish_session_activated(session_id).await.unwrap();
    service
        .publish_stream_created(session_id, stream_id)
        .await
        .unwrap();
    service
        .publish_stream_completed(session_id, stream_id)
        .await
        .unwrap();
    service.publish_session_closed(session_id).await.unwrap();

    let events = service.get_session_events(session_id).unwrap();
    assert_eq!(events.len(), 4);

    // Verify event order
    assert!(matches!(events[0], DomainEvent::SessionActivated { .. }));
    assert!(matches!(events[1], DomainEvent::StreamCreated { .. }));
    assert!(matches!(events[2], DomainEvent::StreamCompleted { .. }));
    assert!(matches!(events[3], DomainEvent::SessionClosed { .. }));
}

// === Edge Cases ===

#[tokio::test]
async fn test_publish_many_events_same_session() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    for _ in 0..100 {
        service.publish_session_activated(session_id).await.unwrap();
    }

    let events = service.get_session_events(session_id).unwrap();
    assert_eq!(events.len(), 100);
}

#[tokio::test]
async fn test_multiple_streams_same_session() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    for _ in 0..10 {
        let stream_id = StreamId::new();
        service
            .publish_stream_created(session_id, stream_id)
            .await
            .unwrap();
    }

    let events = service.get_session_events(session_id).unwrap();
    assert_eq!(events.len(), 10);
}

#[tokio::test]
async fn test_timestamp_filtering() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    let timestamp_before = Utc::now() - Duration::seconds(5);

    // Publish event after timestamp
    service.publish_session_activated(session_id).await.unwrap();

    let events_since = service.get_events_since_dto(timestamp_before).unwrap();
    assert_eq!(events_since.len(), 1);
}

#[tokio::test]
async fn test_event_type_detection() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();

    let event = create_session_activated_event(session_id);
    service.publish_event(event.clone()).await.unwrap();

    assert_eq!(event.event_type(), "session_activated");
}

#[tokio::test]
async fn test_stream_started_event() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    let event = DomainEvent::StreamStarted {
        session_id,
        stream_id,
        timestamp: Utc::now(),
    };

    service.publish_event(event).await.unwrap();

    let events = service.get_stream_events(stream_id).unwrap();
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn test_large_error_message() {
    let store = create_test_event_store();
    let service = EventService::with_noop_handler(store);
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    let large_error = "error ".repeat(1000);
    service
        .publish_stream_failed(session_id, stream_id, large_error.clone())
        .await
        .unwrap();

    let events = service.get_stream_events(stream_id).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        DomainEvent::StreamFailed { error, .. } => {
            assert_eq!(error.len(), large_error.len());
        }
        _ => panic!("Expected StreamFailed event"),
    }
}

#[tokio::test]
async fn test_event_metadata_extraction() {
    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    let event = DomainEvent::StreamCreated {
        session_id,
        stream_id,
        timestamp: Utc::now(),
    };

    assert_eq!(event.session_id(), session_id);
    assert_eq!(event.stream_id(), Some(stream_id));
}

#[tokio::test]
async fn test_session_event_no_stream_id() {
    let session_id = SessionId::new();

    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: Utc::now(),
    };

    assert_eq!(event.session_id(), session_id);
    assert_eq!(event.stream_id(), None);
}
