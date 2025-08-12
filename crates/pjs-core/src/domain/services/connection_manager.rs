use async_std::sync::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::domain::DomainError;
use crate::domain::value_objects::{SessionId, StreamId};

/// Connection state tracking
#[derive(Debug, Clone)]
pub struct ConnectionState {
    pub session_id: SessionId,
    pub stream_id: Option<StreamId>,
    pub connected_at: Instant,
    pub last_activity: Instant,
    pub bytes_sent: usize,
    pub bytes_received: usize,
    pub is_active: bool,
}

/// Connection lifecycle events
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    Connected(SessionId),
    Disconnected(SessionId),
    Timeout(SessionId),
    Error(SessionId, String),
}

/// Connection manager service
pub struct ConnectionManager {
    connections: Arc<RwLock<HashMap<SessionId, ConnectionState>>>,
    timeout_duration: Duration,
    max_connections: usize,
}

impl ConnectionManager {
    pub fn new(timeout_duration: Duration, max_connections: usize) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            timeout_duration,
            max_connections,
        }
    }

    /// Register a new connection
    pub async fn register_connection(&self, session_id: SessionId) -> Result<(), DomainError> {
        let mut connections = self.connections.write().await;

        if connections.len() >= self.max_connections {
            return Err(DomainError::ValidationError(
                "Maximum connections reached".to_string(),
            ));
        }

        let state = ConnectionState {
            session_id,
            stream_id: None,
            connected_at: Instant::now(),
            last_activity: Instant::now(),
            bytes_sent: 0,
            bytes_received: 0,
            is_active: true,
        };

        connections.insert(session_id, state);
        Ok(())
    }

    /// Update connection activity
    pub async fn update_activity(&self, session_id: &SessionId) -> Result<(), DomainError> {
        let mut connections = self.connections.write().await;

        match connections.get_mut(session_id) {
            Some(state) => {
                state.last_activity = Instant::now();
                Ok(())
            }
            None => Err(DomainError::ValidationError(format!(
                "Connection not found: {session_id}"
            ))),
        }
    }

    /// Update connection metrics
    pub async fn update_metrics(
        &self,
        session_id: &SessionId,
        bytes_sent: usize,
        bytes_received: usize,
    ) -> Result<(), DomainError> {
        let mut connections = self.connections.write().await;

        match connections.get_mut(session_id) {
            Some(state) => {
                state.bytes_sent += bytes_sent;
                state.bytes_received += bytes_received;
                state.last_activity = Instant::now();
                Ok(())
            }
            None => Err(DomainError::ValidationError(format!(
                "Connection not found: {session_id}"
            ))),
        }
    }

    /// Associate stream with connection
    pub async fn set_stream(
        &self,
        session_id: &SessionId,
        stream_id: StreamId,
    ) -> Result<(), DomainError> {
        let mut connections = self.connections.write().await;

        match connections.get_mut(session_id) {
            Some(state) => {
                state.stream_id = Some(stream_id);
                state.last_activity = Instant::now();
                Ok(())
            }
            None => Err(DomainError::ValidationError(format!(
                "Connection not found: {session_id}"
            ))),
        }
    }

    /// Close connection
    pub async fn close_connection(&self, session_id: &SessionId) -> Result<(), DomainError> {
        let mut connections = self.connections.write().await;

        match connections.get_mut(session_id) {
            Some(state) => {
                state.is_active = false;
                Ok(())
            }
            None => Err(DomainError::ValidationError(format!(
                "Connection not found: {session_id}"
            ))),
        }
    }

    /// Remove connection completely
    pub async fn remove_connection(&self, session_id: &SessionId) -> Result<(), DomainError> {
        let mut connections = self.connections.write().await;

        match connections.remove(session_id) {
            Some(_) => Ok(()),
            None => Err(DomainError::ValidationError(format!(
                "Connection not found: {session_id}"
            ))),
        }
    }

    /// Get connection state
    pub async fn get_connection(&self, session_id: &SessionId) -> Option<ConnectionState> {
        let connections = self.connections.read().await;
        connections.get(session_id).cloned()
    }

    /// Get all active connections
    pub async fn get_active_connections(&self) -> Vec<ConnectionState> {
        let connections = self.connections.read().await;
        connections
            .values()
            .filter(|state| state.is_active)
            .cloned()
            .collect()
    }

    /// Check for timed out connections
    pub async fn check_timeouts(&self) -> Vec<SessionId> {
        let now = Instant::now();
        let connections = self.connections.read().await;

        connections
            .values()
            .filter(|state| {
                state.is_active && now.duration_since(state.last_activity) > self.timeout_duration
            })
            .map(|state| state.session_id)
            .collect()
    }

    /// Process timeout check iteration (to be called by infrastructure layer)
    pub async fn process_timeouts(&self) {
        let timed_out = self.check_timeouts().await;
        for session_id in timed_out {
            if let Err(e) = self.close_connection(&session_id).await {
                tracing::warn!("Failed to close timed out connection: {e}");
            }
        }
    }

    /// Get connection statistics
    pub async fn get_statistics(&self) -> ConnectionStatistics {
        let connections = self.connections.read().await;

        let active_count = connections.values().filter(|s| s.is_active).count();
        let total_bytes_sent: usize = connections.values().map(|s| s.bytes_sent).sum();
        let total_bytes_received: usize = connections.values().map(|s| s.bytes_received).sum();

        ConnectionStatistics {
            total_connections: connections.len(),
            active_connections: active_count,
            inactive_connections: connections.len() - active_count,
            total_bytes_sent,
            total_bytes_received,
        }
    }
}

/// Connection statistics
#[derive(Debug, Clone)]
pub struct ConnectionStatistics {
    pub total_connections: usize,
    pub active_connections: usize,
    pub inactive_connections: usize,
    pub total_bytes_sent: usize,
    pub total_bytes_received: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_lifecycle() {
        let manager = ConnectionManager::new(Duration::from_secs(60), 100);
        let session_id = SessionId::new();

        // Register connection
        assert!(
            manager
                .register_connection(session_id)
                .await
                .is_ok()
        );

        // Get connection state
        let state = manager.get_connection(&session_id).await;
        assert!(state.is_some());
        assert!(state.unwrap().is_active);

        // Update activity
        assert!(manager.update_activity(&session_id).await.is_ok());

        // Update metrics
        assert!(manager.update_metrics(&session_id, 100, 50).await.is_ok());

        // Close connection
        assert!(manager.close_connection(&session_id).await.is_ok());

        // Verify closed
        let state = manager.get_connection(&session_id).await;
        assert!(state.is_some());
        assert!(!state.unwrap().is_active);

        // Remove connection
        assert!(manager.remove_connection(&session_id).await.is_ok());

        // Verify removed
        let state = manager.get_connection(&session_id).await;
        assert!(state.is_none());
    }

    #[tokio::test]
    async fn test_max_connections() {
        let manager = ConnectionManager::new(Duration::from_secs(60), 2);

        // Register max connections
        let session1 = SessionId::new();
        let session2 = SessionId::new();
        let session3 = SessionId::new();

        assert!(manager.register_connection(session1).await.is_ok());
        assert!(manager.register_connection(session2).await.is_ok());

        // Should fail - max reached
        assert!(manager.register_connection(session3).await.is_err());
    }

    #[tokio::test]
    async fn test_timeout_detection() {
        let manager = ConnectionManager::new(Duration::from_millis(100), 10);
        let session_id = SessionId::new();

        assert!(
            manager
                .register_connection(session_id)
                .await
                .is_ok()
        );

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        let timed_out = manager.check_timeouts().await;
        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0], session_id);
    }
}
