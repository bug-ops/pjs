//! In-memory repository implementation for testing and development

use async_trait::async_trait;
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};

use crate::domain::{
    DomainResult,
    value_objects::{SessionId, StreamId},
    aggregates::StreamSession,
    entities::Stream,
    ports::{StreamRepository, StreamStore},
};

/// In-memory implementation of StreamRepository
#[derive(Debug, Clone)]
pub struct InMemoryStreamRepository {
    sessions: Arc<RwLock<HashMap<SessionId, StreamSession>>>,
}

impl InMemoryStreamRepository {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Get number of stored sessions
    pub fn session_count(&self) -> usize {
        self.sessions.read().len()
    }
    
    /// Clear all sessions (for testing)
    pub fn clear(&self) {
        self.sessions.write().clear();
    }
    
    /// Get all session IDs (for testing)
    pub fn all_session_ids(&self) -> Vec<SessionId> {
        self.sessions.read().keys().copied().collect()
    }
}

impl Default for InMemoryStreamRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StreamRepository for InMemoryStreamRepository {
    async fn find_session(&self, session_id: SessionId) -> DomainResult<Option<StreamSession>> {
        let sessions = self.sessions.read();
        Ok(sessions.get(&session_id).cloned())
    }
    
    async fn save_session(&self, session: StreamSession) -> DomainResult<()> {
        let mut sessions = self.sessions.write();
        sessions.insert(session.id(), session);
        Ok(())
    }
    
    async fn remove_session(&self, session_id: SessionId) -> DomainResult<()> {
        let mut sessions = self.sessions.write();
        sessions.remove(&session_id);
        Ok(())
    }
    
    async fn find_active_sessions(&self) -> DomainResult<Vec<StreamSession>> {
        let sessions = self.sessions.read();
        let active_sessions = sessions
            .values()
            .filter(|session| session.is_active())
            .cloned()
            .collect();
        Ok(active_sessions)
    }
}

/// In-memory implementation of StreamStore
#[derive(Debug, Clone)]
pub struct InMemoryStreamStore {
    streams: Arc<RwLock<HashMap<StreamId, Stream>>>,
    session_streams: Arc<RwLock<HashMap<SessionId, Vec<StreamId>>>>,
}

impl InMemoryStreamStore {
    pub fn new() -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
            session_streams: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Get number of stored streams
    pub fn stream_count(&self) -> usize {
        self.streams.read().len()
    }
    
    /// Clear all streams (for testing)
    pub fn clear(&self) {
        self.streams.write().clear();
        self.session_streams.write().clear();
    }
    
    /// Get all stream IDs (for testing)
    pub fn all_stream_ids(&self) -> Vec<StreamId> {
        self.streams.read().keys().copied().collect()
    }
}

impl Default for InMemoryStreamStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StreamStore for InMemoryStreamStore {
    async fn store_stream(&self, stream: Stream) -> DomainResult<()> {
        let stream_id = stream.id();
        let session_id = stream.session_id();
        
        // Store the stream
        {
            let mut streams = self.streams.write();
            streams.insert(stream_id, stream);
        }
        
        // Update session -> streams mapping
        {
            let mut session_streams = self.session_streams.write();
            session_streams
                .entry(session_id)
                .or_insert_with(Vec::new)
                .push(stream_id);
        }
        
        Ok(())
    }
    
    async fn get_stream(&self, stream_id: StreamId) -> DomainResult<Option<Stream>> {
        let streams = self.streams.read();
        Ok(streams.get(&stream_id).cloned())
    }
    
    async fn delete_stream(&self, stream_id: StreamId) -> DomainResult<()> {
        // Get session ID before removing stream
        let session_id = {
            let streams = self.streams.read();
            streams.get(&stream_id).map(|s| s.session_id())
        };
        
        // Remove stream
        {
            let mut streams = self.streams.write();
            streams.remove(&stream_id);
        }
        
        // Remove from session mapping
        if let Some(session_id) = session_id {
            let mut session_streams = self.session_streams.write();
            if let Some(stream_ids) = session_streams.get_mut(&session_id) {
                stream_ids.retain(|&id| id != stream_id);
                
                // Remove empty session mapping
                if stream_ids.is_empty() {
                    session_streams.remove(&session_id);
                }
            }
        }
        
        Ok(())
    }
    
    async fn list_streams_for_session(&self, session_id: SessionId) -> DomainResult<Vec<Stream>> {
        let session_streams = self.session_streams.read();
        let stream_ids = session_streams.get(&session_id).cloned().unwrap_or_default();
        
        let streams = self.streams.read();
        let mut result = Vec::new();
        
        for stream_id in stream_ids {
            if let Some(stream) = streams.get(&stream_id) {
                result.push(stream.clone());
            }
        }
        
        Ok(result)
    }
}

/// Thread-safe in-memory repository with concurrent access optimization
#[derive(Debug, Clone)]
pub struct ConcurrentMemoryRepository {
    repository: InMemoryStreamRepository,
    store: InMemoryStreamStore,
}

impl ConcurrentMemoryRepository {
    pub fn new() -> Self {
        Self {
            repository: InMemoryStreamRepository::new(),
            store: InMemoryStreamStore::new(),
        }
    }
    
    pub fn repository(&self) -> &InMemoryStreamRepository {
        &self.repository
    }
    
    pub fn store(&self) -> &InMemoryStreamStore {
        &self.store
    }
    
    /// Get comprehensive statistics
    pub fn statistics(&self) -> RepositoryStatistics {
        RepositoryStatistics {
            session_count: self.repository.session_count(),
            stream_count: self.store.stream_count(),
            active_session_count: 0, // Would need async call to calculate
        }
    }
    
    /// Clear all data
    pub fn clear_all(&self) {
        self.repository.clear();
        self.store.clear();
    }
}

impl Default for ConcurrentMemoryRepository {
    fn default() -> Self {
        Self::new()
    }
}

/// Repository statistics for monitoring
#[derive(Debug, Clone)]
pub struct RepositoryStatistics {
    pub session_count: usize,
    pub stream_count: usize,
    pub active_session_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        aggregates::stream_session::SessionConfig,
        entities::stream::StreamConfig,
    };
    
    #[tokio::test]
    async fn test_memory_repository_basic_operations() {
        let repo = InMemoryStreamRepository::new();
        
        // Create and save session
        let mut session = StreamSession::new(SessionConfig::default());
        let _ = session.activate();
        let session_id = session.id();
        
        repo.save_session(session.clone()).await.unwrap();
        
        // Retrieve session
        let retrieved = repo.find_session(session_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id(), session_id);
        
        // Check active sessions
        let active = repo.find_active_sessions().await.unwrap();
        assert_eq!(active.len(), 1);
        
        // Remove session
        repo.remove_session(session_id).await.unwrap();
        let removed = repo.find_session(session_id).await.unwrap();
        assert!(removed.is_none());
    }
    
    #[tokio::test]
    async fn test_stream_store_operations() {
        let store = InMemoryStreamStore::new();
        let session_id = SessionId::new();
        
        // Create and store stream
        let stream = Stream::new(
            session_id,
            serde_json::json!({"test": "data"}),
            StreamConfig::default(),
        );
        let stream_id = stream.id();
        
        store.store_stream(stream.clone()).await.unwrap();
        
        // Retrieve stream
        let retrieved = store.get_stream(stream_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id(), stream_id);
        
        // List streams for session
        let session_streams = store.list_streams_for_session(session_id).await.unwrap();
        assert_eq!(session_streams.len(), 1);
        
        // Delete stream
        store.delete_stream(stream_id).await.unwrap();
        let deleted = store.get_stream(stream_id).await.unwrap();
        assert!(deleted.is_none());
    }
    
    #[tokio::test]
    async fn test_concurrent_repository() {
        let repo = ConcurrentMemoryRepository::new();
        
        let stats = repo.statistics();
        assert_eq!(stats.session_count, 0);
        assert_eq!(stats.stream_count, 0);
        
        // Create session
        let session = StreamSession::new(SessionConfig::default());
        let session_id = session.id();
        repo.repository().save_session(session).await.unwrap();
        
        // Create stream  
        let stream = Stream::new(
            session_id,
            serde_json::json!({"test": "data"}),
            StreamConfig::default(),
        );
        repo.store().store_stream(stream).await.unwrap();
        
        let stats = repo.statistics();
        assert_eq!(stats.session_count, 1);
        assert_eq!(stats.stream_count, 1);
        
        // Clear all
        repo.clear_all();
        let stats = repo.statistics();
        assert_eq!(stats.session_count, 0);
        assert_eq!(stats.stream_count, 0);
    }
    
    #[tokio::test]
    async fn test_concurrent_access() {
        use std::sync::Arc;
        use tokio::task::JoinSet;
        
        let repo = Arc::new(InMemoryStreamRepository::new());
        let mut tasks = JoinSet::new();
        
        // Spawn multiple tasks to test concurrent access
        for i in 0..10 {
            let repo_clone = repo.clone();
            tasks.spawn(async move {
                let mut session = StreamSession::new(SessionConfig::default());
                let _ = session.activate();
                
                // TODO: Add unique identifier when metadata support is added
                // Each session is already unique by its SessionId
                
                repo_clone.save_session(session).await.unwrap();
            });
        }
        
        // Wait for all tasks to complete
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
        
        // Verify all sessions were saved
        assert_eq!(repo.session_count(), 10);
        
        let active_sessions = repo.find_active_sessions().await.unwrap();
        assert_eq!(active_sessions.len(), 10);
    }
}