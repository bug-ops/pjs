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
use crate::infrastructure::bounded_channel::{ByteBoundedSender, Envelope, byte_bounded_channel};

/// Lock-free notification system using DashMap for maximum concurrency
type NotificationId = u64;
type NotificationCallback = Arc<dyn Fn(&DomainEvent) + Send + Sync>;

/// Capacity of the streaming channel returned by [`InMemoryEventPublisher::with_channel`].
///
/// Gives `publish`/`publish_batch` room to absorb bursts before a full
/// channel starts dropping events (logged rather than blocking the publish
/// hot path — see [`EventPublisherGat`]'s doc). This is a message-count
/// bound only; [`MAX_QUEUED_EVENT_BYTES`] additionally bounds cumulative
/// queued bytes. 1000 is a conservative default chosen without a specific
/// throughput target; sizing it from an expected consumer lag or event
/// rate is tracked as a follow-up.
const EVENT_CHANNEL_CAPACITY: usize = 1000;

/// Cumulative byte budget for the streaming channel, on top of
/// [`EVENT_CHANNEL_CAPACITY`]'s message-count bound.
///
/// Keeps worst-case queued memory a small, predictable constant regardless
/// of individual event size (e.g. an event carrying large `metadata`).
/// Sized via [`StoredEvent::approx_byte_size`], not an exact serialized
/// size — see that method's doc for why.
const MAX_QUEUED_EVENT_BYTES: usize = 16 * 1024 * 1024;

/// Maximum number of entries `event_log` is allowed to hold before
/// [`InMemoryEventPublisher::evict_oldest_if_over_capacity`] trims it back
/// down to [`EVENT_LOG_EVICT_TARGET`].
const EVENT_LOG_CAPACITY: usize = 10_000;

/// Entry count `event_log` is trimmed back down to once it exceeds
/// [`EVENT_LOG_CAPACITY`].
const EVENT_LOG_EVICT_TARGET: usize = 9_000;

/// One-shot pause point armed inside [`InMemoryEventPublisher::evict_oldest_if_over_capacity`],
/// used by `tests::test_concurrent_eviction_forced_interleaving_stays_at_target`
/// to deterministically reproduce a stale-length-vs-fresh-snapshot
/// interleaving: `excess` sized from a `len()` read before a concurrent
/// evictor shrinks `event_log`, then applied to a snapshot taken after.
/// This hazard was caught and prevented during #352's code review (see that
/// PR's `evict_oldest_if_over_capacity` comment) and never shipped, but a
/// future edit could reintroduce it, so this hook lets a test pin it down
/// as a regression guard rather than relying on organic tokio scheduling to
/// hit a window only a few CPU instructions wide (#353). `cfg(test)`-only:
/// absent from every non-test build.
#[cfg(test)]
static EVICTION_RACE_HOOK: std::sync::Mutex<
    Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
> = std::sync::Mutex::new(None);

/// Handle returned by [`arm_eviction_race_pause`] to synchronize with the
/// eviction pass it armed.
#[cfg(test)]
struct EvictionRacePause {
    paused_rx: std::sync::mpsc::Receiver<()>,
    resume_tx: std::sync::mpsc::Sender<()>,
}

#[cfg(test)]
impl EvictionRacePause {
    /// Blocks the calling thread until the armed eviction pass reaches its
    /// pause point, or up to 10 seconds. Call from a blocking context (e.g.
    /// `spawn_blocking`), not directly on an async task, to avoid stalling
    /// the runtime. The timeout keeps a broken hook a fast test failure
    /// instead of an indefinite CI hang.
    fn wait_until_paused(&self) {
        self.paused_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("evict_oldest_if_over_capacity never reached the armed pause point");
    }

    /// Releases the paused eviction pass to continue.
    fn resume(self) {
        let _ = self.resume_tx.send(());
    }
}

/// Arms a one-shot pause point inside
/// [`InMemoryEventPublisher::evict_oldest_if_over_capacity`]: the next call
/// that finds `event_log` over capacity blocks right after its capacity
/// check, before collecting its eviction snapshot, until
/// [`EvictionRacePause::resume`] is called. This lets a test run a second
/// eviction pass to completion in between, so the paused pass resumes
/// against an already-shrunk `event_log`.
#[cfg(test)]
fn arm_eviction_race_pause() -> EvictionRacePause {
    let (paused_tx, paused_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    *EVICTION_RACE_HOOK.lock().unwrap() = Some((paused_tx, resume_rx));
    EvictionRacePause {
        paused_rx,
        resume_tx,
    }
}

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
    channel_tx: Arc<tokio::sync::RwLock<Option<ByteBoundedSender<StoredEvent>>>>,
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

impl StoredEvent {
    /// Rough byte-size estimate used only to charge
    /// [`InMemoryEventPublisher::with_channel`]'s streaming-channel byte
    /// budget ([`MAX_QUEUED_EVENT_BYTES`]).
    ///
    /// Not an exact serialized size: `SessionId`/`EventId` don't implement
    /// `Serialize`, so this covers the variable-length parts (`event_type`,
    /// `metadata`) that dominate real payload size, plus a fixed allowance
    /// for the small fixed-size fields. It also doesn't account for
    /// heap/allocator overhead or JSON-escaping expansion, so it under-counts
    /// real memory use — acceptable today because
    /// `DomainEvent::metadata()` (`pjs-domain/src/events/mod.rs`) only ever
    /// produces a handful of small fixed entries, keeping every
    /// `StoredEvent` small regardless of this estimate's precision. If a
    /// future `DomainEvent` variant lets `metadata()` carry large or
    /// caller-controlled content, revisit this estimate (and
    /// `MAX_QUEUED_EVENT_BYTES`, which is sized assuming today's small
    /// events and is otherwise essentially unreachable) — it would no
    /// longer be a safe stand-in for actual memory pressure.
    fn approx_byte_size(&self) -> usize {
        const FIXED_FIELD_ALLOWANCE: usize = 96;
        self.event_type.len()
            + self
                .metadata
                .iter()
                .map(|(k, v)| k.len() + v.len())
                .sum::<usize>()
            + FIXED_FIELD_ALLOWANCE
    }
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
    pub fn with_channel() -> (Self, mpsc::Receiver<Envelope<StoredEvent>>) {
        let (tx, rx) = byte_bounded_channel(EVENT_CHANNEL_CAPACITY, MAX_QUEUED_EVENT_BYTES);
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
            // The `.take()` result is bound in its own statement, not
            // directly in the `if let` scrutinee: temporaries in an `if
            // let` condition stay alive for the whole block, which would
            // otherwise hold the `MutexGuard` (and thus the lock) for as
            // long as this pass stays paused below — deadlocking any other
            // eviction pass that hits this same gate meanwhile.
            #[cfg(test)]
            let armed_pause = EVICTION_RACE_HOOK.lock().unwrap().take();
            #[cfg(test)]
            if let Some((paused_tx, resume_rx)) = armed_pause {
                let _ = paused_tx.send(());
                let _ = resume_rx.recv();
            }

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
            // event publishing, so a full channel (or an over-budget event,
            // see `MAX_QUEUED_EVENT_BYTES`) drops the event and logs rather
            // than awaiting capacity.
            let approx_size = stored_event.approx_byte_size();
            if let Some(tx) = self.channel_tx.read().await.as_ref()
                && let Err(e) = tx.try_send(stored_event, approx_size)
            {
                tracing::warn!("Dropping event from streaming channel: {e:?}");
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
                    let approx_size = stored_event.approx_byte_size();
                    if let Err(e) = tx.try_send(stored_event, approx_size) {
                        tracing::warn!("Dropping event from streaming channel: {e:?}");
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
    async fn test_streaming_channel_rejects_event_exceeding_byte_budget() {
        // Regression test for #349: a message-count bound alone doesn't
        // bound queued bytes. Exercises the byte-bounded channel directly
        // with a small budget (rather than `MAX_QUEUED_EVENT_BYTES`, which
        // no `StoredEvent` producible through the public `publish` API can
        // realistically reach, since `DomainEvent::metadata()` is always
        // small) to prove an over-budget event is rejected rather than
        // queued.
        let (tx, _rx) = byte_bounded_channel::<StoredEvent>(EVENT_CHANNEL_CAPACITY, 10);
        let mut large_metadata = std::collections::HashMap::new();
        large_metadata.insert("key".to_string(), "x".repeat(1000));
        let event = StoredEvent {
            id: EventId::new(),
            event_type: "test".to_string(),
            session_id: None,
            timestamp: chrono::Utc::now(),
            metadata: large_metadata,
            sequence: 0,
        };
        let size = event.approx_byte_size();

        assert!(
            matches!(
                tx.try_send(event, size),
                Err(crate::infrastructure::bounded_channel::TrySendError::BudgetExceeded(_))
            ),
            "an event whose approximate size exceeds the byte budget must be rejected"
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_eviction_forced_interleaving_stays_at_target() {
        // Deterministic regression guard for #353: pins down the
        // stale-`len()`-vs-fresh-snapshot interleaving hazard identified
        // during #352's code review (see this file's
        // `evict_oldest_if_over_capacity` comment) instead of relying on
        // organic tokio scheduling, which essentially never hits a race
        // window only a few CPU instructions wide (see
        // `event_publisher_comprehensive::test_concurrent_eviction_stays_bounded_near_target`
        // in `tests/`, which does rely on natural scheduling and is a
        // coarse sanity check only).
        //
        // Sequence:
        // 1. Arm the pause hook, then publish a large batch (task A) that
        //    pushes `event_log` over capacity. Its eviction pass blocks
        //    right after the capacity check, before it takes its own
        //    eviction snapshot.
        // 2. While A is paused, publish one more event (task B) whose
        //    eviction pass runs to completion, shrinking `event_log` down
        //    to exactly EVENT_LOG_EVICT_TARGET.
        // 3. Resume A. It now takes its eviction snapshot against the
        //    already-shrunk `event_log`.
        //
        // If `excess` were sized from the stale `len()` read *before*
        // pausing (~20,000) instead of the post-resume snapshot (~9,000
        // entries), A would remove far more than the snapshot holds and
        // wipe `event_log` to 0. Deriving `excess` from the snapshot's own
        // length means A removes ~0 additional entries, so the outcome is
        // fully deterministic: exactly EVENT_LOG_EVICT_TARGET.
        let publisher = Arc::new(InMemoryEventPublisher::new());
        let session_id = SessionId::new();

        let pause = arm_eviction_race_pause();

        let publisher_a = Arc::clone(&publisher);
        let task_a = tokio::spawn(async move {
            let events: Vec<DomainEvent> = (0..20_000)
                .map(|_| DomainEvent::SessionActivated {
                    session_id,
                    timestamp: chrono::Utc::now(),
                })
                .collect();
            publisher_a.publish_batch(events).await.unwrap();
        });

        // Block a dedicated thread waiting for task A's eviction pass to
        // hit the pause point, so as not to stall the runtime's async
        // worker threads.
        let paused = tokio::task::spawn_blocking(move || {
            pause.wait_until_paused();
            pause
        })
        .await
        .unwrap();

        // Task B's eviction pass runs to completion while A is still
        // paused, shrinking event_log down to EVENT_LOG_EVICT_TARGET.
        publisher
            .publish(DomainEvent::SessionActivated {
                session_id,
                timestamp: chrono::Utc::now(),
            })
            .await
            .unwrap();

        paused.resume();
        task_a.await.unwrap();

        assert_eq!(
            publisher.event_count(),
            EVENT_LOG_EVICT_TARGET,
            "forced eviction-race interleaving left event_log away from the deterministic target"
        );
    }
}
