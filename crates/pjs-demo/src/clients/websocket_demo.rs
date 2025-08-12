//! WebSocket client demonstration for PJS streaming
//!
//! This client connects to WebSocket streaming servers and demonstrates
//! real-time PJS frame reconstruction and visualization.

use futures::{SinkExt, StreamExt};
use pjson_rs::{
    compression::{CompressionStrategy, SchemaCompressor},
    domain::value_objects::{Priority, SessionId},
    ApplicationResult, JsonReconstructor,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{error, info, warn};
use url::Url;

/// WebSocket client configuration
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Server WebSocket URL
    pub server_url: String,
    /// Session ID for connection
    pub session_id: Option<String>,
    /// Enable frame decompression
    pub enable_decompression: bool,
    /// Display progress updates
    pub show_progress: bool,
    /// Save received data to file
    pub output_file: Option<String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: "ws://127.0.0.1:3001/ws".to_string(),
            session_id: None,
            enable_decompression: true,
            show_progress: true,
            output_file: None,
        }
    }
}

/// PJS frame received from WebSocket
#[derive(Debug, Clone, Deserialize)]
pub struct PjsFrame {
    #[serde(rename = "@type")]
    pub frame_type: String,
    #[serde(rename = "@session_id")]
    pub session_id: String,
    #[serde(rename = "@frame_index")]
    pub frame_index: Option<usize>,
    #[serde(rename = "@priority")]
    pub priority: Option<u8>,
    #[serde(rename = "@compressed")]
    pub compressed: Option<bool>,
    #[serde(rename = "@timestamp")]
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub data: JsonValue,
}

/// Stream completion message
#[derive(Debug, Clone, Deserialize)]
pub struct StreamCompletion {
    #[serde(rename = "@type")]
    pub message_type: String,
    #[serde(rename = "@session_id")]
    pub session_id: String,
    #[serde(rename = "@total_frames")]
    pub total_frames: usize,
    #[serde(rename = "@timestamp")]
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Client statistics
#[derive(Debug, Clone)]
pub struct ClientStats {
    pub frames_received: usize,
    pub bytes_received: u64,
    pub start_time: Instant,
    pub first_frame_time: Option<Instant>,
    pub completion_time: Option<Instant>,
    pub priority_distribution: HashMap<u8, usize>,
    pub decompression_errors: usize,
}

impl ClientStats {
    pub fn new() -> Self {
        Self {
            frames_received: 0,
            bytes_received: 0,
            start_time: Instant::now(),
            first_frame_time: None,
            completion_time: None,
            priority_distribution: HashMap::new(),
            decompression_errors: 0,
        }
    }

    pub fn record_frame(&mut self, frame: &PjsFrame, bytes: usize) {
        self.frames_received += 1;
        self.bytes_received += bytes as u64;

        if self.first_frame_time.is_none() {
            self.first_frame_time = Some(Instant::now());
        }

        if let Some(priority) = frame.priority {
            *self.priority_distribution.entry(priority).or_insert(0) += 1;
        }
    }

    pub fn time_to_first_frame_ms(&self) -> Option<f64> {
        self.first_frame_time.map(|t| t.duration_since(self.start_time).as_secs_f64() * 1000.0)
    }

    pub fn total_duration_ms(&self) -> f64 {
        let end_time = self.completion_time.unwrap_or_else(Instant::now);
        end_time.duration_since(self.start_time).as_secs_f64() * 1000.0
    }
}

/// JSON reconstructor for building complete objects from PJS frames
#[derive(Debug)]
pub struct DemoJsonReconstructor {
    /// Partial JSON state being built
    state: JsonValue,
    /// Frames received so far
    frames_received: Vec<PjsFrame>,
    /// Decompressor for compressed frames
    decompressor: SchemaCompressor,
}

impl DemoJsonReconstructor {
    pub fn new() -> Self {
        Self {
            state: JsonValue::Null,
            frames_received: Vec::new(),
            decompressor: SchemaCompressor::new(),
        }
    }

    /// Add a new frame and update the reconstructed JSON
    pub fn add_frame(&mut self, frame: PjsFrame) -> ApplicationResult<()> {
        // Decompress frame if needed
        let frame_data = if frame.compressed.unwrap_or(false) {
            match self.decompressor.decompress(&frame.data) {
                Ok(decompressed) => decompressed,
                Err(e) => {
                    warn!("Failed to decompress frame: {}", e);
                    frame.data.clone()
                }
            }
        } else {
            frame.data.clone()
        };

        // Apply frame data to current state
        self.apply_frame_data(frame_data)?;
        
        self.frames_received.push(frame);
        Ok(())
    }

    /// Apply frame data to current JSON state
    fn apply_frame_data(&mut self, data: JsonValue) -> ApplicationResult<()> {
        match &mut self.state {
            JsonValue::Null => {
                // First frame - initialize state
                self.state = data;
            }
            JsonValue::Object(obj) => {
                // Merge with existing object
                if let JsonValue::Object(new_obj) = data {
                    for (key, value) in new_obj {
                        obj.insert(key, value);
                    }
                }
            }
            JsonValue::Array(arr) => {
                // Append to existing array
                if let JsonValue::Array(new_arr) = data {
                    arr.extend(new_arr);
                } else {
                    arr.push(data);
                }
            }
            _ => {
                // Replace primitive values
                self.state = data;
            }
        }
        Ok(())
    }

    /// Get current reconstructed JSON state
    pub fn current_state(&self) -> &JsonValue {
        &self.state
    }

    /// Get number of frames processed
    pub fn frame_count(&self) -> usize {
        self.frames_received.len()
    }
}

/// WebSocket client for PJS streaming
pub struct WebSocketClient {
    config: ClientConfig,
    stats: ClientStats,
    reconstructor: DemoJsonReconstructor,
}

impl WebSocketClient {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config,
            stats: ClientStats::new(),
            reconstructor: DemoDemoJsonReconstructor::new(),
        }
    }

    /// Connect to WebSocket server and start receiving frames
    pub async fn connect_and_stream(&mut self) -> ApplicationResult<()> {
        info!("Connecting to WebSocket server: {}", self.config.server_url);

        // Parse WebSocket URL
        let url = Url::parse(&self.config.server_url)
            .map_err(|e| format!("Invalid WebSocket URL: {}", e))?;

        // Connect to WebSocket server
        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|e| format!("WebSocket connection failed: {}", e))?;

        info!("WebSocket connection established");

        let (mut write, mut read) = ws_stream.split();

        // Send initial configuration if needed
        if let Some(session_id) = &self.config.session_id {
            let config_message = serde_json::json!({
                "type": "client_config",
                "session_id": session_id,
                "enable_decompression": self.config.enable_decompression
            });

            let message_text = serde_json::to_string(&config_message)
                .map_err(|e| format!("Failed to serialize config: {}", e))?;

            write.send(Message::Text(message_text)).await
                .map_err(|e| format!("Failed to send config: {}", e))?;
        }

        // Main message processing loop
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Err(e) = self.handle_text_message(text).await {
                        error!("Failed to handle message: {}", e);
                        break;
                    }
                }
                Ok(Message::Binary(data)) => {
                    warn!("Received unexpected binary message ({} bytes)", data.len());
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket connection closed by server");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        self.stats.completion_time = Some(Instant::now());
        self.print_final_stats();

        Ok(())
    }

    /// Handle incoming text message
    async fn handle_text_message(&mut self, text: String) -> ApplicationResult<()> {
        // Try to parse as PJS frame first
        if let Ok(frame) = serde_json::from_str::<PjsFrame>(&text) {
            match frame.frame_type.as_str() {
                "pjs_frame" => {
                    self.handle_pjs_frame(frame, text.len()).await?;
                }
                "stream_complete" => {
                    if let Ok(completion) = serde_json::from_str::<StreamCompletion>(&text) {
                        self.handle_stream_completion(completion).await?;
                    }
                }
                _ => {
                    warn!("Unknown frame type: {}", frame.frame_type);
                }
            }
        } else {
            warn!("Failed to parse message as PJS frame: {}", text);
        }

        Ok(())
    }

    /// Handle PJS frame
    async fn handle_pjs_frame(&mut self, frame: PjsFrame, message_size: usize) -> ApplicationResult<()> {
        // Record statistics
        self.stats.record_frame(&frame, message_size);

        // Add frame to reconstructor
        if let Err(e) = self.reconstructor.add_frame(frame.clone()) {
            self.stats.decompression_errors += 1;
            warn!("Failed to add frame to reconstructor: {}", e);
        }

        // Show progress if enabled
        if self.config.show_progress {
            let priority_str = frame.priority.map(|p| format!("{}", p)).unwrap_or_else(|| "?".to_string());
            info!(
                "Received frame {}: priority={}, bytes={}, total_frames={}",
                frame.frame_index.unwrap_or(0),
                priority_str,
                message_size,
                self.stats.frames_received
            );
        }

        Ok(())
    }

    /// Handle stream completion
    async fn handle_stream_completion(&mut self, completion: StreamCompletion) -> ApplicationResult<()> {
        self.stats.completion_time = Some(Instant::now());
        
        info!(
            "Stream completed: {} frames received out of {} total",
            self.stats.frames_received,
            completion.total_frames
        );

        // Save reconstructed data to file if specified
        if let Some(output_file) = &self.config.output_file {
            self.save_to_file(output_file).await?;
        }

        Ok(())
    }

    /// Save reconstructed JSON to file
    async fn save_to_file(&self, filename: &str) -> ApplicationResult<()> {
        let json_str = serde_json::to_string_pretty(self.reconstructor.current_state())
            .map_err(|e| format!("Failed to serialize JSON: {}", e))?;

        tokio::fs::write(filename, json_str).await
            .map_err(|e| format!("Failed to write file {}: {}", filename, e))?;

        info!("Reconstructed JSON saved to: {}", filename);
        Ok(())
    }

    /// Print final statistics
    fn print_final_stats(&self) {
        info!("=== WebSocket Client Statistics ===");
        info!("Frames received: {}", self.stats.frames_received);
        info!("Bytes received: {}", self.stats.bytes_received);
        
        if let Some(ttff) = self.stats.time_to_first_frame_ms() {
            info!("Time to first frame: {:.2} ms", ttff);
        }
        
        info!("Total duration: {:.2} ms", self.stats.total_duration_ms());
        info!("Decompression errors: {}", self.stats.decompression_errors);
        info!("Reconstructed objects: {}", self.reconstructor.frame_count());
        
        // Print priority distribution
        if !self.stats.priority_distribution.is_empty() {
            info!("Priority distribution:");
            let mut priorities: Vec<_> = self.stats.priority_distribution.iter().collect();
            priorities.sort_by_key(|(priority, _)| *priority);
            priorities.reverse(); // Show highest priority first
            
            for (priority, count) in priorities {
                info!("  Priority {}: {} frames", priority, count);
            }
        }
    }
}

/// CLI interface for WebSocket client demo
#[tokio::main]
async fn main() -> ApplicationResult<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    let config = if args.len() > 1 {
        ClientConfig {
            server_url: args[1].clone(),
            session_id: args.get(2).cloned(),
            enable_decompression: true,
            show_progress: true,
            output_file: args.get(3).cloned(),
        }
    } else {
        ClientConfig::default()
    };

    // Print usage information
    if args.len() == 1 {
        info!("Usage: {} <websocket_url> [session_id] [output_file]", args[0]);
        info!("Example: {} ws://127.0.0.1:3001/ws/session123 session123 output.json", args[0]);
        info!("Using default configuration...");
    }

    // Create and run client
    let mut client = WebSocketClient::new(config);
    
    info!("Starting WebSocket client demo...");
    
    match client.connect_and_stream().await {
        Ok(_) => {
            info!("WebSocket client demo completed successfully");
        }
        Err(e) => {
            error!("WebSocket client error: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}