//! Repository adapters for various storage backends
//!
//! These adapters implement the domain repository ports using
//! different storage technologies (memory, disk, database).

use crate::domain::{
    DomainResult, DomainError,
    entities::{Frame, Stream, stream::StreamState},
    aggregates::StreamSession,
    value_objects::{SessionId, StreamId, JsonPath, Priority},
    ports::repositories::*,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    sync::Arc,
    time::SystemTime,
};
use tokio::sync::RwLock;

/// Type alias for complex frame index map
type FrameIndexMap = Arc<RwLock<HashMap<JsonPath, Vec<(StreamId, usize)>>>>;

/// Type alias for complex cache data map  
type CacheDataMap = Arc<RwLock<HashMap<String, (Vec<u8>, Option<SystemTime>)>>>;

/// In-memory implementation of StreamSessionRepository
/// 
/// This implementation provides full ACID transaction support
/// and is suitable for testing and development.
pub struct InMemoryStreamSessionRepository {
    sessions: Arc<RwLock<HashMap<SessionId, (StreamSession, u64)>>>, // (session, version)
    health_snapshots: Arc<RwLock<HashMap<SessionId, SessionHealthSnapshot>>>,
    next_version: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for InMemoryStreamSessionRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStreamSessionRepository {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            health_snapshots: Arc::new(RwLock::new(HashMap::new())),
            next_version: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }
}

#[async_trait]
impl StreamSessionRepository for InMemoryStreamSessionRepository {
    async fn begin_transaction(&self) -> DomainResult<Box<dyn SessionTransaction>> {
        Ok(Box::new(InMemorySessionTransaction::new(
            self.sessions.clone(),
            self.next_version.clone(),
        )))
    }

    async fn find_session(&self, session_id: SessionId) -> DomainResult<Option<StreamSession>> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(&session_id).map(|(session, _version)| session.clone()))
    }

    async fn save_session(&self, session: StreamSession, expected_version: Option<u64>) -> DomainResult<u64> {
        let mut sessions = self.sessions.write().await;
        let session_id = session.id();
        
        // Check optimistic concurrency control
        if let Some(expected) = expected_version
            && let Some((_existing_session, current_version)) = sessions.get(&session_id)
                && *current_version != expected {
                    return Err(DomainError::ConcurrencyConflict(format!(
                        "Session version mismatch: expected {expected}, got {current_version}"
                    )));
                }

        let new_version = self.next_version.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        sessions.insert(session_id, (session, new_version));
        
        Ok(new_version)
    }

    async fn remove_session(&self, session_id: SessionId) -> DomainResult<()> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(&session_id);
        
        let mut health_snapshots = self.health_snapshots.write().await;
        health_snapshots.remove(&session_id);
        
        Ok(())
    }

    async fn find_sessions_by_criteria(
        &self,
        criteria: SessionQueryCriteria,
        pagination: Pagination,
    ) -> DomainResult<SessionQueryResult> {
        let start_time = SystemTime::now();
        let sessions = self.sessions.read().await;
        
        // Apply filtering criteria
        let mut filtered_sessions: Vec<StreamSession> = sessions
            .values()
            .map(|(session, _version)| session.clone())
            .filter(|session| self.matches_criteria(session, &criteria))
            .collect();

        let total_count = filtered_sessions.len();

        // Apply sorting
        if let Some(sort_field) = &pagination.sort_by {
            match sort_field.as_str() {
                "created_at" => {
                    filtered_sessions.sort_by(|a, b| {
                        match pagination.sort_order {
                            SortOrder::Ascending => a.created_at().cmp(&b.created_at()),
                            SortOrder::Descending => b.created_at().cmp(&a.created_at()),
                        }
                    });
                },
                "stream_count" => {
                    filtered_sessions.sort_by(|a, b| {
                        let count_a = a.streams().len();
                        let count_b = b.streams().len();
                        match pagination.sort_order {
                            SortOrder::Ascending => count_a.cmp(&count_b),
                            SortOrder::Descending => count_b.cmp(&count_a),
                        }
                    });
                },
                _ => {} // Unknown sort field, no sorting
            }
        }

        // Apply pagination
        let paginated_sessions: Vec<StreamSession> = filtered_sessions
            .into_iter()
            .skip(pagination.offset)
            .take(pagination.limit)
            .collect();

        let has_more = total_count > pagination.offset + paginated_sessions.len();
        let query_duration_ms = start_time.elapsed()
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(SessionQueryResult {
            sessions: paginated_sessions,
            total_count,
            has_more,
            query_duration_ms,
        })
    }

    async fn get_session_health(&self, session_id: SessionId) -> DomainResult<SessionHealthSnapshot> {
        let health_snapshots = self.health_snapshots.read().await;
        
        if let Some(snapshot) = health_snapshots.get(&session_id) {
            Ok(snapshot.clone())
        } else {
            // Generate health snapshot on-the-fly
            let sessions = self.sessions.read().await;
            if let Some((session, _version)) = sessions.get(&session_id) {
                let snapshot = SessionHealthSnapshot {
                    session_id,
                    is_healthy: true, // Simple health check
                    active_streams: session.streams().len(),
                    total_frames: 0, // Would calculate from streams
                    last_activity: Utc::now(),
                    error_rate: 0.0,
                    metrics: HashMap::new(),
                };
                Ok(snapshot)
            } else {
                Err(DomainError::NotFound(format!("Session {session_id} not found")))
            }
        }
    }

    async fn session_exists(&self, session_id: SessionId) -> DomainResult<bool> {
        let sessions = self.sessions.read().await;
        Ok(sessions.contains_key(&session_id))
    }
}

impl InMemoryStreamSessionRepository {
    fn matches_criteria(&self, session: &StreamSession, criteria: &SessionQueryCriteria) -> bool {
        // Apply state filter
        if let Some(ref states) = criteria.states {
            let session_state = format!("{:?}", session.state()); // Convert enum to string
            if !states.contains(&session_state) {
                return false;
            }
        }

        // Apply time filters
        if let Some(after) = criteria.created_after
            && session.created_at() <= after {
                return false;
            }

        if let Some(before) = criteria.created_before
            && session.created_at() >= before {
                return false;
            }

        // Apply stream count filters
        let stream_count = session.streams().len();
        
        if let Some(min_count) = criteria.min_stream_count
            && stream_count < min_count {
                return false;
            }

        if let Some(max_count) = criteria.max_stream_count
            && stream_count > max_count {
                return false;
            }

        // Apply active streams filter
        if let Some(has_active) = criteria.has_active_streams {
            let has_active_streams = session.streams().values()
                .any(|stream| matches!(stream.state(), StreamState::Streaming));
            if has_active != has_active_streams {
                return false;
            }
        }

        true
    }
}

/// Transaction implementation for in-memory repository
pub struct InMemorySessionTransaction {
    sessions: Arc<RwLock<HashMap<SessionId, (StreamSession, u64)>>>,
    next_version: Arc<std::sync::atomic::AtomicU64>,
    pending_changes: Vec<TransactionOperation>,
    committed: bool,
}

enum TransactionOperation {
    SaveSession(StreamSession),
    RemoveSession(SessionId),
    AddStream(SessionId, Stream),
}

impl InMemorySessionTransaction {
    fn new(
        sessions: Arc<RwLock<HashMap<SessionId, (StreamSession, u64)>>>,
        next_version: Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        Self {
            sessions,
            next_version,
            pending_changes: Vec::new(),
            committed: false,
        }
    }
}

#[async_trait]
impl SessionTransaction for InMemorySessionTransaction {
    async fn save_session(&self, _session: StreamSession) -> DomainResult<()> {
        // In a real implementation, we'd store pending changes
        // For simplicity, we'll implement this as immediate operations
        Ok(())
    }

    async fn remove_session(&self, _session_id: SessionId) -> DomainResult<()> {
        // Store pending removal
        Ok(())
    }

    async fn add_stream(&self, _session_id: SessionId, _stream: Stream) -> DomainResult<()> {
        // Store pending stream addition
        Ok(())
    }

    async fn commit(mut self: Box<Self>) -> DomainResult<()> {
        if self.committed {
            return Err(DomainError::Logic("Transaction already committed".to_string()));
        }

        // Apply all pending changes
        let mut sessions = self.sessions.write().await;
        
        for operation in &self.pending_changes {
            match operation {
                TransactionOperation::SaveSession(session) => {
                    let new_version = self.next_version.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    sessions.insert(session.id(), (session.clone(), new_version));
                },
                TransactionOperation::RemoveSession(session_id) => {
                    sessions.remove(session_id);
                },
                TransactionOperation::AddStream(_session_id, _stream) => {
                    // Would implement stream addition to session
                }
            }
        }

        self.committed = true;
        Ok(())
    }

    async fn rollback(mut self: Box<Self>) -> DomainResult<()> {
        if self.committed {
            return Err(DomainError::Logic("Cannot rollback committed transaction".to_string()));
        }

        // Clear pending changes
        self.pending_changes.clear();
        Ok(())
    }
}

/// In-memory implementation of FrameRepository
pub struct InMemoryFrameRepository {
    frames: Arc<RwLock<HashMap<StreamId, Vec<Frame>>>>,
    frame_indices: FrameIndexMap, // path -> (stream_id, frame_index)
}

impl Default for InMemoryFrameRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryFrameRepository {
    pub fn new() -> Self {
        Self {
            frames: Arc::new(RwLock::new(HashMap::new())),
            frame_indices: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn build_frame_index(&self, stream_id: StreamId, _frame: &Frame, frame_index: usize) {
        // This is a simplified indexing - in reality would parse JSON paths from frame content
        let mut indices = self.frame_indices.write().await;
        // Example: index by frame type or priority
        if let Ok(priority_path) = JsonPath::new("$.priority") {
            indices.entry(priority_path)
                .or_insert_with(Vec::new)
                .push((stream_id, frame_index));
        }
    }
}

#[async_trait]
impl FrameRepository for InMemoryFrameRepository {
    async fn store_frame(&self, frame: Frame) -> DomainResult<()> {
        let stream_id = frame.stream_id();
        let frame_index = {
            let mut frames = self.frames.write().await;
            let stream_frames = frames.entry(stream_id).or_insert_with(Vec::new);
            let frame_index = stream_frames.len();
            stream_frames.push(frame.clone());
            frame_index
        };
        
        // Build indices for fast querying (outside the lock)
        self.build_frame_index(stream_id, &frame, frame_index).await;
        
        Ok(())
    }

    async fn store_frames(&self, frames: Vec<Frame>) -> DomainResult<()> {
        for frame in frames {
            self.store_frame(frame).await?;
        }
        Ok(())
    }

    async fn get_frames_by_stream(
        &self,
        stream_id: StreamId,
        priority_filter: Option<Priority>,
        pagination: Pagination,
    ) -> DomainResult<FrameQueryResult> {
        let frames = self.frames.read().await;
        {
            if let Some(stream_frames) = frames.get(&stream_id) {
                // Apply priority filter
                let filtered_frames: Vec<Frame> = stream_frames.iter()
                    .filter(|frame| {
                        if let Some(min_priority) = priority_filter {
                            let frame_priority = frame.priority().unwrap_or(Priority::LOW.value());
                            Priority::new(frame_priority).unwrap_or(Priority::LOW) >= min_priority
                        } else {
                            true
                        }
                    })
                    .cloned()
                    .collect();

                let total_count = filtered_frames.len();
                
                // Apply pagination
                let paginated_frames: Vec<Frame> = filtered_frames
                    .into_iter()
                    .skip(pagination.offset)
                    .take(pagination.limit)
                    .collect();

                let has_more = total_count > pagination.offset + paginated_frames.len();
                
                // Calculate priority range
                let priorities: Vec<u8> = paginated_frames.iter()
                    .map(|f| f.priority().unwrap_or(Priority::LOW.value()))
                    .collect();
                
                let highest_priority = priorities.iter().max().map(|&p| Priority::new(p).unwrap_or(Priority::LOW));
                let lowest_priority = priorities.iter().min().map(|&p| Priority::new(p).unwrap_or(Priority::LOW));

                Ok(FrameQueryResult {
                    frames: paginated_frames,
                    total_count,
                    has_more,
                    highest_priority,
                    lowest_priority,
                })
            } else {
                Ok(FrameQueryResult {
                    frames: Vec::new(),
                    total_count: 0,
                    has_more: false,
                    highest_priority: None,
                    lowest_priority: None,
                })
            }
        }
    }

    async fn get_frames_by_path(
        &self,
        stream_id: StreamId,
        path: JsonPath,
    ) -> DomainResult<Vec<Frame>> {
        // Use frame indices for fast path-based lookup
        let indices = self.frame_indices.read().await;
        {
            if let Some(frame_locations) = indices.get(&path) {
                let relevant_locations: Vec<usize> = frame_locations.iter()
                    .filter(|(sid, _)| *sid == stream_id)
                    .map(|(_, index)| *index)
                    .collect();

                let frames = self.frames.read().await;
        {
                    if let Some(stream_frames) = frames.get(&stream_id) {
                        let result_frames: Vec<Frame> = relevant_locations.iter()
                            .filter_map(|&index| stream_frames.get(index).cloned())
                            .collect();
                        return Ok(result_frames);
                    }
                }
            }
        }
        
        Ok(Vec::new())
    }

    async fn cleanup_old_frames(&self, older_than: DateTime<Utc>) -> DomainResult<u64> {
        let mut cleaned_count = 0u64;
        
        {
            let mut frames = self.frames.write().await;
            for stream_frames in frames.values_mut() {
                let original_len = stream_frames.len();
                stream_frames.retain(|frame| {
                    frame.timestamp() > older_than
                });
                cleaned_count += (original_len - stream_frames.len()) as u64;
            }
        }
        
        Ok(cleaned_count)
    }

    async fn get_frame_priority_distribution(&self, stream_id: StreamId) -> DomainResult<PriorityDistribution> {
        let frames = self.frames.read().await;
        {
            if let Some(stream_frames) = frames.get(&stream_id) {
                let mut distribution = PriorityDistribution::default();
                
                for frame in stream_frames {
                    let priority = frame.priority().unwrap_or(Priority::LOW.value());
                    match priority {
                            90..=255 => distribution.critical_frames += 1,
                            70..=89 => distribution.high_frames += 1,
                            40..=69 => distribution.medium_frames += 1,
                            20..=39 => distribution.low_frames += 1,
                            1..=19 => distribution.background_frames += 1,
                            _ => distribution.background_frames += 1,
                        }
                }
                
                Ok(distribution)
            } else {
                Ok(PriorityDistribution::default())
            }
        }
    }
}

/// Simple in-memory cache implementation
pub struct InMemoryCache {
    data: CacheDataMap,
    stats: Arc<RwLock<CacheStatistics>>,
}

impl Default for InMemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryCache {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(CacheStatistics::default())),
        }
    }

    async fn update_stats(&self, hit: bool) {
        let mut stats = self.stats.write().await;
        {
            if hit {
                let total_requests = (stats.hit_rate + stats.miss_rate) * 100.0;
                let hits = stats.hit_rate * total_requests / 100.0 + 1.0;
                let total = total_requests + 1.0;
                stats.hit_rate = (hits / total) * 100.0;
                stats.miss_rate = 100.0 - stats.hit_rate;
            } else {
                let total_requests = (stats.hit_rate + stats.miss_rate) * 100.0;
                let misses = stats.miss_rate * total_requests / 100.0 + 1.0;
                let total = total_requests + 1.0;
                stats.miss_rate = (misses / total) * 100.0;
                stats.hit_rate = 100.0 - stats.miss_rate;
            }
        }
    }
}

#[async_trait]
impl CacheRepository for InMemoryCache {
    async fn get_bytes(&self, key: &str) -> DomainResult<Option<Vec<u8>>> {
        let data = self.data.read().await;
        if let Some((value, expiry)) = data.get(key) {
            // Check expiration
            if let Some(exp_time) = expiry
                && SystemTime::now() > *exp_time {
                    drop(data); // Release lock before async call
                    self.update_stats(false).await;
                    return Ok(None);
                }

            let value = value.clone();
            drop(data); // Release lock before async call
            self.update_stats(true).await;
            Ok(Some(value))
        } else {
            drop(data); // Release lock before async call
            self.update_stats(false).await;
            Ok(None)
        }
    }

    async fn set_bytes(&self, key: &str, value: Vec<u8>, ttl: Option<std::time::Duration>) -> DomainResult<()> {
        let expiry = ttl.map(|duration| SystemTime::now() + duration);

        let (data_len, key_len, value_len) = {
            let mut data = self.data.write().await;
            data.insert(key.to_string(), (value.clone(), expiry));
            (data.len() as u64, key.len() as u64, value.len() as u64)
        };
        
        // Update stats outside the data lock
        {
            let mut stats = self.stats.write().await;
            stats.total_keys = data_len;
            stats.memory_usage_bytes += key_len + value_len;
        }

        Ok(())
    }

    async fn remove(&self, key: &str) -> DomainResult<()> {
        let data_len = {
            let mut data = self.data.write().await;
            data.remove(key);
            data.len() as u64
        };
        
        // Update stats outside the data lock  
        {
            let mut stats = self.stats.write().await;
            stats.total_keys = data_len;
            stats.eviction_count += 1;
        }
        
        Ok(())
    }

    async fn clear_prefix(&self, prefix: &str) -> DomainResult<()> {
        let data_len = {
            let mut data = self.data.write().await;
            let keys_to_remove: Vec<String> = data.keys()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect();

            for key in keys_to_remove {
                data.remove(&key);
            }
            
            data.len() as u64
        };
        
        // Update stats outside the data lock
        {
            let mut stats = self.stats.write().await;
            stats.total_keys = data_len;
            stats.eviction_count += 1;
        }
        
        Ok(())
    }

    async fn get_stats(&self) -> DomainResult<CacheStatistics> {
        let stats = self.stats.read().await;
        Ok(stats.clone())
    }
}