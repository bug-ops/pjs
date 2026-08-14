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

/// Capacity of the streaming channel returned by [`InMemoryEventPublisher::with_channel`].
///
/// Gives `publish`/`publish_batch` room to absorb bursts before a full
/// channel starts dropping events (logged via [`mpsc::error::TrySendError`]
/// rather than blocking the publish hot path — see [`EventPublisherGat`]'s
/// doc). 1000 is a conservative default chosen without a specific
/// throughput target; sizing it from an expected consumer lag or event
/// rate is tracked as a follow-up.
const EVENT_CHANNEL_CAPACITY: usize = 1000;

/// Maximum number of entries `event_log` is allowed to hold before
/// [`InMemoryEventPublisher::evict_oldest_if_over_capacity`] trims it back
/// down to [`EVENT_LOG_EVICT_TARGET`].
const EVENT_LOG_CAPACITY: usize = 10_000;

/// Entry count `event_log` is trimmed back down to once it exceeds
/// [`EVENT_LOG_CAPACITY`].
const EVENT_LOG_EVICT_TARGET: usize = 9_000;

/// In-memory event publisher with subscription support
pub struct InMemoryEventPublisher {
    /// Lock-free notification callbacks using DashMap
    notification_callbacks: Arc<DashMap<NotificationId, NotificationCallback>>,
    /// Lock-free event storage
    event_log: Arc<DashMap<EventId, StoredEvent>>,
    /// Next notification ID generator
    next_notification_id: Arc<AtomicU64>,
    /// Monotonic counter stamped onto [`StoredEvent::sequence`] at store time.
    ///
    /// `EventId` is a random UUIDv4 and `event_log`'s `DashMap` has no
    /// insertion order of its own, so this is the only reliable way to
    /// recover chronological order for eviction and [`Self::recent_events`].
    next_sequence: Arc<AtomicU64>,
    /// Optional channel for streaming events
    channel_tx: Arc<tokio::sync::RwLock<Option<mpsc::Sender<StoredEvent>>>>,
}

impl Clone for InMemoryEventPublisher {
    fn clone(&self) -> Self {
        Self {
            notification_callbacks: Arc::clone(&self.notification_callbacks),
            event_log: Arc::clone(&self.event_log),
            next_notification_id: Arc::clone(&self.next_notification_id),
            next_sequence: Arc::clone(&self.next_sequence),
            channel_tx: Arc::clone(&self.channel_tx),
        }
    }
}

/// Event recorded by [`InMemoryEventPublisher`] for inspection and replay.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    /// Surrogate key minted with [`EventId::new`] when this record is
    /// stored/published. `DomainEvent` itself carries no identity, so this
    /// id does not correlate across independently-stored copies of "the
    /// same" event (e.g. each destination in [`CompositeEventPublisher`]
    /// mints its own).
    pub id: EventId,
    /// Stringified event-type discriminant.
    pub event_type: String,
    /// Identifier of the session the event belongs to, when applicable.
    pub session_id: Option<SessionId>,
    /// Wall-clock instant the event occurred.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Free-form metadata captured from the event.
    pub metadata: std::collections::HashMap<String, String>,
    /// Monotonic sequence number stamped by [`InMemoryEventPublisher`] when
    /// this event is stored, used to recover chronological order for
    /// eviction and `recent_events()` since `event_log`'s `DashMap` and the
    /// random `EventId` key both lack one. Not `event_log`-insertion order
    /// under concurrency (the stamp is assigned before the entry is
    /// inserted), per-publisher-instance (restarts at 0 on every `new()`/
    /// `with_channel()`), not comparable across independent
    /// `InMemoryEventPublisher` instances (e.g. different
    /// [`CompositeEventPublisher`] destinations), and not an external
    /// correlation key.
    pub sequence: u64,
}

impl InMemoryEventPublisher {
    /// Create an empty publisher with no subscribers and no streaming channel.
    pub fn new() -> Self {
        Self {
            notification_callbacks: Arc::new(DashMap::new()),
            event_log: Arc::new(DashMap::new()),
            next_notification_id: Arc::new(AtomicU64::new(1)),
            next_sequence: Arc::new(AtomicU64::new(0)),
            channel_tx: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Initialize event streaming channel (lock-free)
    pub fn with_channel() -> (Self, mpsc::Receiver<StoredEvent>) {
        let (tx, rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let publisher = Self {
            notification_callbacks: Arc::new(DashMap::new()),
            event_log: Arc::new(DashMap::new()),
            next_notification_id: Arc::new(AtomicU64::new(1)),
            next_sequence: Arc::new(AtomicU64::new(0)),
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

    /// Evict the oldest entries once `event_log` exceeds
    /// [`EVENT_LOG_CAPACITY`], dropping back to [`EVENT_LOG_EVICT_TARGET`]
    /// by removing the lowest-`sequence` entries in a single pass — this
    /// must handle removing more than [`EVENT_LOG_CAPACITY`] -
    /// [`EVENT_LOG_EVICT_TARGET`] entries at once, since `publish_batch`
    /// can insert an arbitrarily large batch before calling this.
    fn evict_oldest_if_over_capacity(&self) {
        // `len()` here is just a cheap gate to skip collecting on the
        // common under-capacity path; it may be stale by the time
        // `by_sequence` is collected below under concurrent publishers.
        // `excess` MUST be derived from `by_sequence.len()` (the actual
        // snapshot being evicted), not this `len` — otherwise a
        // concurrent evictor shrinking the map between the two reads
        // makes `excess` overstate what's left, over-evicting (up to
        // wiping `event_log` entirely under enough concurrent pressure).
        if self.event_log.len() > EVENT_LOG_CAPACITY {
            let mut by_sequence: Vec<(EventId, u64)> = self
                .event_log
                .iter()
                .map(|entry| (*entry.key(), entry.value().sequence))
                .collect();
            by_sequence.sort_unstable_by_key(|(_, sequence)| *sequence);

            let excess = by_sequence.len().saturating_sub(EVENT_LOG_EVICT_TARGET);
            for (key, _) in by_sequence.into_iter().take(excess) {
                self.event_log.remove(&key);
            }
        }
    }

    /// Get the `limit` most recently published events, newest first.
    ///
    /// Ordered by [`StoredEvent::sequence`], not `DashMap` iteration order.
    pub fn recent_events(&self, limit: usize) -> Vec<StoredEvent> {
        let mut events: Vec<StoredEvent> = self
            .event_log
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        events.sort_unstable_by_key(|event| std::cmp::Reverse(event.sequence));
        events.truncate(limit);
        events
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
                id: EventId::new(),
                event_type: event.event_type().to_string(),
                session_id: Some(event.session_id()),
                timestamp: event.occurred_at(),
                metadata: event.metadata(),
                sequence: self.next_sequence.fetch_add(1, Ordering::Relaxed),
            };

            // Store event in lock-free map (EventId is Copy)
            let event_id = stored_event.id;
            self.event_log.insert(event_id, stored_event.clone());

            self.evict_oldest_if_over_capacity();

            // Send to channel if configured. `try_send` keeps this hot path
            // non-blocking: a stalled consumer must not stall unrelated
            // event publishing, so a full channel drops the event and logs
            // rather than awaiting capacity.
            if let Some(tx) = self.channel_tx.read().await.as_ref()
                && let Err(e) = tx.try_send(stored_event)
            {
                tracing::warn!("Dropping event from streaming channel: {e}");
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
            // Reserve a contiguous sequence block up front: stamping via a
            // per-event `fetch_add` inside `into_par_iter` would assign
            // sequences in rayon's thread-scheduling order rather than
            // batch order, scrambling the ordering `sequence` exists to
            // provide.
            let base_sequence = self
                .next_sequence
                .fetch_add(events.len() as u64, Ordering::Relaxed);

            // Process events in parallel for maximum performance
            let stored_events: Vec<_> = events
                .into_par_iter()
                .enumerate()
                .map(|(i, event)| {
                    let stored_event = StoredEvent {
                        id: EventId::new(),
                        event_type: event.event_type().to_string(),
                        session_id: Some(event.session_id()),
                        timestamp: event.occurred_at(),
                        metadata: event.metadata(),
                        sequence: base_sequence + i as u64,
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

            // Send to channel if configured (sequential for channel ordering).
            // Same non-blocking `try_send` policy as `publish`: drop and log
            // on a full channel instead of stalling the batch.
            if let Some(tx) = self.channel_tx.read().await.as_ref() {
                for stored_event in stored_events {
                    if let Err(e) = tx.try_send(stored_event) {
                        tracing::warn!("Dropping event from streaming channel: {e}");
                    }
                }
            }

            self.evict_oldest_if_over_capacity();

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
    /// Build a publisher that POSTs serialized events to `endpoint` with three retries.
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
            retry_attempts: 3,
        }
    }

    /// Override the maximum number of retry attempts before giving up.
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
                "event_id": EventId::new().to_string(),
                "event_type": event.event_type(),
                "session_id": event.session_id().to_string(),
                "occurred_at": event.occurred_at(),
                "metadata": event.metadata()
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
                        "event_id": EventId::new().to_string(),
                        "event_type": event.event_type(),
                        "session_id": event.session_id().to_string(),
                        "occurred_at": event.occurred_at(),
                        "metadata": event.metadata()
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
    /// Variant backed by [`InMemoryEventPublisher`].
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
    /// Build an empty composite publisher with no destinations.
    pub fn new() -> Self {
        Self {
            publishers: Vec::new(),
            fail_fast: false,
        }
    }

    /// Append `publisher` to the list of destinations.
    pub fn add_publisher(mut self, publisher: EventPublisherVariant) -> Self {
        self.publishers.push(publisher);
        self
    }

    /// When `enabled`, the first failing destination short-circuits the publish.
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
        events::DomainEvent,
        value_objects::{SessionId, StreamId},
    };

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
    async fn test_structurally_identical_events_do_not_collide() {
        // Regression test for #328: EventId used to be derived from the
        // event's `Debug` content hash, so two structurally-identical
        // events (same variant, session_id, and timestamp) produced the
        // same EventId and silently overwrote each other in `event_log`.
        let publisher = InMemoryEventPublisher::new();
        let session_id = SessionId::new();
        let timestamp = chrono::Utc::now();

        let event1 = DomainEvent::SessionActivated {
            session_id,
            timestamp,
        };
        let event2 = DomainEvent::SessionActivated {
            session_id,
            timestamp,
        };
        assert_eq!(
            format!("{event1:?}"),
            format!("{event2:?}"),
            "events must be structurally identical for this regression test to be meaningful"
        );

        publisher.publish(event1).await.unwrap();
        publisher.publish(event2).await.unwrap();

        assert_eq!(
            publisher.event_count(),
            2,
            "both events must be stored under distinct EventIds, not overwrite each other"
        );

        let stored = publisher.events_for_session(session_id);
        assert_eq!(stored.len(), 2);
        assert_ne!(
            stored[0].id, stored[1].id,
            "structurally identical events must still get distinct EventIds"
        );
    }

    #[tokio::test]
    async fn test_streaming_channel_is_bounded() {
        // Regression test for #314: `with_channel` used to return an
        // unbounded channel. It must now be capped at `EVENT_CHANNEL_CAPACITY`.
        let (publisher, _rx) = InMemoryEventPublisher::with_channel();
        let tx = publisher
            .channel_tx
            .read()
            .await
            .clone()
            .expect("with_channel must configure a sender");
        assert_eq!(tx.max_capacity(), EVENT_CHANNEL_CAPACITY);
    }

    #[tokio::test]
    async fn test_publish_does_not_block_when_streaming_channel_is_full() {
        // Regression test for #314: once the streaming channel is full,
        // `publish` must drop-and-log via `try_send` rather than blocking
        // on a stalled consumer. This asserts the drop actually happens
        // (not just that `publish` returns quickly): `event_log` holds
        // every event regardless, so the channel itself must be checked
        // directly to prove the overflow event was dropped rather than
        // queued.
        let (publisher, mut rx) = InMemoryEventPublisher::with_channel();
        let session_id = SessionId::new();

        for _ in 0..EVENT_CHANNEL_CAPACITY {
            let event = DomainEvent::SessionActivated {
                session_id,
                timestamp: chrono::Utc::now(),
            };
            publisher.publish(event).await.unwrap();
        }

        // The channel must now be completely full.
        assert_eq!(
            publisher
                .channel_tx
                .read()
                .await
                .as_ref()
                .expect("with_channel must configure a sender")
                .capacity(),
            0,
            "channel should be at capacity after publishing EVENT_CHANNEL_CAPACITY events"
        );

        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: chrono::Utc::now(),
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), publisher.publish(event))
            .await
            .expect("publish must not block when the streaming channel is full")
            .unwrap();

        // event_log stores every event regardless of channel state.
        assert_eq!(publisher.event_count(), EVENT_CHANNEL_CAPACITY + 1);

        // But the channel itself must hold only the events that fit before
        // it filled up: the overflow event was dropped, not queued.
        rx.close();
        let mut drained = 0;
        while rx.try_recv().is_ok() {
            drained += 1;
        }
        assert_eq!(
            drained, EVENT_CHANNEL_CAPACITY,
            "the overflow event must have been dropped from the channel, not queued"
        );
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
