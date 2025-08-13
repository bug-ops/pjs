//! WebSocket transport layer for real-time PJS streaming
//!
//! Provides WebSocket-based streaming with progressive JSON delivery
//! and backpressure handling for optimal client performance.

use crate::{
    StreamFrame, PriorityStreamer,
    Error as PjsError, Result as PjsResult,
    domain::Priority,
    security::{RateLimitGuard, RateLimitError},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use sha2::{Sha256, Digest};

#[cfg(feature = "websocket-client")]
pub mod client;
#[cfg(feature = "http-server")]
pub mod server;
pub mod security;

#[cfg(feature = "websocket-client")]
pub use client::*;
#[cfg(feature = "http-server")]
pub use server::*;
pub use security::SecureWebSocketHandler;

/// WebSocket message types for PJS streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WsMessage {
    /// Stream initialization request
    StreamInit {
        session_id: String,
        data: Value,
        options: StreamOptions,
    },
    /// Stream frame with priority data
    StreamFrame {
        session_id: String,
        frame_id: u32,
        priority: u8,
        payload: Value,
        is_complete: bool,
    },
    /// Client acknowledgment of frame
    FrameAck {
        session_id: String,
        frame_id: u32,
        processing_time_ms: u64,
    },
    /// Stream completion signal
    StreamComplete {
        session_id: String,
        checksum: String,
    },
    /// Error message
    Error {
        session_id: Option<String>,
        error: String,
        code: u16,
    },
    /// Heartbeat/ping message
    Ping { timestamp: u64 },
    /// Heartbeat/pong response
    Pong { timestamp: u64 },
}

/// Stream configuration options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamOptions {
    /// Maximum frame size in bytes
    pub max_frame_size: usize,
    /// Client processing capability (frames per second)
    pub client_fps: Option<u32>,
    /// Enable compression
    pub compression: bool,
    /// Custom priority mapping
    pub priority_mapping: Option<HashMap<String, u8>>,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            max_frame_size: 64 * 1024, // 64KB
            client_fps: None,           // Auto-detect
            compression: true,
            priority_mapping: None,
        }
    }
}

/// WebSocket streaming session state
#[derive(Debug)]
pub struct StreamSession {
    pub id: String,
    pub created_at: Instant,
    pub options: StreamOptions,
    pub plan: Vec<StreamFrame>, // Simplified for now
    pub current_frame: u32,
    pub acknowledged_frames: Vec<u32>,
    pub client_metrics: ClientMetrics,
    pub rate_limit_guard: Option<RateLimitGuard>,
}

/// Client performance metrics for adaptive streaming
#[derive(Debug, Default)]
pub struct ClientMetrics {
    pub average_processing_time_ms: f64,
    pub frames_acknowledged: u32,
    pub last_ack_time: Option<Instant>,
    pub estimated_bandwidth_kbps: Option<f64>,
    pub connection_rtt_ms: Option<u64>,
}

impl ClientMetrics {
    pub fn update_processing_time(&mut self, processing_time_ms: u64) {
        let new_time = processing_time_ms as f64;
        if self.frames_acknowledged == 0 {
            self.average_processing_time_ms = new_time;
        } else {
            // Exponential moving average
            let alpha = 0.3;
            self.average_processing_time_ms = 
                alpha * new_time + (1.0 - alpha) * self.average_processing_time_ms;
        }
        self.frames_acknowledged += 1;
        self.last_ack_time = Some(Instant::now());
    }

    pub fn is_client_slow(&self) -> bool {
        self.average_processing_time_ms > 100.0 // > 100ms per frame
    }

    pub fn recommended_frame_delay(&self) -> Duration {
        if self.is_client_slow() {
            Duration::from_millis((self.average_processing_time_ms * 0.5) as u64)
        } else {
            Duration::from_millis(10) // Fast clients get minimal delay
        }
    }
}

/// WebSocket transport trait for different implementations
#[async_trait]
pub trait WebSocketTransport: Send + Sync {
    type Connection: Send + Sync;
    
    /// Start streaming session
    async fn start_stream(
        &self,
        connection: Arc<Self::Connection>,
        data: Value,
        options: StreamOptions,
    ) -> PjsResult<String>;
    
    /// Send frame to client
    async fn send_frame(
        &self,
        connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> PjsResult<()>;
    
    /// Handle incoming message
    async fn handle_message(
        &self,
        connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> PjsResult<()>;
    
    /// Close streaming session
    async fn close_stream(&self, session_id: &str) -> PjsResult<()>;
}

/// Adaptive streaming controller
pub struct AdaptiveStreamController {
    sessions: Arc<RwLock<HashMap<String, StreamSession>>>,
    streamer: PriorityStreamer,
    frame_tx: broadcast::Sender<(String, WsMessage)>,
}

impl AdaptiveStreamController {
    pub fn new() -> Self {
        let (frame_tx, _) = broadcast::channel(1000);
        
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            streamer: PriorityStreamer::new(),
            frame_tx,
        }
    }
    
    /// Create new streaming session
    pub async fn create_session(
        &self,
        data: Value,
        options: StreamOptions,
    ) -> PjsResult<String> {
        let session_id = Uuid::new_v4().to_string();
        let plan = vec![StreamFrame {
            data: data.clone(),
            priority: Priority::HIGH,
            metadata: std::collections::HashMap::new(),
        }]; // Simplified for now
        
        let session = StreamSession {
            id: session_id.clone(),
            created_at: Instant::now(),
            options,
            plan,
            current_frame: 0,
            acknowledged_frames: Vec::new(),
            client_metrics: ClientMetrics::default(),
            rate_limit_guard: None, // Will be set when connection is established
        };
        
        self.sessions.write().await.insert(session_id.clone(), session);
        
        info!("Created streaming session: {}", session_id);
        Ok(session_id)
    }
    
    /// Start streaming frames for session
    pub async fn start_streaming(&self, session_id: &str) -> PjsResult<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;
        
        // Start streaming task
        let session_id = session_id.to_string();
        let frame_tx = self.frame_tx.clone();
        let plan = session.plan.clone();
        
        tokio::spawn(async move {
            if let Err(e) = Self::stream_frames(session_id, plan, frame_tx).await {
                error!("Error streaming frames: {}", e);
            }
        });
        
        Ok(())
    }
    
    async fn stream_frames(
        session_id: String,
        plan: Vec<StreamFrame>, // Simplified for now
        frame_tx: broadcast::Sender<(String, WsMessage)>,
    ) -> Result<(), PjsError> {
        let mut frames_data = Vec::new();
        
        for (frame_id, frame) in plan.iter().enumerate() {
            // Collect frame payload for checksum calculation
            let payload_bytes = serde_json::to_vec(&frame.data).map_err(|e| PjsError::Other(e.to_string()))?;
            frames_data.push(payload_bytes);
            
            let ws_message = WsMessage::StreamFrame {
                session_id: session_id.clone(),
                frame_id: frame_id as u32,
                priority: frame.priority.value(),
                payload: frame.data.clone(),
                is_complete: frame_id == (plan.len() - 1),
            };
            
            if let Err(e) = frame_tx.send((session_id.clone(), ws_message)) {
                error!("Failed to send frame {}: {}", frame_id, e);
                break;
            }
            
            
            // TODO: Add adaptive delay based on client metrics
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        
        // Send completion message with calculated checksum
        let complete_message = WsMessage::StreamComplete {
            session_id: session_id.clone(),
            checksum: calculate_stream_checksum(&frames_data),
        };
        
        let _ = frame_tx.send((session_id, complete_message));
        Ok(())
    }
    
    /// Handle frame acknowledgment
    pub async fn handle_frame_ack(
        &self,
        session_id: &str,
        frame_id: u32,
        processing_time_ms: u64,
    ) -> PjsResult<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;
        
        session.acknowledged_frames.push(frame_id);
        session.client_metrics.update_processing_time(processing_time_ms);
        
        debug!(
            "Frame {} acknowledged for session {} (processing: {}ms, avg: {:.1}ms)",
            frame_id,
            session_id,
            processing_time_ms,
            session.client_metrics.average_processing_time_ms
        );
        
        if session.client_metrics.is_client_slow() {
            warn!(
                "Client {} is processing slowly (avg: {:.1}ms)",
                session_id,
                session.client_metrics.average_processing_time_ms
            );
        }
        
        Ok(())
    }
    
    /// Get subscriber for frame events
    pub fn subscribe_frames(&self) -> broadcast::Receiver<(String, WsMessage)> {
        self.frame_tx.subscribe()
    }
    
    /// Set rate limit guard for a session
    pub async fn set_rate_limit_guard(&self, session_id: &str, guard: RateLimitGuard) -> PjsResult<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(session_id)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;
        
        session.rate_limit_guard = Some(guard);
        Ok(())
    }
    
    /// Validate message against rate limits
    pub async fn validate_message(&self, session_id: &str, frame_size: usize) -> PjsResult<()> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(session_id)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;
            
        if let Some(guard) = &session.rate_limit_guard {
            guard.check_message(frame_size)
                .map_err(|e| PjsError::SecurityError(format!("Rate limit violation: {}", e)))?;
        }
        
        Ok(())
    }

    /// Clean up expired sessions
    pub async fn cleanup_expired_sessions(&self, max_age: Duration) {
        let mut sessions = self.sessions.write().await;
        let now = Instant::now();
        
        sessions.retain(|id, session| {
            let expired = now.duration_since(session.created_at) > max_age;
            if expired {
                info!("Cleaning up expired session: {}", id);
            }
            !expired
        });
    }
}

impl Default for AdaptiveStreamController {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate SHA-256 checksum for stream completion verification
fn calculate_stream_checksum(frames_data: &[Vec<u8>]) -> String {
    let mut hasher = Sha256::new();
    
    // Hash each frame's data
    for frame_data in frames_data {
        hasher.update(frame_data);
    }
    
    // Hash frame count to ensure integrity
    hasher.update((frames_data.len() as u64).to_le_bytes());
    
    let result = hasher.finalize();
    format!("sha256:{result:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_create_session() {
        let controller = AdaptiveStreamController::new();
        let data = json!({
            "critical": {"id": 1, "status": "active"},
            "details": {"name": "test", "description": "test data"}
        });
        
        let session_id = controller
            .create_session(data, StreamOptions::default())
            .await
            .unwrap();
        
        assert!(!session_id.is_empty());
        
        let sessions = controller.sessions.read().await;
        assert!(sessions.contains_key(&session_id));
    }

    #[tokio::test]
    async fn test_frame_acknowledgment() {
        let controller = AdaptiveStreamController::new();
        let data = json!({"test": "data"});
        
        let session_id = controller
            .create_session(data, StreamOptions::default())
            .await
            .unwrap();
        
        controller
            .handle_frame_ack(&session_id, 0, 50)
            .await
            .unwrap();
        
        let sessions = controller.sessions.read().await;
        let session = sessions.get(&session_id).unwrap();
        assert_eq!(session.acknowledged_frames, vec![0]);
        assert_eq!(session.client_metrics.average_processing_time_ms, 50.0);
    }

    #[test]
    fn test_client_metrics() {
        let mut metrics = ClientMetrics::default();
        
        metrics.update_processing_time(100);
        assert_eq!(metrics.average_processing_time_ms, 100.0);
        
        metrics.update_processing_time(200);
        // Should be exponential moving average: 0.3 * 200 + 0.7 * 100 = 130
        assert!((metrics.average_processing_time_ms - 130.0).abs() < 0.1);
        
        assert!(metrics.is_client_slow());
    }

    #[test]
    fn test_checksum_calculation() {
        // Test empty frames
        let empty_frames: Vec<Vec<u8>> = vec![];
        let checksum = calculate_stream_checksum(&empty_frames);
        assert!(checksum.starts_with("sha256:"));
        
        // Test single frame
        let single_frame = vec![vec![1, 2, 3, 4]];
        let checksum1 = calculate_stream_checksum(&single_frame);
        assert!(checksum1.starts_with("sha256:"));
        
        // Test multiple frames
        let multi_frames = vec![vec![1, 2], vec![3, 4], vec![5, 6]];
        let checksum2 = calculate_stream_checksum(&multi_frames);
        assert!(checksum2.starts_with("sha256:"));
        
        // Same data should produce same checksum
        let same_frames = vec![vec![1, 2], vec![3, 4], vec![5, 6]];
        let checksum3 = calculate_stream_checksum(&same_frames);
        assert_eq!(checksum2, checksum3);
        
        // Different data should produce different checksum
        let diff_frames = vec![vec![1, 2], vec![3, 4], vec![5, 7]]; // Last byte different
        let checksum4 = calculate_stream_checksum(&diff_frames);
        assert_ne!(checksum2, checksum4);
        
        // Different order should produce different checksum
        let reordered_frames = vec![vec![3, 4], vec![1, 2], vec![5, 6]];
        let checksum5 = calculate_stream_checksum(&reordered_frames);
        assert_ne!(checksum2, checksum5);
    }
}