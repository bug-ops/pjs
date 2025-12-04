//! Comprehensive tests for InMemoryEventPublisher
//!
//! Coverage targets:
//! - Event publishing
//! - Subscriber management (notification callbacks)
//! - Event filtering and retrieval
//! - Lock-free concurrent access
//! - Channel-based event streaming

use pjson_rs::{
    domain::{events::DomainEvent, ports::EventPublisherGat, value_objects::SessionId},
    infrastructure::adapters::event_publisher::InMemoryEventPublisher,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::timeout;

// ============================================================================
// Publisher Creation
// ============================================================================

#[test]
fn test_new_publisher_empty() {
    let publisher = InMemoryEventPublisher::new();

    assert_eq!(publisher.event_count(), 0);
}

#[test]
fn test_default_publisher_empty() {
    let publisher = InMemoryEventPublisher::default();

    assert_eq!(publisher.event_count(), 0);
}

#[test]
fn test_with_channel_creates_publisher_and_receiver() {
    let (publisher, _rx) = InMemoryEventPublisher::with_channel();

    assert_eq!(publisher.event_count(), 0);
}

// ============================================================================
// Event Publishing
// ============================================================================

#[tokio::test]
async fn test_publish_single_event() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event).await.unwrap();

    assert_eq!(publisher.event_count(), 1);
}

#[tokio::test]
async fn test_publish_multiple_events() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    for _ in 0..5 {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        publisher.publish(event).await.unwrap();
    }

    assert_eq!(publisher.event_count(), 5);
}

#[tokio::test]
async fn test_publish_different_event_types() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    let event1 = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    let event2 = DomainEvent::SessionClosed {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event1).await.unwrap();
    publisher.publish(event2).await.unwrap();

    assert_eq!(publisher.event_count(), 2);
}

// ============================================================================
// Event Retrieval
// ============================================================================

#[tokio::test]
async fn test_events_by_type_empty() {
    let publisher = InMemoryEventPublisher::new();

    let events = publisher.events_by_type("session_activated");

    assert_eq!(events.len(), 0);
}

#[tokio::test]
async fn test_events_by_type_filtering() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    let event1 = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    let event2 = DomainEvent::SessionClosed {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    let event3 = DomainEvent::SessionActivated {
        session_id: SessionId::new(),
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event1).await.unwrap();
    publisher.publish(event2).await.unwrap();
    publisher.publish(event3).await.unwrap();

    let activated_events = publisher.events_by_type("session_activated");

    assert_eq!(activated_events.len(), 2);
    assert!(
        activated_events
            .iter()
            .all(|e| e.event_type == "session_activated")
    );
}

#[tokio::test]
async fn test_events_for_session_empty() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    let events = publisher.events_for_session(session_id);

    assert_eq!(events.len(), 0);
}

#[tokio::test]
async fn test_events_for_session_filtering() {
    let publisher = InMemoryEventPublisher::new();
    let session1 = SessionId::new();
    let session2 = SessionId::new();

    let event1 = DomainEvent::SessionActivated {
        session_id: session1,
        timestamp: chrono::Utc::now(),
    };

    let event2 = DomainEvent::SessionActivated {
        session_id: session2,
        timestamp: chrono::Utc::now(),
    };

    let event3 = DomainEvent::SessionClosed {
        session_id: session1,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event1).await.unwrap();
    publisher.publish(event2).await.unwrap();
    publisher.publish(event3).await.unwrap();

    let session1_events = publisher.events_for_session(session1);

    assert_eq!(session1_events.len(), 2);
    assert!(
        session1_events
            .iter()
            .all(|e| e.session_id == Some(session1))
    );
}

#[tokio::test]
async fn test_recent_events_empty() {
    let publisher = InMemoryEventPublisher::new();

    let events = publisher.recent_events(10);

    assert_eq!(events.len(), 0);
}

#[tokio::test]
async fn test_recent_events_respects_limit() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    for _ in 0..10 {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        publisher.publish(event).await.unwrap();
    }

    let recent = publisher.recent_events(5);

    assert_eq!(recent.len(), 5);
}

#[tokio::test]
async fn test_recent_events_all_when_under_limit() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    for _ in 0..3 {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        publisher.publish(event).await.unwrap();
    }

    let recent = publisher.recent_events(10);

    assert_eq!(recent.len(), 3);
}

// ============================================================================
// Clear Operations
// ============================================================================

#[tokio::test]
async fn test_clear_events() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    for _ in 0..5 {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        publisher.publish(event).await.unwrap();
    }

    assert_eq!(publisher.event_count(), 5);

    publisher.clear();

    assert_eq!(publisher.event_count(), 0);
}

#[tokio::test]
async fn test_clear_empty_publisher() {
    let publisher = InMemoryEventPublisher::new();

    publisher.clear();

    assert_eq!(publisher.event_count(), 0);
}

// ============================================================================
// Notification Callbacks
// ============================================================================

#[tokio::test]
async fn test_add_notification_callback() {
    let publisher = InMemoryEventPublisher::new();
    let called = Arc::new(Mutex::new(false));
    let called_clone = Arc::clone(&called);

    let callback_id = publisher.add_notification_callback(move |_event| {
        let mut called = called_clone.lock().unwrap();
        *called = true;
    });

    assert!(callback_id > 0);

    let session_id = SessionId::new();
    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event).await.unwrap();

    // Give callback time to execute
    tokio::time::sleep(Duration::from_millis(10)).await;

    let was_called = *called.lock().unwrap();
    assert!(was_called);
}

#[tokio::test]
async fn test_multiple_notification_callbacks() {
    let publisher = InMemoryEventPublisher::new();
    let counter = Arc::new(Mutex::new(0));

    let counter_clone1 = Arc::clone(&counter);
    let counter_clone2 = Arc::clone(&counter);

    let _id1 = publisher.add_notification_callback(move |_event| {
        let mut count = counter_clone1.lock().unwrap();
        *count += 1;
    });

    let _id2 = publisher.add_notification_callback(move |_event| {
        let mut count = counter_clone2.lock().unwrap();
        *count += 10;
    });

    let session_id = SessionId::new();
    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event).await.unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;

    let final_count = *counter.lock().unwrap();
    assert_eq!(final_count, 11); // 1 + 10
}

#[tokio::test]
async fn test_remove_notification_callback() {
    let publisher = InMemoryEventPublisher::new();
    let called = Arc::new(Mutex::new(false));
    let called_clone = Arc::clone(&called);

    let callback_id = publisher.add_notification_callback(move |_event| {
        let mut called = called_clone.lock().unwrap();
        *called = true;
    });

    let removed = publisher.remove_notification_callback(callback_id);
    assert!(removed.is_some());

    let session_id = SessionId::new();
    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event).await.unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;

    let was_called = *called.lock().unwrap();
    assert!(!was_called); // Should not be called after removal
}

#[test]
fn test_remove_nonexistent_callback() {
    let publisher = InMemoryEventPublisher::new();

    let removed = publisher.remove_notification_callback(9999);

    assert!(removed.is_none());
}

#[tokio::test]
async fn test_callback_receives_correct_event_data() {
    let publisher = InMemoryEventPublisher::new();
    let received_session_id = Arc::new(Mutex::new(None));
    let received_clone = Arc::clone(&received_session_id);

    let _callback_id = publisher.add_notification_callback(move |event| {
        let mut stored = received_clone.lock().unwrap();
        *stored = Some(event.session_id());
    });

    let session_id = SessionId::new();
    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event).await.unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;

    let received = received_session_id.lock().unwrap();
    assert_eq!(*received, Some(session_id));
}

// ============================================================================
// Channel-based Event Streaming
// ============================================================================

#[tokio::test]
async fn test_channel_receives_events() {
    let (publisher, mut rx) = InMemoryEventPublisher::with_channel();
    let session_id = SessionId::new();

    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event).await.unwrap();

    let received = timeout(Duration::from_millis(100), rx.recv()).await;

    assert!(received.is_ok());
    let stored_event = received.unwrap();
    assert!(stored_event.is_some());
    assert_eq!(stored_event.unwrap().event_type, "session_activated");
}

#[tokio::test]
async fn test_channel_receives_multiple_events() {
    let (publisher, mut rx) = InMemoryEventPublisher::with_channel();
    let session_id = SessionId::new();

    for _ in 0..3 {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        publisher.publish(event).await.unwrap();
    }

    let mut count = 0;
    while let Ok(Some(_)) = timeout(Duration::from_millis(50), rx.recv()).await {
        count += 1;
        if count >= 3 {
            break;
        }
    }

    assert_eq!(count, 3);
}

#[tokio::test]
async fn test_channel_preserves_event_order() {
    let (publisher, mut rx) = InMemoryEventPublisher::with_channel();
    let session_id = SessionId::new();

    let event1 = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    let event2 = DomainEvent::SessionClosed {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event1).await.unwrap();
    publisher.publish(event2).await.unwrap();

    let first = timeout(Duration::from_millis(50), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let second = timeout(Duration::from_millis(50), rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(first.event_type, "session_activated");
    assert_eq!(second.event_type, "session_closed");
}

// ============================================================================
// Concurrent Access
// ============================================================================

#[tokio::test]
async fn test_concurrent_publishing() {
    let publisher = Arc::new(InMemoryEventPublisher::new());
    let mut handles = vec![];

    for _ in 0..10 {
        let pub_clone = Arc::clone(&publisher);
        let handle = tokio::spawn(async move {
            let session_id = SessionId::new();
            let event = DomainEvent::SessionActivated {
                session_id,
                timestamp: chrono::Utc::now(),
            };
            pub_clone.publish(event).await.unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(publisher.event_count(), 10);
}

#[tokio::test]
async fn test_concurrent_read_write() {
    let publisher = Arc::new(InMemoryEventPublisher::new());
    let mut handles = vec![];

    // Writers
    for _ in 0..5 {
        let pub_clone = Arc::clone(&publisher);
        let handle = tokio::spawn(async move {
            let session_id = SessionId::new();
            let event = DomainEvent::SessionActivated {
                session_id,
                timestamp: chrono::Utc::now(),
            };
            pub_clone.publish(event).await.unwrap();
        });
        handles.push(handle);
    }

    // Readers
    for _ in 0..5 {
        let pub_clone = Arc::clone(&publisher);
        let handle = tokio::spawn(async move {
            let _count = pub_clone.event_count();
            let _recent = pub_clone.recent_events(10);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(publisher.event_count(), 5);
}

#[tokio::test]
async fn test_concurrent_callbacks() {
    let publisher = Arc::new(InMemoryEventPublisher::new());
    let counter = Arc::new(Mutex::new(0));
    let mut handles = vec![];

    // Add multiple callbacks concurrently
    for _ in 0..5 {
        let pub_clone = Arc::clone(&publisher);
        let counter_clone = Arc::clone(&counter);
        let handle = tokio::spawn(async move {
            pub_clone.add_notification_callback(move |_event| {
                let mut count = counter_clone.lock().unwrap();
                *count += 1;
            });
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Publish one event - should trigger all 5 callbacks
    let session_id = SessionId::new();
    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let final_count = *counter.lock().unwrap();
    assert_eq!(final_count, 5);
}

// ============================================================================
// Memory Management
// ============================================================================

#[tokio::test]
async fn test_memory_management_evicts_old_events() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    // Publish more than 10000 events to trigger cleanup
    for _ in 0..10500 {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        publisher.publish(event).await.unwrap();
    }

    // Should have cleaned up some events
    assert!(publisher.event_count() <= 10000);
}

// ============================================================================
// Clone Implementation
// ============================================================================

#[tokio::test]
async fn test_clone_shares_state() {
    let publisher1 = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher1.publish(event).await.unwrap();

    let publisher2 = publisher1.clone();

    // Both should see the same event count
    assert_eq!(publisher1.event_count(), 1);
    assert_eq!(publisher2.event_count(), 1);
}

#[tokio::test]
async fn test_clone_shared_callbacks() {
    let publisher1 = InMemoryEventPublisher::new();
    let called = Arc::new(Mutex::new(false));
    let called_clone = Arc::clone(&called);

    let _callback_id = publisher1.add_notification_callback(move |_event| {
        let mut called = called_clone.lock().unwrap();
        *called = true;
    });

    let publisher2 = publisher1.clone();

    let session_id = SessionId::new();
    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher2.publish(event).await.unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;

    let was_called = *called.lock().unwrap();
    assert!(was_called);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[tokio::test]
async fn test_publish_batch_future_exists() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    let events = vec![
        DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        },
        DomainEvent::SessionClosed {
            session_id,
            timestamp: chrono::Utc::now(),
        },
    ];

    // Test that publish_batch is available
    let result = publisher.publish_batch(events).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_events_preserve_metadata() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    let event = DomainEvent::SessionActivated {
        session_id,
        timestamp: chrono::Utc::now(),
    };

    publisher.publish(event).await.unwrap();

    let stored_events = publisher.events_by_type("session_activated");
    assert_eq!(stored_events.len(), 1);

    let stored = &stored_events[0];
    assert_eq!(stored.session_id, Some(session_id));
    assert_eq!(stored.event_type, "session_activated");
}
