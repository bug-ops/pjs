//! Event publisher implementation for PJS domain events

use async_trait::async_trait;
use parking_lot::RwLock;
use std::{
    collections::HashMap,
    sync::Arc,
};
use tokio::sync::mpsc;

use crate::domain::{
    DomainResult,
    events::{DomainEvent, EventId, EventSubscriber},
    ports::EventPublisher,
    value_objects::SessionId,
};

/// In-memory event publisher with subscription support
#[derive(Debug, Clone)]
pub struct InMemoryEventPublisher {
    subscribers: Arc<RwLock<HashMap<String, Vec<Arc<dyn EventSubscriber + Send + Sync>>>>>,
    event_log: Arc<RwLock<Vec<StoredEvent>>>,
    channel_tx: Arc<RwLock<Option<mpsc::UnboundedSender<StoredEvent>>>>,
}

#[derive(Debug, Clone)]
struct StoredEvent {
    id: EventId,
    event_type: String,
    session_id: Option<SessionId>,
    timestamp: chrono::DateTime<chrono::Utc>,
    payload: serde_json::Value,
}

impl InMemoryEventPublisher {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            event_log: Arc::new(RwLock::new(Vec::new())),
            channel_tx: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Initialize event streaming channel
    pub fn with_channel() -> (Self, mpsc::UnboundedReceiver<StoredEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let publisher = Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            event_log: Arc::new(RwLock::new(Vec::new())),
            channel_tx: Arc::new(RwLock::new(Some(tx))),
        };
        (publisher, rx)
    }
    
    /// Subscribe to specific event types
    pub fn subscribe<S>(&self, event_type: &str, subscriber: S) 
    where
        S: EventSubscriber + Send + Sync + 'static,
    {
        let mut subscribers = self.subscribers.write();
        subscribers
            .entry(event_type.to_string())
            .or_insert_with(Vec::new)
            .push(Arc::new(subscriber));
    }
    
    /// Get event count for testing
    pub fn event_count(&self) -> usize {
        self.event_log.read().len()
    }
    
    /// Get events by type
    pub fn events_by_type(&self, event_type: &str) -> Vec<StoredEvent> {
        self.event_log
            .read()
            .iter()
            .filter(|event| event.event_type == event_type)
            .cloned()
            .collect()
    }
    
    /// Get events for session
    pub fn events_for_session(&self, session_id: SessionId) -> Vec<StoredEvent> {
        self.event_log
            .read()
            .iter()
            .filter(|event| event.session_id == Some(session_id))
            .cloned()
            .collect()
    }
    
    /// Clear all events (for testing)
    pub fn clear(&self) {
        self.event_log.write().clear();
    }
    
    /// Get recent events
    pub fn recent_events(&self, limit: usize) -> Vec<StoredEvent> {
        let events = self.event_log.read();
        events
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}

impl Default for InMemoryEventPublisher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventPublisher for InMemoryEventPublisher {
    async fn publish(&self, event: DomainEvent) -> DomainResult<()> {
        let event_type = event.event_type().to_string();
        let stored_event = StoredEvent {
            id: event.event_id(),
            event_type: event_type.clone(),
            session_id: event.session_id(),
            timestamp: event.occurred_at(),
            payload: event.payload().clone(),
        };
        
        // Store event
        {
            let mut log = self.event_log.write();
            log.push(stored_event.clone());
            
            // Keep only last 10000 events to prevent memory growth
            if log.len() > 10000 {
                log.drain(..1000);
            }
        }
        
        // Send to channel if configured
        if let Some(tx) = self.channel_tx.read().as_ref() {
            let _ = tx.send(stored_event.clone());
        }
        
        // Notify subscribers
        {
            let subscribers = self.subscribers.read();
            if let Some(event_subscribers) = subscribers.get(&event_type) {
                for subscriber in event_subscribers {
                    if let Err(e) = subscriber.handle(&event).await {
                        // Log error but don't fail the publish
                        eprintln!("Subscriber error for event {}: {}", event.event_id(), e);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    async fn publish_batch(&self, events: Vec<DomainEvent>) -> DomainResult<()> {
        for event in events {
            self.publish(event).await?;
        }
        Ok(())
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
#[async_trait]
impl EventPublisher for HttpEventPublisher {
    async fn publish(&self, event: DomainEvent) -> DomainResult<()> {
        let payload = serde_json::json!({
            "event_id": event.event_id().to_string(),
            "event_type": event.event_type(),
            "session_id": event.session_id().map(|id| id.to_string()),
            "occurred_at": event.occurred_at(),
            "payload": event.payload()
        });
        
        for attempt in 0..self.retry_attempts {
            match self.client
                .post(&self.endpoint)
                .json(&payload)
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) => {
                    eprintln!("HTTP event publish failed with status: {}", response.status());
                    if attempt == self.retry_attempts - 1 {
                        return Err(format!("HTTP publish failed: {}", response.status()).into());
                    }
                },
                Err(e) => {
                    eprintln!("HTTP event publish error (attempt {}): {}", attempt + 1, e);
                    if attempt == self.retry_attempts - 1 {
                        return Err(format!("HTTP publish error: {}", e).into());
                    }
                }
            }
            
            // Exponential backoff
            tokio::time::sleep(std::time::Duration::from_millis(100 << attempt)).await;
        }
        
        Ok(())
    }
    
    async fn publish_batch(&self, events: Vec<DomainEvent>) -> DomainResult<()> {
        let batch_payload: Vec<_> = events.iter()
            .map(|event| serde_json::json!({
                "event_id": event.event_id().to_string(),
                "event_type": event.event_type(),
                "session_id": event.session_id().map(|id| id.to_string()),
                "occurred_at": event.occurred_at(),
                "payload": event.payload()
            }))
            .collect();
        
        for attempt in 0..self.retry_attempts {
            match self.client
                .post(&format!("{}/batch", self.endpoint))
                .json(&batch_payload)
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) => {
                    eprintln!("HTTP batch publish failed with status: {}", response.status());
                    if attempt == self.retry_attempts - 1 {
                        return Err(format!("HTTP batch publish failed: {}", response.status()).into());
                    }
                },
                Err(e) => {
                    eprintln!("HTTP batch publish error (attempt {}): {}", attempt + 1, e);
                    if attempt == self.retry_attempts - 1 {
                        return Err(format!("HTTP batch publish error: {}", e).into());
                    }
                }
            }
            
            tokio::time::sleep(std::time::Duration::from_millis(100 << attempt)).await;
        }
        
        Ok(())
    }
}

/// Composite event publisher that sends to multiple destinations
#[derive(Clone)]
pub struct CompositeEventPublisher {
    publishers: Vec<Arc<dyn EventPublisher + Send + Sync>>,
    fail_fast: bool,
}

impl CompositeEventPublisher {
    pub fn new() -> Self {
        Self {
            publishers: Vec::new(),
            fail_fast: false,
        }
    }
    
    pub fn add_publisher<P>(mut self, publisher: P) -> Self 
    where
        P: EventPublisher + Send + Sync + 'static,
    {
        self.publishers.push(Arc::new(publisher));
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

#[async_trait]
impl EventPublisher for CompositeEventPublisher {
    async fn publish(&self, event: DomainEvent) -> DomainResult<()> {
        let mut errors = Vec::new();
        
        for publisher in &self.publishers {
            match publisher.publish(event.clone()).await {
                Ok(()) => {},
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
            Err(format!("Multiple publish errors: {:?}", errors).into())
        }
    }
    
    async fn publish_batch(&self, events: Vec<DomainEvent>) -> DomainResult<()> {
        let mut errors = Vec::new();
        
        for publisher in &self.publishers {
            match publisher.publish_batch(events.clone()).await {
                Ok(()) => {},
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
            Err(format!("Multiple batch publish errors: {:?}", errors).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        events::{SessionCreated, StreamStarted},
        value_objects::{SessionId, StreamId},
    };
    
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
            self.received_events.read().len()
        }
    }
    
    #[async_trait]
    impl EventSubscriber for TestSubscriber {
        async fn handle(&self, event: &DomainEvent) -> DomainResult<()> {
            self.received_events.write().push(event.clone());
            Ok(())
        }
    }
    
    #[tokio::test]
    async fn test_in_memory_event_publisher() {
        let publisher = InMemoryEventPublisher::new();
        let subscriber = TestSubscriber::new();
        
        publisher.subscribe("SessionCreated", subscriber.clone());
        
        let session_id = SessionId::new();
        let event = SessionCreated::new(session_id, chrono::Utc::now()).into();
        
        publisher.publish(event).await.unwrap();
        
        assert_eq!(publisher.event_count(), 1);
        assert_eq!(subscriber.event_count(), 1);
        
        let events_for_session = publisher.events_for_session(session_id);
        assert_eq!(events_for_session.len(), 1);
    }
    
    #[tokio::test]
    async fn test_event_publisher_with_channel() {
        let (publisher, mut rx) = InMemoryEventPublisher::with_channel();
        
        let session_id = SessionId::new();
        let event = SessionCreated::new(session_id, chrono::Utc::now()).into();
        
        publisher.publish(event).await.unwrap();
        
        let received = rx.recv().await.unwrap();
        assert_eq!(received.event_type, "SessionCreated");
        assert_eq!(received.session_id, Some(session_id));
    }
    
    #[tokio::test]
    async fn test_batch_publishing() {
        let publisher = InMemoryEventPublisher::new();
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        
        let events = vec![
            SessionCreated::new(session_id, chrono::Utc::now()).into(),
            StreamStarted::new(stream_id, session_id, chrono::Utc::now()).into(),
        ];
        
        publisher.publish_batch(events).await.unwrap();
        
        assert_eq!(publisher.event_count(), 2);
    }
    
    #[tokio::test]
    async fn test_composite_publisher() {
        let publisher1 = InMemoryEventPublisher::new();
        let publisher2 = InMemoryEventPublisher::new();
        
        let composite = CompositeEventPublisher::new()
            .add_publisher(publisher1.clone())
            .add_publisher(publisher2.clone());
        
        let session_id = SessionId::new();
        let event = SessionCreated::new(session_id, chrono::Utc::now()).into();
        
        composite.publish(event).await.unwrap();
        
        assert_eq!(publisher1.event_count(), 1);
        assert_eq!(publisher2.event_count(), 1);
    }
}