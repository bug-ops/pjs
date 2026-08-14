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

#[tokio::test]
async fn test_recent_events_returns_true_newest_first_order() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    for _ in 0..20 {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        publisher.publish(event).await.unwrap();
    }

    // Sequential publish assigns sequence 0..20 in order, so recent_events
    // must return them in strictly descending sequence order (19, 18, ..., 0),
    // not merely the right count/limit.
    let sequences: Vec<u64> = publisher
        .recent_events(20)
        .iter()
        .map(|e| e.sequence)
        .collect();
    let expected: Vec<u64> = (0..20).rev().collect();
    assert_eq!(sequences, expected);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_eviction_stays_bounded_near_target() {
    let publisher = Arc::new(InMemoryEventPublisher::new());
    let session_id = SessionId::new();

    // `evict_oldest_if_over_capacity` has no internal `.await`, so on the
    // default single-threaded test runtime two calls could never truly
    // overlap — this needs a real multi-thread runtime for multiple tasks'
    // eviction passes to race on separate OS threads (the exact condition
    // S4 required: a stale `len()` read racing a concurrent evictor's
    // already-reduced snapshot). Many tasks each publishing large batches
    // maximizes the race window, per the critic's finding that it widens
    // under load and with big batches.
    let task_count = 16;
    let batches_per_task = 5;
    let batch_size = 3000usize;

    let mut handles = Vec::new();
    for _ in 0..task_count {
        let publisher = Arc::clone(&publisher);
        handles.push(tokio::spawn(async move {
            for _ in 0..batches_per_task {
                let events: Vec<DomainEvent> = (0..batch_size)
                    .map(|_| DomainEvent::SessionActivated {
                        session_id,
                        timestamp: chrono::Utc::now(),
                    })
                    .collect();
                publisher.publish_batch(events).await.unwrap();
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Under the pre-S4-fix bug, concurrent eviction passes could apply a
    // stale (too-large) removal count to an already-shrunk snapshot,
    // repeatedly over-evicting until event_log was wiped to 0 (critic
    // reproduced exactly 0 remaining at 20,000 and 109,000 inserts under
    // forced interleaving, versus the correct ~9,000). A generous lower
    // bound (well above 0, comfortably below the ~9,000 correct-code
    // should converge to) distinguishes a real regression from ordinary
    // race noise between near-identical concurrent snapshots.
    let count = publisher.event_count();
    assert!(
        count > 0,
        "event_log collapsed to empty under concurrent eviction (S4 regression)"
    );
    assert!(
        count >= 5_000,
        "event_log dropped far below EVENT_LOG_EVICT_TARGET (9000) under concurrent eviction, got {count} (S4 regression)"
    );
    assert!(
        count <= 10_000,
        "event_log exceeded EVENT_LOG_CAPACITY (10000), got {count}"
    );
}

#[tokio::test]
async fn test_eviction_removes_oldest_by_sequence_not_arbitrary_entries() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();
    let total: u64 = 11_000;

    for _ in 0..total {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        publisher.publish(event).await.unwrap();
    }

    let count = publisher.event_count() as u64;
    assert!(count <= 10000);

    // Sequential single-event publish assigns sequence i to the i-th
    // publish, so the retained set after any number of evictions must be
    // exactly the newest `count` sequences: [total - count, total - 1].
    // Any gap or out-of-range sequence would mean eviction dropped
    // something other than the true oldest entries.
    let mut sequences: Vec<u64> = publisher
        .recent_events(count as usize)
        .iter()
        .map(|e| e.sequence)
        .collect();
    sequences.sort_unstable();

    let expected = (total - count)..total;
    assert!(sequences.iter().copied().eq(expected));
}

#[tokio::test]
async fn test_eviction_boundary_at_capacity_does_not_evict() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    for _ in 0..10_000 {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        publisher.publish(event).await.unwrap();
    }

    // Exactly at capacity: no eviction should have run, so the very first
    // published event (sequence 0) must still be present.
    assert_eq!(publisher.event_count(), 10_000);
    let has_oldest = publisher
        .recent_events(10_000)
        .iter()
        .any(|e| e.sequence == 0);
    assert!(
        has_oldest,
        "sequence 0 should not have been evicted at exactly capacity"
    );
}

#[tokio::test]
async fn test_eviction_boundary_one_over_capacity_evicts_down_to_target() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();

    for _ in 0..10_001 {
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        publisher.publish(event).await.unwrap();
    }

    // One over capacity (10_001 > EVENT_LOG_CAPACITY) triggers a single
    // eviction of the 1001 lowest sequences (0..=1000) — not a fixed 1000 —
    // to reach exactly EVENT_LOG_EVICT_TARGET (9000) entries, starting at
    // sequence 1001.
    assert_eq!(publisher.event_count(), 9_000);
    let mut sequences: Vec<u64> = publisher
        .recent_events(9_000)
        .iter()
        .map(|e| e.sequence)
        .collect();
    sequences.sort_unstable();
    let expected = 1001..10_001;
    assert!(sequences.iter().copied().eq(expected));
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
async fn test_publish_batch_stamps_sequence_in_input_order() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();
    let batch_size = 1000usize;

    // `publish_batch` stamps `sequence` from inside a rayon `into_par_iter`,
    // so thread-scheduling order could scramble it relative to input order.
    // Split the batch into a first-half/second-half type boundary so any
    // scrambling shows up as `session_closed` entries with a lower sequence
    // than some `session_activated` entry, instead of relying on identical
    // events where mis-ordering would be invisible.
    let events: Vec<DomainEvent> = (0..batch_size)
        .map(|i| {
            if i < batch_size / 2 {
                DomainEvent::SessionActivated {
                    session_id,
                    timestamp: chrono::Utc::now(),
                }
            } else {
                DomainEvent::SessionClosed {
                    session_id,
                    timestamp: chrono::Utc::now(),
                }
            }
        })
        .collect();

    publisher.publish_batch(events).await.unwrap();

    let mut stored = publisher.recent_events(batch_size);
    assert_eq!(stored.len(), batch_size);
    stored.sort_unstable_by_key(|e| e.sequence);

    // The reserved sequence block must be contiguous and gap-free.
    let sequences: Vec<u64> = stored.iter().map(|e| e.sequence).collect();
    assert!(sequences.iter().copied().eq(0..batch_size as u64));

    // Sequence order must match input order: all `session_activated`
    // entries (input indices 0..500) sort before all `session_closed`
    // entries (input indices 500..1000), with no interleaving.
    assert!(
        stored[..batch_size / 2]
            .iter()
            .all(|e| e.event_type == "session_activated")
    );
    assert!(
        stored[batch_size / 2..]
            .iter()
            .all(|e| e.event_type == "session_closed")
    );
}

#[tokio::test]
async fn test_publish_batch_over_capacity_evicts_down_to_target_in_one_pass() {
    let publisher = InMemoryEventPublisher::new();
    let session_id = SessionId::new();
    let batch_size = 12_000usize;

    // A single `publish_batch` call inserting more than EVENT_LOG_CAPACITY
    // over the cap in one go must still bring `event_log` down to exactly
    // EVENT_LOG_EVICT_TARGET (9000), not just under EVENT_LOG_CAPACITY —
    // `evict_oldest_if_over_capacity` has to remove more than the old
    // hardcoded 1000-per-call amount to do so.
    let events: Vec<DomainEvent> = (0..batch_size)
        .map(|_| DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        })
        .collect();

    publisher.publish_batch(events).await.unwrap();

    assert_eq!(publisher.event_count(), 9_000);

    // The retained entries must be the newest 9000 sequences, i.e. the
    // oldest 3000 (0..=2999) were evicted, not an arbitrary subset.
    let mut sequences: Vec<u64> = publisher
        .recent_events(9_000)
        .iter()
        .map(|e| e.sequence)
        .collect();
    sequences.sort_unstable();
    let expected = (batch_size as u64 - 9_000)..batch_size as u64;
    assert!(sequences.iter().copied().eq(expected));
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
