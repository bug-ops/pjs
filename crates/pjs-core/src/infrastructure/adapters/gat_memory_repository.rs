//! GAT-based in-memory repository implementations
//!
//! Zero-cost abstractions for domain ports using Generic Associated Types.

use parking_lot::RwLock;
use std::{collections::HashMap, future::Future, sync::Arc};

use crate::domain::{
    DomainResult,
    value_objects::{SessionId, StreamId},
    aggregates::StreamSession,
    entities::Stream,
    ports::{StreamRepositoryGat, StreamStoreGat},
};

/// GAT-based in-memory implementation of StreamRepositoryGat
#[derive(Debug, Clone)]
pub struct GatInMemoryStreamRepository {
    sessions: Arc<RwLock<HashMap<SessionId, StreamSession>>>,
}

impl GatInMemoryStreamRepository {
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

impl Default for GatInMemoryStreamRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamRepositoryGat for GatInMemoryStreamRepository {
    type FindSessionFuture<'a> = impl Future<Output = DomainResult<Option<StreamSession>>> + Send + 'a
    where
        Self: 'a;

    type SaveSessionFuture<'a> = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type RemoveSessionFuture<'a> = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type FindActiveSessionsFuture<'a> = impl Future<Output = DomainResult<Vec<StreamSession>>> + Send + 'a
    where
        Self: 'a;

    fn find_session(&self, session_id: SessionId) -> Self::FindSessionFuture<'_> {
        async move {
            Ok(self.sessions.read().get(&session_id).cloned())
        }
    }

    fn save_session(&self, session: StreamSession) -> Self::SaveSessionFuture<'_> {
        async move {
            self.sessions.write().insert(session.id(), session);
            Ok(())
        }
    }

    fn remove_session(&self, session_id: SessionId) -> Self::RemoveSessionFuture<'_> {
        async move {
            self.sessions.write().remove(&session_id);
            Ok(())
        }
    }

    fn find_active_sessions(&self) -> Self::FindActiveSessionsFuture<'_> {
        async move {
            Ok(self.sessions
                .read()
                .values()
                .filter(|s| s.is_active())
                .cloned()
                .collect())
        }
    }
}

/// GAT-based in-memory implementation of StreamStoreGat
#[derive(Debug, Clone)]
pub struct GatInMemoryStreamStore {
    streams: Arc<RwLock<HashMap<StreamId, Stream>>>,
}

impl GatInMemoryStreamStore {
    pub fn new() -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Get number of stored streams
    pub fn stream_count(&self) -> usize {
        self.streams.read().len()
    }
    
    /// Clear all streams (for testing)
    pub fn clear(&self) {
        self.streams.write().clear();
    }
    
    /// Get all stream IDs (for testing)
    pub fn all_stream_ids(&self) -> Vec<StreamId> {
        self.streams.read().keys().copied().collect()
    }
}

impl Default for GatInMemoryStreamStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamStoreGat for GatInMemoryStreamStore {
    type StoreStreamFuture<'a> = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type GetStreamFuture<'a> = impl Future<Output = DomainResult<Option<Stream>>> + Send + 'a
    where
        Self: 'a;

    type DeleteStreamFuture<'a> = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type ListStreamsFuture<'a> = impl Future<Output = DomainResult<Vec<Stream>>> + Send + 'a
    where
        Self: 'a;

    fn store_stream(&self, stream: Stream) -> Self::StoreStreamFuture<'_> {
        async move {
            self.streams.write().insert(stream.id(), stream);
            Ok(())
        }
    }

    fn get_stream(&self, stream_id: StreamId) -> Self::GetStreamFuture<'_> {
        async move {
            Ok(self.streams.read().get(&stream_id).cloned())
        }
    }

    fn delete_stream(&self, stream_id: StreamId) -> Self::DeleteStreamFuture<'_> {
        async move {
            self.streams.write().remove(&stream_id);
            Ok(())
        }
    }

    fn list_streams_for_session(&self, session_id: SessionId) -> Self::ListStreamsFuture<'_> {
        async move {
            Ok(self.streams
                .read()
                .values()
                .filter(|s| s.session_id() == session_id)
                .cloned()
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::aggregates::stream_session::SessionConfig;

    #[tokio::test]
    async fn test_gat_repository_crud() {
        let repo = GatInMemoryStreamRepository::new();
        
        // Test save and find
        let session = StreamSession::new(SessionConfig::default());
        let session_id = session.id();
        
        repo.save_session(session.clone()).await.unwrap();
        
        let found = repo.find_session(session_id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id(), session_id);
        
        // Test remove
        repo.remove_session(session_id).await.unwrap();
        let not_found = repo.find_session(session_id).await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_gat_store_crud() {
        let store = GatInMemoryStreamStore::new();
        
        // Would need to create a proper Stream for full testing
        // For now just test the interface works
        assert_eq!(store.stream_count(), 0);
        store.clear();
        assert_eq!(store.stream_count(), 0);
    }
}