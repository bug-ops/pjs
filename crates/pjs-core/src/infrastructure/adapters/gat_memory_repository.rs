//! GAT-based in-memory repository implementations
//!
//! Zero-cost abstractions for domain ports using Generic Associated Types.

use std::future::Future;

use crate::domain::{
    DomainResult,
    aggregates::StreamSession,
    entities::Stream,
    ports::{StreamRepositoryGat, StreamStoreGat},
    value_objects::{SessionId, StreamId},
};

use super::generic_store::{SessionStore, StreamStore};

/// GAT-based in-memory implementation of StreamRepositoryGat
#[derive(Debug, Clone, Default)]
pub struct GatInMemoryStreamRepository {
    store: SessionStore,
}

impl GatInMemoryStreamRepository {
    pub fn new() -> Self {
        Self {
            store: SessionStore::new(),
        }
    }

    /// Get number of stored sessions
    pub fn session_count(&self) -> usize {
        self.store.count()
    }

    /// Clear all sessions (for testing)
    pub fn clear(&self) {
        self.store.clear();
    }

    /// Get all session IDs (for testing)
    pub fn all_session_ids(&self) -> Vec<SessionId> {
        self.store.all_keys()
    }
}

impl StreamRepositoryGat for GatInMemoryStreamRepository {
    type FindSessionFuture<'a>
        = impl Future<Output = DomainResult<Option<StreamSession>>> + Send + 'a
    where
        Self: 'a;

    type SaveSessionFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type RemoveSessionFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type FindActiveSessionsFuture<'a>
        = impl Future<Output = DomainResult<Vec<StreamSession>>> + Send + 'a
    where
        Self: 'a;

    fn find_session(&self, session_id: SessionId) -> Self::FindSessionFuture<'_> {
        async move { Ok(self.store.get(&session_id)) }
    }

    fn save_session(&self, session: StreamSession) -> Self::SaveSessionFuture<'_> {
        async move {
            self.store.insert(session.id(), session);
            Ok(())
        }
    }

    fn remove_session(&self, session_id: SessionId) -> Self::RemoveSessionFuture<'_> {
        async move {
            self.store.remove(&session_id);
            Ok(())
        }
    }

    fn find_active_sessions(&self) -> Self::FindActiveSessionsFuture<'_> {
        async move { Ok(self.store.filter(|s| s.is_active())) }
    }
}

/// GAT-based in-memory implementation of StreamStoreGat
#[derive(Debug, Clone, Default)]
pub struct GatInMemoryStreamStore {
    store: StreamStore,
}

impl GatInMemoryStreamStore {
    pub fn new() -> Self {
        Self {
            store: StreamStore::new(),
        }
    }

    /// Get number of stored streams
    pub fn stream_count(&self) -> usize {
        self.store.count()
    }

    /// Clear all streams (for testing)
    pub fn clear(&self) {
        self.store.clear();
    }

    /// Get all stream IDs (for testing)
    pub fn all_stream_ids(&self) -> Vec<StreamId> {
        self.store.all_keys()
    }
}

impl StreamStoreGat for GatInMemoryStreamStore {
    type StoreStreamFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type GetStreamFuture<'a>
        = impl Future<Output = DomainResult<Option<Stream>>> + Send + 'a
    where
        Self: 'a;

    type DeleteStreamFuture<'a>
        = impl Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type ListStreamsFuture<'a>
        = impl Future<Output = DomainResult<Vec<Stream>>> + Send + 'a
    where
        Self: 'a;

    fn store_stream(&self, stream: Stream) -> Self::StoreStreamFuture<'_> {
        async move {
            self.store.insert(stream.id(), stream);
            Ok(())
        }
    }

    fn get_stream(&self, stream_id: StreamId) -> Self::GetStreamFuture<'_> {
        async move { Ok(self.store.get(&stream_id)) }
    }

    fn delete_stream(&self, stream_id: StreamId) -> Self::DeleteStreamFuture<'_> {
        async move {
            self.store.remove(&stream_id);
            Ok(())
        }
    }

    fn list_streams_for_session(&self, session_id: SessionId) -> Self::ListStreamsFuture<'_> {
        async move { Ok(self.store.filter(|s| s.session_id() == session_id)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::aggregates::stream_session::SessionConfig;

    #[tokio::test]
    async fn test_gat_repository_crud() {
        let repo = GatInMemoryStreamRepository::new();

        let session = StreamSession::new(SessionConfig::default());
        let session_id = session.id();

        repo.save_session(session.clone()).await.unwrap();

        let found = repo.find_session(session_id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id(), session_id);

        repo.remove_session(session_id).await.unwrap();
        let not_found = repo.find_session(session_id).await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_gat_store_crud() {
        let store = GatInMemoryStreamStore::new();

        assert_eq!(store.stream_count(), 0);
        store.clear();
        assert_eq!(store.stream_count(), 0);
    }
}
