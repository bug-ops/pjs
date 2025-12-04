//! Comprehensive tests for GAT-based memory repositories
//!
//! Coverage targets:
//! - CRUD operations for sessions
//! - CRUD operations for streams
//! - Query operations
//! - Concurrent access patterns
//! - Repository helper methods

use pjson_rs::{
    domain::{
        aggregates::{StreamSession, stream_session::SessionConfig},
        entities::Stream,
        ports::{StreamRepositoryGat, StreamStoreGat},
        value_objects::{JsonData, SessionId, StreamId},
    },
    infrastructure::adapters::gat_memory_repository::{
        GatInMemoryStreamRepository, GatInMemoryStreamStore,
    },
};
use std::sync::Arc;
use tokio::task::JoinSet;

// ============================================================================
// StreamRepository Tests - Creation
// ============================================================================

#[test]
fn test_repository_new_empty() {
    let repo = GatInMemoryStreamRepository::new();

    assert_eq!(repo.session_count(), 0);
}

#[test]
fn test_repository_default_empty() {
    let repo = GatInMemoryStreamRepository::default();

    assert_eq!(repo.session_count(), 0);
}

#[test]
fn test_repository_clone() {
    let repo1 = GatInMemoryStreamRepository::new();
    let repo2 = repo1.clone();

    // Clones share the same underlying storage
    assert_eq!(repo1.session_count(), repo2.session_count());
}

// ============================================================================
// StreamRepository Tests - CRUD Operations
// ============================================================================

#[tokio::test]
async fn test_save_and_find_session() {
    let repo = GatInMemoryStreamRepository::new();
    let session = StreamSession::new(SessionConfig::default());
    let session_id = session.id();

    repo.save_session(session.clone()).await.unwrap();

    let found = repo.find_session(session_id).await.unwrap();

    assert!(found.is_some());
    assert_eq!(found.unwrap().id(), session_id);
}

#[tokio::test]
async fn test_find_nonexistent_session() {
    let repo = GatInMemoryStreamRepository::new();
    let fake_id = SessionId::new();

    let result = repo.find_session(fake_id).await.unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn test_save_multiple_sessions() {
    let repo = GatInMemoryStreamRepository::new();

    let session1 = StreamSession::new(SessionConfig::default());
    let session2 = StreamSession::new(SessionConfig::default());
    let session3 = StreamSession::new(SessionConfig::default());

    repo.save_session(session1.clone()).await.unwrap();
    repo.save_session(session2.clone()).await.unwrap();
    repo.save_session(session3.clone()).await.unwrap();

    assert_eq!(repo.session_count(), 3);

    let found1 = repo.find_session(session1.id()).await.unwrap();
    let found2 = repo.find_session(session2.id()).await.unwrap();
    let found3 = repo.find_session(session3.id()).await.unwrap();

    assert!(found1.is_some());
    assert!(found2.is_some());
    assert!(found3.is_some());
}

#[tokio::test]
async fn test_save_overwrites_existing_session() {
    let repo = GatInMemoryStreamRepository::new();
    let mut session = StreamSession::new(SessionConfig::default());
    let session_id = session.id();

    repo.save_session(session.clone()).await.unwrap();
    assert_eq!(repo.session_count(), 1);

    // Activate and save again
    session.activate().unwrap();
    repo.save_session(session.clone()).await.unwrap();

    assert_eq!(repo.session_count(), 1); // Still 1 session

    let found = repo.find_session(session_id).await.unwrap().unwrap();
    assert!(found.is_active()); // Should have updated state
}

#[tokio::test]
async fn test_remove_session() {
    let repo = GatInMemoryStreamRepository::new();
    let session = StreamSession::new(SessionConfig::default());
    let session_id = session.id();

    repo.save_session(session).await.unwrap();
    assert_eq!(repo.session_count(), 1);

    repo.remove_session(session_id).await.unwrap();
    assert_eq!(repo.session_count(), 0);

    let found = repo.find_session(session_id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_remove_nonexistent_session() {
    let repo = GatInMemoryStreamRepository::new();
    let fake_id = SessionId::new();

    let result = repo.remove_session(fake_id).await;

    // Should succeed (idempotent)
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_clear_all_sessions() {
    let repo = GatInMemoryStreamRepository::new();

    for _ in 0..5 {
        let session = StreamSession::new(SessionConfig::default());
        repo.save_session(session).await.unwrap();
    }

    assert_eq!(repo.session_count(), 5);

    repo.clear();

    assert_eq!(repo.session_count(), 0);
}

// ============================================================================
// StreamRepository Tests - Query Operations
// ============================================================================

#[tokio::test]
async fn test_find_active_sessions_empty() {
    let repo = GatInMemoryStreamRepository::new();

    let active = repo.find_active_sessions().await.unwrap();

    assert_eq!(active.len(), 0);
}

#[tokio::test]
async fn test_find_active_sessions_filters_inactive() {
    let repo = GatInMemoryStreamRepository::new();

    let session1 = StreamSession::new(SessionConfig::default());
    let mut session2 = StreamSession::new(SessionConfig::default());
    let mut session3 = StreamSession::new(SessionConfig::default());

    session2.activate().unwrap();
    session3.activate().unwrap();

    repo.save_session(session1).await.unwrap();
    repo.save_session(session2).await.unwrap();
    repo.save_session(session3).await.unwrap();

    let active = repo.find_active_sessions().await.unwrap();

    assert_eq!(active.len(), 2); // Only activated sessions
}

#[tokio::test]
async fn test_find_active_sessions_all_active() {
    let repo = GatInMemoryStreamRepository::new();

    for _ in 0..5 {
        let mut session = StreamSession::new(SessionConfig::default());
        session.activate().unwrap();
        repo.save_session(session).await.unwrap();
    }

    let active = repo.find_active_sessions().await.unwrap();

    assert_eq!(active.len(), 5);
}

#[tokio::test]
async fn test_all_session_ids() {
    let repo = GatInMemoryStreamRepository::new();

    let session1 = StreamSession::new(SessionConfig::default());
    let session2 = StreamSession::new(SessionConfig::default());

    let id1 = session1.id();
    let id2 = session2.id();

    repo.save_session(session1).await.unwrap();
    repo.save_session(session2).await.unwrap();

    let all_ids = repo.all_session_ids();

    assert_eq!(all_ids.len(), 2);
    assert!(all_ids.contains(&id1));
    assert!(all_ids.contains(&id2));
}

#[tokio::test]
async fn test_all_session_ids_empty() {
    let repo = GatInMemoryStreamRepository::new();

    let all_ids = repo.all_session_ids();

    assert_eq!(all_ids.len(), 0);
}

// ============================================================================
// StreamRepository Tests - Concurrent Access
// ============================================================================

#[tokio::test]
async fn test_concurrent_saves() {
    let repo = Arc::new(GatInMemoryStreamRepository::new());
    let mut tasks = JoinSet::new();

    for _ in 0..10 {
        let repo_clone = Arc::clone(&repo);
        tasks.spawn(async move {
            let session = StreamSession::new(SessionConfig::default());
            repo_clone.save_session(session).await.unwrap();
        });
    }

    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }

    assert_eq!(repo.session_count(), 10);
}

#[tokio::test]
async fn test_concurrent_reads() {
    let repo = Arc::new(GatInMemoryStreamRepository::new());
    let session = StreamSession::new(SessionConfig::default());
    let session_id = session.id();

    repo.save_session(session).await.unwrap();

    let mut tasks = JoinSet::new();

    for _ in 0..10 {
        let repo_clone = Arc::clone(&repo);
        let id = session_id;
        tasks.spawn(async move {
            let found = repo_clone.find_session(id).await.unwrap();
            assert!(found.is_some());
        });
    }

    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
}

#[tokio::test]
async fn test_concurrent_mixed_operations() {
    let repo = Arc::new(GatInMemoryStreamRepository::new());
    let mut tasks = JoinSet::new();

    // Create some initial sessions
    for _ in 0..5 {
        let session = StreamSession::new(SessionConfig::default());
        repo.save_session(session).await.unwrap();
    }

    // Mix of operations
    for i in 0..20 {
        let repo_clone = Arc::clone(&repo);
        if i % 3 == 0 {
            // Save
            tasks.spawn(async move {
                let session = StreamSession::new(SessionConfig::default());
                repo_clone.save_session(session).await.unwrap();
            });
        } else if i % 3 == 1 {
            // Read
            tasks.spawn(async move {
                let _ = repo_clone.find_active_sessions().await;
            });
        } else {
            // Count
            tasks.spawn(async move {
                let _ = repo_clone.session_count();
            });
        }
    }

    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }

    // Should have initial 5 + ~7 new saves = ~12 sessions
    assert!(repo.session_count() >= 5);
}

// ============================================================================
// StreamStore Tests - Creation
// ============================================================================

#[test]
fn test_store_new_empty() {
    let store = GatInMemoryStreamStore::new();

    assert_eq!(store.stream_count(), 0);
}

#[test]
fn test_store_default_empty() {
    let store = GatInMemoryStreamStore::default();

    assert_eq!(store.stream_count(), 0);
}

#[test]
fn test_store_clone() {
    let store1 = GatInMemoryStreamStore::new();
    let store2 = store1.clone();

    assert_eq!(store1.stream_count(), store2.stream_count());
}

// ============================================================================
// StreamStore Tests - CRUD Operations
// ============================================================================

#[tokio::test]
async fn test_store_and_get_stream() {
    let store = GatInMemoryStreamStore::new();
    let session_id = SessionId::new();
    let data = JsonData::Object(std::collections::HashMap::new());
    let stream = Stream::new(
        session_id,
        data,
        pjson_rs::domain::entities::stream::StreamConfig::default(),
    );
    let stream_id = stream.id();

    store.store_stream(stream.clone()).await.unwrap();

    let found = store.get_stream(stream_id).await.unwrap();

    assert!(found.is_some());
    assert_eq!(found.unwrap().id(), stream_id);
}

#[tokio::test]
async fn test_get_nonexistent_stream() {
    let store = GatInMemoryStreamStore::new();
    let fake_id = StreamId::new();

    let result = store.get_stream(fake_id).await.unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn test_store_multiple_streams() {
    let store = GatInMemoryStreamStore::new();
    let session_id = SessionId::new();

    for _ in 0..5 {
        let data = JsonData::Object(std::collections::HashMap::new());
        let stream = Stream::new(
            session_id,
            data,
            pjson_rs::domain::entities::stream::StreamConfig::default(),
        );
        store.store_stream(stream).await.unwrap();
    }

    assert_eq!(store.stream_count(), 5);
}

#[tokio::test]
async fn test_store_overwrites_existing_stream() {
    let store = GatInMemoryStreamStore::new();
    let session_id = SessionId::new();
    let data = JsonData::Object(std::collections::HashMap::new());
    let mut stream = Stream::new(
        session_id,
        data.clone(),
        pjson_rs::domain::entities::stream::StreamConfig::default(),
    );
    let stream_id = stream.id();

    store.store_stream(stream.clone()).await.unwrap();
    assert_eq!(store.stream_count(), 1);

    // Start and store again
    stream.start_streaming().unwrap();
    store.store_stream(stream.clone()).await.unwrap();

    assert_eq!(store.stream_count(), 1); // Still 1 stream

    let found = store.get_stream(stream_id).await.unwrap().unwrap();
    assert!(found.is_active()); // Should have updated state
}

#[tokio::test]
async fn test_delete_stream() {
    let store = GatInMemoryStreamStore::new();
    let session_id = SessionId::new();
    let data = JsonData::Object(std::collections::HashMap::new());
    let stream = Stream::new(
        session_id,
        data,
        pjson_rs::domain::entities::stream::StreamConfig::default(),
    );
    let stream_id = stream.id();

    store.store_stream(stream).await.unwrap();
    assert_eq!(store.stream_count(), 1);

    store.delete_stream(stream_id).await.unwrap();
    assert_eq!(store.stream_count(), 0);

    let found = store.get_stream(stream_id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_delete_nonexistent_stream() {
    let store = GatInMemoryStreamStore::new();
    let fake_id = StreamId::new();

    let result = store.delete_stream(fake_id).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_clear_all_streams() {
    let store = GatInMemoryStreamStore::new();
    let session_id = SessionId::new();

    for _ in 0..5 {
        let data = JsonData::Object(std::collections::HashMap::new());
        let stream = Stream::new(
            session_id,
            data,
            pjson_rs::domain::entities::stream::StreamConfig::default(),
        );
        store.store_stream(stream).await.unwrap();
    }

    assert_eq!(store.stream_count(), 5);

    store.clear();

    assert_eq!(store.stream_count(), 0);
}

// ============================================================================
// StreamStore Tests - Query Operations
// ============================================================================

#[tokio::test]
async fn test_list_streams_for_session_empty() {
    let store = GatInMemoryStreamStore::new();
    let session_id = SessionId::new();

    let streams = store.list_streams_for_session(session_id).await.unwrap();

    assert_eq!(streams.len(), 0);
}

#[tokio::test]
async fn test_list_streams_for_session_filters() {
    let store = GatInMemoryStreamStore::new();
    let session1 = SessionId::new();
    let session2 = SessionId::new();

    // Add streams for session1
    for _ in 0..3 {
        let data = JsonData::Object(std::collections::HashMap::new());
        let stream = Stream::new(
            session1,
            data,
            pjson_rs::domain::entities::stream::StreamConfig::default(),
        );
        store.store_stream(stream).await.unwrap();
    }

    // Add streams for session2
    for _ in 0..2 {
        let data = JsonData::Object(std::collections::HashMap::new());
        let stream = Stream::new(
            session2,
            data,
            pjson_rs::domain::entities::stream::StreamConfig::default(),
        );
        store.store_stream(stream).await.unwrap();
    }

    let session1_streams = store.list_streams_for_session(session1).await.unwrap();
    let session2_streams = store.list_streams_for_session(session2).await.unwrap();

    assert_eq!(session1_streams.len(), 3);
    assert_eq!(session2_streams.len(), 2);
}

#[tokio::test]
async fn test_all_stream_ids() {
    let store = GatInMemoryStreamStore::new();
    let session_id = SessionId::new();

    let data1 = JsonData::Object(std::collections::HashMap::new());
    let stream1 = Stream::new(
        session_id,
        data1,
        pjson_rs::domain::entities::stream::StreamConfig::default(),
    );
    let id1 = stream1.id();

    let data2 = JsonData::Object(std::collections::HashMap::new());
    let stream2 = Stream::new(
        session_id,
        data2,
        pjson_rs::domain::entities::stream::StreamConfig::default(),
    );
    let id2 = stream2.id();

    store.store_stream(stream1).await.unwrap();
    store.store_stream(stream2).await.unwrap();

    let all_ids = store.all_stream_ids();

    assert_eq!(all_ids.len(), 2);
    assert!(all_ids.contains(&id1));
    assert!(all_ids.contains(&id2));
}

#[tokio::test]
async fn test_all_stream_ids_empty() {
    let store = GatInMemoryStreamStore::new();

    let all_ids = store.all_stream_ids();

    assert_eq!(all_ids.len(), 0);
}

// ============================================================================
// StreamStore Tests - Concurrent Access
// ============================================================================

#[tokio::test]
async fn test_concurrent_stream_stores() {
    let store = Arc::new(GatInMemoryStreamStore::new());
    let session_id = SessionId::new();
    let mut tasks = JoinSet::new();

    for _ in 0..10 {
        let store_clone = Arc::clone(&store);
        tasks.spawn(async move {
            let data = JsonData::Object(std::collections::HashMap::new());
            let stream = Stream::new(
                session_id,
                data,
                pjson_rs::domain::entities::stream::StreamConfig::default(),
            );
            store_clone.store_stream(stream).await.unwrap();
        });
    }

    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }

    assert_eq!(store.stream_count(), 10);
}

#[tokio::test]
async fn test_concurrent_stream_reads() {
    let store = Arc::new(GatInMemoryStreamStore::new());
    let session_id = SessionId::new();
    let data = JsonData::Object(std::collections::HashMap::new());
    let stream = Stream::new(
        session_id,
        data,
        pjson_rs::domain::entities::stream::StreamConfig::default(),
    );
    let stream_id = stream.id();

    store.store_stream(stream).await.unwrap();

    let mut tasks = JoinSet::new();

    for _ in 0..10 {
        let store_clone = Arc::clone(&store);
        tasks.spawn(async move {
            let found = store_clone.get_stream(stream_id).await.unwrap();
            assert!(found.is_some());
        });
    }

    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
}

#[tokio::test]
async fn test_concurrent_stream_mixed_operations() {
    let store = Arc::new(GatInMemoryStreamStore::new());
    let session_id = SessionId::new();
    let mut tasks = JoinSet::new();

    // Create some initial streams
    for _ in 0..5 {
        let data = JsonData::Object(std::collections::HashMap::new());
        let stream = Stream::new(
            session_id,
            data,
            pjson_rs::domain::entities::stream::StreamConfig::default(),
        );
        store.store_stream(stream).await.unwrap();
    }

    // Mix of operations
    for i in 0..20 {
        let store_clone = Arc::clone(&store);
        if i % 3 == 0 {
            // Store
            tasks.spawn(async move {
                let data = JsonData::Object(std::collections::HashMap::new());
                let stream = Stream::new(
                    session_id,
                    data,
                    pjson_rs::domain::entities::stream::StreamConfig::default(),
                );
                store_clone.store_stream(stream).await.unwrap();
            });
        } else if i % 3 == 1 {
            // List
            tasks.spawn(async move {
                let _ = store_clone.list_streams_for_session(session_id).await;
            });
        } else {
            // Count
            tasks.spawn(async move {
                let _ = store_clone.stream_count();
            });
        }
    }

    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }

    assert!(store.stream_count() >= 5);
}

// ============================================================================
// Integration Tests - Both Repositories Together
// ============================================================================

#[tokio::test]
async fn test_session_and_stream_integration() {
    let session_repo = GatInMemoryStreamRepository::new();
    let stream_store = GatInMemoryStreamStore::new();

    // Create session
    let mut session = StreamSession::new(SessionConfig::default());
    session.activate().unwrap();
    let session_id = session.id();

    session_repo.save_session(session).await.unwrap();

    // Create streams for this session
    for _ in 0..3 {
        let data = JsonData::Object(std::collections::HashMap::new());
        let stream = Stream::new(
            session_id,
            data,
            pjson_rs::domain::entities::stream::StreamConfig::default(),
        );
        stream_store.store_stream(stream).await.unwrap();
    }

    // Verify
    let stored_session = session_repo.find_session(session_id).await.unwrap();
    let stored_streams = stream_store
        .list_streams_for_session(session_id)
        .await
        .unwrap();

    assert!(stored_session.is_some());
    assert_eq!(stored_streams.len(), 3);
}
