//! Event publisher implementation for PJS domain events

// async_trait removed - using GAT traits with lock-free concurrency
use dashmap::DashMap;
use rayon::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::mpsc;

use crate::domain::{
    DomainResult,
    events::{DomainEvent, EventId},
    ports::EventPublisherGat,
    value_objects::SessionId,
};

/// Lock-free notification system using DashMap for maximum concurrency
type NotificationId = u64;
type NotificationCallback = Arc<dyn Fn(&DomainEvent) + Send + Sync>;

/// In-memory event publisher with subscription support
pub struct InMemoryEventPublisher {
    /// Lock-free notification callbacks using DashMap
    notification_callbacks: Arc<DashMap<NotificationId, NotificationCallback>>,
    /// Lock-free event storage
    event_log: Arc<DashMap<EventId, StoredEvent>>,
    /// Next notification ID generator
    next_notification_id: Arc<AtomicU64>,
    /// Optional channel for streaming events
    channel_tx: Arc<tokio::sync::RwLock<Option<mpsc::UnboundedSender<StoredEvent>>>>,
}

impl Clone for InMemoryEventPublisher {
    fn clone(&self) -> Self {
        Self {
            notification_callbacks: Arc::clone(&self.notification_callbacks),
            event_log: Arc::clone(&self.event_log),
            next_notification_id: Arc::clone(&self.next_notification_id),
            channel_tx: Arc::clone(&self.channel_tx),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub id: EventId,
    pub event_type: String,
    pub session_id: Option<SessionId>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub payload: serde_json::Value,
}

impl InMemoryEventPublisher {
    pub fn new() -> Self {
        Self {
            notification_callbacks: Arc::new(DashMap::new()),
            event_log: Arc::new(DashMap::new()),
            next_notification_id: Arc::new(AtomicU64::new(1)),
            channel_tx: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Initialize event streaming channel (lock-free)
    pub fn with_channel() -> (Self, mpsc::UnboundedReceiver<StoredEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let publisher = Self {
            notification_callbacks: Arc::new(DashMap::new()),
            event_log: Arc::new(DashMap::new()),
            next_notification_id: Arc::new(AtomicU64::new(1)),
            channel_tx: Arc::new(tokio::sync::RwLock::new(Some(tx))),
        };
        (publisher, rx)
    }

    /// Add notification callback (lock-free)
    pub fn add_notification_callback<F>(&self, callback: F) -> NotificationId
    where
        F: Fn(&DomainEvent) + Send + Sync + 'static,
    {
        let id = self.next_notification_id.fetch_add(1, Ordering::Relaxed);
        self.notification_callbacks.insert(id, Arc::new(callback));
        id
    }

    /// Remove notification callback (lock-free)
    pub fn remove_notification_callback(&self, id: NotificationId) -> Option<NotificationCallback> {
        self.notification_callbacks
            .remove(&id)
            .map(|(_, callback)| callback)
    }

    /// Get event count for testing (lock-free)
    pub fn event_count(&self) -> usize {
        self.event_log.len()
    }

    /// Get events by type (lock-free)
    pub fn events_by_type(&self, event_type: &str) -> Vec<StoredEvent> {
        self.event_log
            .iter()
            .filter(|entry| entry.value().event_type == event_type)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get events for session (lock-free)
    pub fn events_for_session(&self, session_id: SessionId) -> Vec<StoredEvent> {
        self.event_log
            .iter()
            .filter(|entry| entry.value().session_id == Some(session_id))
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Clear all events (for testing, lock-free)
    pub fn clear(&self) {
        self.event_log.clear();
    }

    /// Get recent events (lock-free, returns up to limit events)  
    pub fn recent_events(&self, limit: usize) -> Vec<StoredEvent> {
        let mut events: Vec<StoredEvent> = self
            .event_log
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        // Reverse to get most recent first, then take limit
        events.reverse();
        events.into_iter().take(limit).collect()
    }
}

impl std::fmt::Debug for InMemoryEventPublisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryEventPublisher")
            .field("async_fields", &"<async RwLock>")
            .finish()
    }
}

impl Default for InMemoryEventPublisher {
    fn default() -> Self {
        Self::new()
    }
}

impl EventPublisherGat for InMemoryEventPublisher {
    type PublishFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type PublishBatchFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    fn publish(&self, event: DomainEvent) -> Self::PublishFuture<'_> {
        async move {
            let stored_event = StoredEvent {
                id: event.event_id(),
                event_type: event.event_type().to_string(),
                session_id: Some(event.session_id()),
                timestamp: event.occurred_at(),
                payload: event.payload().clone(),
            };

            // Store event in lock-free map (EventId is Copy)
            let event_id = stored_event.id;
            self.event_log.insert(event_id, stored_event.clone());

            // Memory management: keep only recent events (lock-free check)
            if self.event_log.len() > 10000 {
                // Remove oldest events by clearing some entries
                // Note: DashMap doesn't maintain insertion order, so we approximate
                let to_remove: Vec<_> = self
                    .event_log
                    .iter()
                    .take(1000) // Remove first 1000 found entries
                    .map(|entry| entry.key().clone())
                    .collect();

                for key in to_remove {
                    self.event_log.remove(&key);
                }
            }

            // Send to channel if configured (lock-free check)
            if let Some(tx) = self.channel_tx.read().await.as_ref() {
                let _ = tx.send(stored_event);
            }

            // Notify callbacks (lock-free iteration)
            self.notification_callbacks.iter().for_each(|entry| {
                let callback = entry.value();
                callback(&event);
            });

            Ok(())
        }
    }

    fn publish_batch(&self, events: Vec<DomainEvent>) -> Self::PublishBatchFuture<'_> {
        async move {
            // Process events in parallel for maximum performance
            let stored_events: Vec<_> = events
                .into_par_iter()
                .map(|event| {
                    let stored_event = StoredEvent {
                        id: event.event_id(),
                        event_type: event.event_type().to_string(),
                        session_id: Some(event.session_id()),
                        timestamp: event.occurred_at(),
                        payload: event.payload().clone(),
                    };

                    // Store event in lock-free map (EventId is Copy)
                    let event_id = stored_event.id;
                    self.event_log.insert(event_id, stored_event.clone());

                    // Notify callbacks (sequential for stability)
                    self.notification_callbacks.iter().for_each(|entry| {
                        let callback = entry.value();
                        callback(&event);
                    });

                    stored_event
                })
                .collect();

            // Send to channel if configured (sequential for channel ordering)
            if let Some(tx) = self.channel_tx.read().await.as_ref() {
                for stored_event in stored_events {
                    let _ = tx.send(stored_event);
                }
            }

            // Memory management after batch
            if self.event_log.len() > 10000 {
                let to_remove: Vec<_> = self
                    .event_log
                    .iter()
                    .take(1000)
                    .map(|entry| entry.key().clone())
                    .collect();

                for key in to_remove {
                    self.event_log.remove(&key);
                }
            }

            Ok(())
        }
    }
}

/// HTTP-based event publisher for distributed systems
#[cfg(feature = "http-client")]
#[derive(Debug, Clone)]
pub struct HttpEventPublisher {
    endpoint: String,
    client: reqwest::Client,
    retry_attempts: usize,
}

#[cfg(feature = "http-client")]
impl HttpEventPublisher {
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
            retry_attempts: 3,
        }
    }

    pub fn with_retry_attempts(mut self, attempts: usize) -> Self {
        self.retry_attempts = attempts;
        self
    }
}

#[cfg(feature = "http-client")]
impl EventPublisherGat for HttpEventPublisher {
    type PublishFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type PublishBatchFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    fn publish(&self, event: DomainEvent) -> Self::PublishFuture<'_> {
        async move {
            let payload = serde_json::json!({
                "event_id": event.event_id().to_string(),
                "event_type": event.event_type(),
                "session_id": event.session_id().to_string(),
                "occurred_at": event.occurred_at(),
                "payload": event.payload()
            });

            for attempt in 0..self.retry_attempts {
                match self.client.post(&self.endpoint).json(&payload).send().await {
                    Ok(response) if response.status().is_success() => return Ok(()),
                    Ok(response) => {
                        eprintln!(
                            "HTTP event publish failed with status: {}",
                            response.status()
                        );
                        if attempt == self.retry_attempts - 1 {
                            return Err(
                                format!("HTTP publish failed: {}", response.status()).into()
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("HTTP event publish error (attempt {}): {}", attempt + 1, e);
                        if attempt == self.retry_attempts - 1 {
                            return Err(format!("HTTP publish error: {e}").into());
                        }
                    }
                }

                // Exponential backoff
                tokio::time::sleep(std::time::Duration::from_millis(100 << attempt)).await;
            }

            Ok(())
        }
    }

    fn publish_batch(&self, events: Vec<DomainEvent>) -> Self::PublishBatchFuture<'_> {
        async move {
            let batch_payload: Vec<_> = events
                .iter()
                .map(|event| {
                    serde_json::json!({
                        "event_id": event.event_id().to_string(),
                        "event_type": event.event_type(),
                        "session_id": event.session_id().to_string(),
                        "occurred_at": event.occurred_at(),
                        "payload": event.payload()
                    })
                })
                .collect();

            for attempt in 0..self.retry_attempts {
                match self
                    .client
                    .post(format!("{}/batch", self.endpoint))
                    .json(&batch_payload)
                    .send()
                    .await
                {
                    Ok(response) if response.status().is_success() => return Ok(()),
                    Ok(response) => {
                        eprintln!(
                            "HTTP batch publish failed with status: {}",
                            response.status()
                        );
                        if attempt == self.retry_attempts - 1 {
                            return Err(format!(
                                "HTTP batch publish failed: {}",
                                response.status()
                            )
                            .into());
                        }
                    }
                    Err(e) => {
                        eprintln!("HTTP batch publish error (attempt {}): {}", attempt + 1, e);
                        if attempt == self.retry_attempts - 1 {
                            return Err(format!("HTTP batch publish error: {e}").into());
                        }
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(100 << attempt)).await;
            }

            Ok(())
        }
    }
}

/// Event publisher variants for composite pattern
#[derive(Clone)]
pub enum EventPublisherVariant {
    InMemory(InMemoryEventPublisher),
    // Add more variants as needed
    // Http(HttpEventPublisher),
}

impl EventPublisherGat for EventPublisherVariant {
    type PublishFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type PublishBatchFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    fn publish(&self, event: DomainEvent) -> Self::PublishFuture<'_> {
        async move {
            match self {
                EventPublisherVariant::InMemory(publisher) => publisher.publish(event).await,
            }
        }
    }

    fn publish_batch(&self, events: Vec<DomainEvent>) -> Self::PublishBatchFuture<'_> {
        async move {
            match self {
                EventPublisherVariant::InMemory(publisher) => publisher.publish_batch(events).await,
            }
        }
    }
}

/// Composite event publisher that sends to multiple destinations
#[derive(Clone)]
pub struct CompositeEventPublisher {
    publishers: Vec<EventPublisherVariant>,
    fail_fast: bool,
}

impl CompositeEventPublisher {
    pub fn new() -> Self {
        Self {
            publishers: Vec::new(),
            fail_fast: false,
        }
    }

    pub fn add_publisher(mut self, publisher: EventPublisherVariant) -> Self {
        self.publishers.push(publisher);
        self
    }

    pub fn with_fail_fast(mut self, enabled: bool) -> Self {
        self.fail_fast = enabled;
        self
    }
}

impl Default for CompositeEventPublisher {
    fn default() -> Self {
        Self::new()
    }
}

impl EventPublisherGat for CompositeEventPublisher {
    type PublishFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type PublishBatchFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    fn publish(&self, event: DomainEvent) -> Self::PublishFuture<'_> {
        async move {
            let mut errors = Vec::new();

            for publisher in &self.publishers {
                match publisher.publish(event.clone()).await {
                    Ok(()) => {}
                    Err(e) => {
                        errors.push(e);
                        if self.fail_fast {
                            return Err(errors.into_iter().next().unwrap());
                        }
                    }
                }
            }

            if errors.is_empty() {
                Ok(())
            } else {
                Err(format!("Multiple publish errors: {errors:?}").into())
            }
        }
    }

    fn publish_batch(&self, events: Vec<DomainEvent>) -> Self::PublishBatchFuture<'_> {
        async move {
            let mut errors = Vec::new();

            for publisher in &self.publishers {
                match publisher.publish_batch(events.clone()).await {
                    Ok(()) => {}
                    Err(e) => {
                        errors.push(e);
                        if self.fail_fast {
                            return Err(errors.into_iter().next().unwrap());
                        }
                    }
                }
            }

            if errors.is_empty() {
                Ok(())
            } else {
                Err(format!("Multiple batch publish errors: {errors:?}").into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        events::{DomainEvent, EventSubscriber},
        value_objects::{SessionId, StreamId},
    };
    use std::sync::RwLock;

    #[derive(Debug, Clone)]
    struct TestSubscriber {
        received_events: Arc<RwLock<Vec<DomainEvent>>>,
    }

    impl TestSubscriber {
        fn new() -> Self {
            Self {
                received_events: Arc::new(RwLock::new(Vec::new())),
            }
        }

        fn event_count(&self) -> usize {
            self.received_events.read().unwrap().len()
        }
    }

    impl EventSubscriber for TestSubscriber {
        type HandleFuture<'a>
            = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        fn handle(&self, event: &DomainEvent) -> Self::HandleFuture<'_> {
            let event = event.clone();
            async move {
                self.received_events.write().unwrap().push(event);
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_in_memory_event_publisher() {
        let publisher = InMemoryEventPublisher::new();

        // Add notification callback instead of subscriber
        let received_events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_clone = received_events.clone();

        publisher.add_notification_callback(move |event| {
            events_clone.lock().unwrap().push(event.clone());
        });

        let session_id = SessionId::new();
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };

        publisher.publish(event).await.unwrap();

        assert_eq!(publisher.event_count(), 1);
        assert_eq!(received_events.lock().unwrap().len(), 1);

        let events_for_session = publisher.events_for_session(session_id);
        assert_eq!(events_for_session.len(), 1);
    }

    #[tokio::test]
    async fn test_event_publisher_with_channel() {
        let (publisher, mut rx) = InMemoryEventPublisher::with_channel();

        let session_id = SessionId::new();
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };

        publisher.publish(event).await.unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.event_type, "session_activated");
        assert_eq!(received.session_id, Some(session_id));
    }

    #[tokio::test]
    async fn test_batch_publishing() {
        let publisher = InMemoryEventPublisher::new();
        let session_id = SessionId::new();
        let stream_id = StreamId::new();

        let events = vec![
            DomainEvent::SessionActivated {
                session_id,
                timestamp: chrono::Utc::now(),
            },
            DomainEvent::StreamStarted {
                session_id,
                stream_id,
                timestamp: chrono::Utc::now(),
            },
        ];

        publisher.publish_batch(events).await.unwrap();

        assert_eq!(publisher.event_count(), 2);
    }

    #[tokio::test]
    async fn test_composite_publisher() {
        let publisher1 = InMemoryEventPublisher::new();
        let publisher2 = InMemoryEventPublisher::new();

        let composite = CompositeEventPublisher::new()
            .add_publisher(EventPublisherVariant::InMemory(publisher1.clone()))
            .add_publisher(EventPublisherVariant::InMemory(publisher2.clone()));

        let session_id = SessionId::new();
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };

        composite.publish(event).await.unwrap();

        assert_eq!(publisher1.event_count(), 1);
        assert_eq!(publisher2.event_count(), 1);
    }
}
