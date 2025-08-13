//! WebSocket streaming server demonstrating real-time PJS delivery
//!
//! This server provides WebSocket endpoints for streaming PJS data
//! with priority-based frame delivery and compression support.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use pjson_rs::{
    compression::SchemaCompressor,
    domain::value_objects::SessionId,
    ApplicationResult, ApplicationError, DomainResult, DomainError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::time::sleep;
use tracing::{error, info, warn};

/// WebSocket streaming server state
#[derive(Clone)]
pub struct AppState {
    active_sessions: Arc<Mutex<HashMap<SessionId, SessionInfo>>>,
    compressor: Arc<SchemaCompressor>,
}

/// Information about active streaming session
#[derive(Debug, Clone)]
struct SessionInfo {
    #[allow(dead_code)] // TODO: Use in session management
    session_id: SessionId,
    #[allow(dead_code)] // TODO: Use for session analytics
    started_at: Instant,
    frame_count: usize,
    total_bytes: u64,
}

/// WebSocket streaming configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Priority threshold (0-255)
    priority_threshold: Option<u8>,
    /// Enable compression
    enable_compression: Option<bool>,
    /// Compression strategy
    compression_strategy: Option<String>,
    /// Frame delay in milliseconds
    frame_delay_ms: Option<u64>,
    /// Maximum frames to send
    max_frames: Option<usize>,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            priority_threshold: Some(50),
            enable_compression: Some(true),
            compression_strategy: Some("adaptive".to_string()),
            frame_delay_ms: Some(10),
            max_frames: Some(1000),
        }
    }
}

/// Streaming request payload
#[derive(Debug, Deserialize)]
pub struct StreamRequest {
    data_type: String,
    config: Option<StreamConfig>,
}

/// Streaming response with session info
#[derive(Debug, Serialize)]
pub struct StreamResponse {
    session_id: String,
    websocket_url: String,
    config: StreamConfig,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
            compressor: Arc::new(SchemaCompressor::new()),
        }
    }

    fn add_session(&self, session_id: SessionId) {
        let mut sessions = self.active_sessions.lock().unwrap();
        sessions.insert(
            session_id,
            SessionInfo {
                session_id,
                started_at: Instant::now(),
                frame_count: 0,
                total_bytes: 0,
            },
        );
    }

    fn remove_session(&self, session_id: &SessionId) {
        let mut sessions = self.active_sessions.lock().unwrap();
        sessions.remove(session_id);
    }

    fn update_session_stats(&self, session_id: &SessionId, bytes_sent: u64) {
        let mut sessions = self.active_sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(session_id) {
            session.frame_count += 1;
            session.total_bytes += bytes_sent;
        }
    }
}

/// Main WebSocket streaming server
#[tokio::main]
async fn main() -> ApplicationResult<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let app_state = AppState::new();

    let app = Router::new()
        .route("/", get(index_page))
        .route("/stream", post(create_stream))
        .route("/ws", get(|ws: WebSocketUpgrade, state: State<AppState>| async { ws.on_upgrade(move |socket| handle_websocket(socket, state.0)) }))
        .route("/health", get(health_check))
        .route("/metrics", get(metrics))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001")
        .await
        .map_err(|e| ApplicationError::from(DomainError::Logic(format!("Failed to bind to address: {e}"))))?;

    info!("WebSocket streaming server running on http://127.0.0.1:3001");
    info!("WebSocket endpoint: ws://127.0.0.1:3001/ws/{{session_id}}");

    axum::serve(listener, app)
        .await
        .map_err(|e| ApplicationError::from(DomainError::Logic(format!("Server error: {e}"))))?;

    Ok(())
}

/// Serve the main HTML page
async fn index_page() -> Html<&'static str> {
    Html(include_str!("../static/performance_comparison.html"))
}

/// Create new streaming session
async fn create_stream(
    State(state): State<AppState>,
    Json(request): Json<StreamRequest>,
) -> Result<Json<StreamResponse>, StatusCode> {
    let session_id = SessionId::new();
    let config = request.config.unwrap_or_default();

    state.add_session(session_id);

    info!(
        "Created streaming session {} for data type: {}",
        session_id, request.data_type
    );

    let response = StreamResponse {
        session_id: session_id.to_string(),
        websocket_url: format!("ws://127.0.0.1:3001/ws/{session_id}"),
        config,
    };

    Ok(Json(response))
}


/// Handle WebSocket connection
async fn handle_websocket(socket: WebSocket, state: AppState) {
    let session_id = SessionId::new();

    info!("WebSocket connection established for session: {}", session_id);

    let (mut sender, mut receiver) = socket.split();

    // Start streaming task
    let streaming_state = state.clone();
    let streaming_session_id = session_id;
    let streaming_task = tokio::spawn(async move {
        if let Err(e) = stream_data(&mut sender, streaming_state, streaming_session_id).await {
            error!("Streaming error for session {}: {}", streaming_session_id, e);
        }
    });

    // Handle incoming messages
    let receive_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    info!("Received message: {}", text);
                    // TODO: Handle client commands (pause, resume, config changes)
                }
                Ok(Message::Binary(_)) => {
                    warn!("Received unexpected binary message");
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket connection closed by client");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = streaming_task => {
            info!("Streaming completed for session: {}", session_id);
        }
        _ = receive_task => {
            info!("Receive task completed for session: {}", session_id);
        }
    }

    state.remove_session(&session_id);
}

/// Stream PJS data through WebSocket
async fn stream_data(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    state: AppState,
    session_id: SessionId,
) -> DomainResult<()> {
    // Generate sample data
    let sample_data = generate_sample_streaming_data();

    // Create simple frames from sample data for demonstration
    let frames = create_demo_frames(&sample_data)?;

    info!(
        "Streaming {} frames for session: {}",
        frames.len(),
        session_id
    );

    // Send frames in priority order
    for (index, frame) in frames.iter().enumerate() {
        // Apply compression if enabled
        let compressed_data = if should_compress(frame) {
            match state.compressor.compress(&frame.data) {
                Ok(compressed) => compressed.data,
                Err(e) => {
                    warn!("Compression failed: {}, using original data", e);
                    frame.data.clone()
                }
            }
        } else {
            frame.data.clone()
        };

        // Create WebSocket message
        let message_data = serde_json::json!({
            "@type": "pjs_frame",
            "@session_id": session_id.to_string(),
            "@frame_index": index,
            "@priority": frame.priority.value(),
            "@compressed": should_compress(frame),
            "@timestamp": chrono::Utc::now(),
            "data": compressed_data
        });

        let message_text = serde_json::to_string(&message_data)
            .map_err(|e| format!("Failed to serialize frame: {e}"))?;

        // Send frame
        if let Err(e) = sender.send(Message::Text(message_text.clone().into())).await {
            error!("Failed to send frame: {}", e);
            break;
        }

        // Update session statistics
        state.update_session_stats(&session_id, message_text.len() as u64);

        // Add delay between frames for demonstration
        sleep(Duration::from_millis(10)).await;

        info!(
            "Sent frame {} with priority {} for session: {}",
            index,
            frame.priority.value(),
            session_id
        );
    }

    // Send completion signal
    let completion_message = serde_json::json!({
        "@type": "stream_complete",
        "@session_id": session_id.to_string(),
        "@total_frames": frames.len(),
        "@timestamp": chrono::Utc::now()
    });

    if let Ok(completion_text) = serde_json::to_string(&completion_message) {
        let _ = sender.send(Message::Text(completion_text.into())).await;
    }

    info!("Streaming completed for session: {}", session_id);
    Ok(())
}

/// Create demo frames from JSON data
fn create_demo_frames(data: &JsonValue) -> DomainResult<Vec<pjson_rs::stream::StreamFrame>> {
    use pjson_rs::domain::Priority;
    use std::collections::HashMap;
    
    let mut frames = Vec::new();
    
    if let JsonValue::Object(obj) = data {
        for (key, value) in obj {
            let priority = match key.as_str() {
                "title" | "last_updated" => Priority::new(255).unwrap_or_else(|_| Priority::new(128).unwrap()), // Critical
                "critical_metrics" => Priority::new(200).unwrap_or_else(|_| Priority::new(128).unwrap()),        // High
                "user_activity" => Priority::new(150).unwrap_or_else(|_| Priority::new(128).unwrap()),          // Medium-high
                "system_stats" => Priority::new(100).unwrap_or_else(|_| Priority::new(128).unwrap()),           // Medium
                _ => Priority::new(50).unwrap_or_else(|_| Priority::new(128).unwrap()),                         // Low
            };
            
            let frame = pjson_rs::stream::StreamFrame {
                data: value.clone(),
                priority,
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert("field_name".to_string(), key.clone());
                    meta.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());
                    meta
                },
            };
            
            frames.push(frame);
        }
    }
    
    // Sort by priority (highest first)
    frames.sort_by(|a, b| b.priority.value().cmp(&a.priority.value()));
    
    Ok(frames)
}

/// Check if frame should be compressed
fn should_compress(frame: &pjson_rs::stream::StreamFrame) -> bool {
    // Compress frames with lower priority or larger payloads
    frame.priority.value() < 128 || 
    serde_json::to_string(&frame.data).unwrap_or_default().len() > 1000
}

/// Generate sample data for streaming demonstration
fn generate_sample_streaming_data() -> JsonValue {
    use pjs_demo::data::{analytics, ecommerce, social, DatasetSize};

    serde_json::json!({
        "dashboard": {
            "title": "Real-time Analytics Dashboard",
            "last_updated": chrono::Utc::now(),
            "analytics_data": analytics::generate_realtime_data(DatasetSize::Medium),
            "ecommerce_data": ecommerce::generate_ecommerce_data(DatasetSize::Medium),
            "social_data": social::generate_social_data(DatasetSize::Medium),
            "summary": {
                "total_users": 12345,
                "active_sessions": 856,
                "revenue_today": 45000.50
            }
        }
    })
}

/// Health check endpoint
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "server": "websocket-streaming",
        "timestamp": chrono::Utc::now()
    }))
}

/// Metrics endpoint
async fn metrics(State(state): State<AppState>) -> Json<serde_json::Value> {
    let sessions = state.active_sessions.lock().unwrap();
    let session_count = sessions.len();
    let total_frames: usize = sessions.values().map(|s| s.frame_count).sum();
    let total_bytes: u64 = sessions.values().map(|s| s.total_bytes).sum();

    Json(serde_json::json!({
        "active_sessions": session_count,
        "total_frames_sent": total_frames,
        "total_bytes_sent": total_bytes,
        "server_uptime_seconds": 0, // TODO: Track actual uptime
        "timestamp": chrono::Utc::now()
    }))
}