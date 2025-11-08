//! Session management service with timeout monitoring and cleanup
//!
//! This service provides centralized session lifecycle management with
//! automatic timeout detection and cleanup capabilities.

use crate::{
    ApplicationResult,
    domain::{DomainError, aggregates::stream_session::StreamSession, value_objects::SessionId},
};
use dashmap::DashMap;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::RwLock,
    time::{Instant as TokioInstant, interval},
};

/// Configuration for session management
#[derive(Debug, Clone)]
pub struct SessionManagerConfig {
    /// How often to check for expired sessions (in seconds)
    pub cleanup_interval_seconds: u64,
    /// Maximum number of sessions to keep in memory
    pub max_sessions: usize,
    /// Default session timeout if not specified
    pub default_timeout_seconds: u64,
    /// Grace period before forced cleanup (in seconds)
    pub grace_period_seconds: u64,
}

impl Default for SessionManagerConfig {
    fn default() -> Self {
        Self {
            cleanup_interval_seconds: 60,  // Check every minute
            max_sessions: 10_000,          // 10K concurrent sessions max
            default_timeout_seconds: 3600, // 1 hour default
            grace_period_seconds: 300,     // 5 minute grace period
        }
    }
}

/// Statistics about session management
#[derive(Debug, Clone, Default)]
pub struct SessionManagerStats {
    /// Total number of active sessions
    pub active_sessions: usize,
    /// Number of sessions cleaned up due to timeout
    pub timeout_cleanups: u64,
    /// Number of sessions cleaned up gracefully
    pub graceful_cleanups: u64,
    /// Last cleanup timestamp
    pub last_cleanup_at: Option<TokioInstant>,
    /// Average session duration in seconds
    pub average_session_duration: f64,
}

/// Session management service with timeout monitoring
pub struct SessionManager {
    /// Active sessions storage
    sessions: Arc<DashMap<SessionId, Arc<RwLock<StreamSession>>>>,
    /// Configuration
    config: SessionManagerConfig,
    /// Statistics
    stats: Arc<RwLock<SessionManagerStats>>,
    /// Cleanup task handle
    cleanup_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::with_config(SessionManagerConfig::default())
    }
}

impl SessionManager {
    /// Create new session manager with default config
    pub fn new() -> Self {
        Self::default()
    }

    /// Create new session manager with custom config
    pub fn with_config(config: SessionManagerConfig) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            config,
            stats: Arc::new(RwLock::new(SessionManagerStats::default())),
            cleanup_handle: None,
        }
    }

    /// Start the session manager with automatic cleanup
    pub async fn start(&mut self) -> ApplicationResult<()> {
        if self.cleanup_handle.is_some() {
            return Err(
                DomainError::InternalError("Session manager already started".to_string()).into(),
            );
        }

        let sessions = Arc::clone(&self.sessions);
        let stats = Arc::clone(&self.stats);
        let config = self.config.clone();

        let handle = tokio::spawn(async move {
            Self::cleanup_task(sessions, stats, config).await;
        });

        self.cleanup_handle = Some(handle);
        Ok(())
    }

    /// Stop the session manager
    pub async fn stop(&mut self) -> ApplicationResult<()> {
        if let Some(handle) = self.cleanup_handle.take() {
            handle.abort();
            let _ = handle.await;
        }
        Ok(())
    }

    /// Add session to management
    pub async fn add_session(&self, session: StreamSession) -> ApplicationResult<SessionId> {
        let session_id = session.id();

        // Check capacity
        if self.sessions.len() >= self.config.max_sessions {
            return Err(DomainError::ResourceExhausted(format!(
                "Maximum sessions limit reached: {}",
                self.config.max_sessions
            ))
            .into());
        }

        self.sessions
            .insert(session_id, Arc::new(RwLock::new(session)));

        // Update stats
        let mut stats = self.stats.write().await;
        stats.active_sessions = self.sessions.len();

        Ok(session_id)
    }

    /// Get session by ID
    pub async fn get_session(&self, session_id: &SessionId) -> Option<Arc<RwLock<StreamSession>>> {
        self.sessions
            .get(session_id)
            .map(|entry| Arc::clone(entry.value()))
    }

    /// Remove session from management
    pub async fn remove_session(&self, session_id: &SessionId) -> ApplicationResult<bool> {
        let removed = self.sessions.remove(session_id).is_some();

        if removed {
            let mut stats = self.stats.write().await;
            stats.active_sessions = self.sessions.len();
            stats.graceful_cleanups += 1;
        }

        Ok(removed)
    }

    /// Force cleanup of expired sessions
    pub async fn cleanup_expired_sessions(&self) -> ApplicationResult<CleanupReport> {
        let mut report = CleanupReport::default();
        let mut sessions_to_remove = Vec::new();

        // Collect expired sessions
        for entry in self.sessions.iter() {
            let session_id = *entry.key();
            let session_arc = entry.value();

            let mut session = session_arc.write().await;

            if session.is_expired() {
                // Try to force close with cleanup
                match session.force_close_expired() {
                    Ok(was_closed) => {
                        if was_closed {
                            sessions_to_remove.push(session_id);
                            report.timeout_cleanups += 1;
                        }
                    }
                    Err(e) => {
                        report
                            .errors
                            .push(format!("Failed to close session {}: {}", session_id, e));
                    }
                }
            }
        }

        // Remove cleaned up sessions
        for session_id in sessions_to_remove {
            self.sessions.remove(&session_id);
            report.sessions_removed += 1;
        }

        // Update stats
        let mut stats = self.stats.write().await;
        stats.active_sessions = self.sessions.len();
        stats.timeout_cleanups += report.timeout_cleanups;
        stats.last_cleanup_at = Some(TokioInstant::now());

        Ok(report)
    }

    /// Get current statistics
    pub async fn stats(&self) -> SessionManagerStats {
        self.stats.read().await.clone()
    }

    /// Internal cleanup task
    async fn cleanup_task(
        sessions: Arc<DashMap<SessionId, Arc<RwLock<StreamSession>>>>,
        stats: Arc<RwLock<SessionManagerStats>>,
        config: SessionManagerConfig,
    ) {
        let mut interval = interval(Duration::from_secs(config.cleanup_interval_seconds));

        loop {
            interval.tick().await;

            let mut cleanup_count = 0;
            let mut sessions_to_remove = Vec::new();

            // Check each session for expiration
            for entry in sessions.iter() {
                let session_id = *entry.key();
                let session_arc = entry.value();

                let mut session = session_arc.write().await;

                if session.is_expired() {
                    match session.force_close_expired() {
                        Ok(was_closed) => {
                            if was_closed {
                                sessions_to_remove.push(session_id);
                                cleanup_count += 1;
                            }
                        }
                        Err(_) => {
                            // Log error but continue cleanup
                            sessions_to_remove.push(session_id);
                        }
                    }
                }
            }

            // Remove expired sessions
            for session_id in sessions_to_remove {
                sessions.remove(&session_id);
            }

            // Update stats
            if cleanup_count > 0 {
                let mut stats_guard = stats.write().await;
                stats_guard.active_sessions = sessions.len();
                stats_guard.timeout_cleanups += cleanup_count;
                stats_guard.last_cleanup_at = Some(TokioInstant::now());
            }
        }
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        if let Some(handle) = self.cleanup_handle.take() {
            handle.abort();
        }
    }
}

/// Report from cleanup operation
#[derive(Debug, Clone, Default)]
pub struct CleanupReport {
    /// Number of sessions removed
    pub sessions_removed: usize,
    /// Number of sessions closed due to timeout
    pub timeout_cleanups: u64,
    /// Any errors encountered during cleanup
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::aggregates::stream_session::SessionConfig;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_session_manager_creation() {
        let manager = SessionManager::new();
        let stats = manager.stats().await;

        assert_eq!(stats.active_sessions, 0);
        assert_eq!(stats.timeout_cleanups, 0);
    }

    #[tokio::test]
    async fn test_add_and_remove_session() {
        let manager = SessionManager::new();
        let session = StreamSession::new(SessionConfig::default());
        let session_id = session.id();

        // Add session
        let added_id = manager.add_session(session).await.unwrap();
        assert_eq!(added_id, session_id);

        let stats = manager.stats().await;
        assert_eq!(stats.active_sessions, 1);

        // Get session
        let retrieved = manager.get_session(&session_id).await;
        assert!(retrieved.is_some());

        // Remove session
        let removed = manager.remove_session(&session_id).await.unwrap();
        assert!(removed);

        let stats = manager.stats().await;
        assert_eq!(stats.active_sessions, 0);
    }

    #[tokio::test]
    async fn test_cleanup_expired_sessions() {
        let manager = SessionManager::new();

        // Create session with very short timeout
        let mut session_config = SessionConfig::default();
        session_config.session_timeout_seconds = 1; // 1 second

        let session = StreamSession::new(session_config);
        let _session_id = session.id();

        manager.add_session(session).await.unwrap();

        // Wait for expiration
        sleep(Duration::from_secs(2)).await;

        // Cleanup expired sessions
        let report = manager.cleanup_expired_sessions().await.unwrap();

        assert_eq!(report.sessions_removed, 1);
        assert_eq!(report.timeout_cleanups, 1);
        assert!(report.errors.is_empty());

        let stats = manager.stats().await;
        assert_eq!(stats.active_sessions, 0);
    }

    #[tokio::test]
    async fn test_session_manager_automatic_cleanup() {
        let config = SessionManagerConfig {
            cleanup_interval_seconds: 1, // Clean every second
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);
        manager.start().await.unwrap();

        // Create expired session
        let mut session_config = SessionConfig::default();
        session_config.session_timeout_seconds = 1;

        let session = StreamSession::new(session_config);
        manager.add_session(session).await.unwrap();

        // Wait for automatic cleanup
        sleep(Duration::from_secs(3)).await;

        let stats = manager.stats().await;
        assert_eq!(stats.active_sessions, 0);
        assert!(stats.timeout_cleanups > 0);

        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_session_capacity_limit() {
        let config = SessionManagerConfig {
            max_sessions: 2,
            ..Default::default()
        };

        let manager = SessionManager::with_config(config);

        // Add sessions up to limit
        for _ in 0..2 {
            let session = StreamSession::new(SessionConfig::default());
            manager.add_session(session).await.unwrap();
        }

        // Try to add one more (should fail)
        let session = StreamSession::new(SessionConfig::default());
        let result = manager.add_session(session).await;
        assert!(result.is_err());
    }
}
