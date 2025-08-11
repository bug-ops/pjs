//! Tokio-based adapters for streaming I/O operations
//!
//! These adapters implement the domain writer ports using Tokio's
//! async runtime and networking primitives.

use crate::domain::{
    DomainError, DomainResult,
    entities::Frame,
    ports::writer::{
        StreamWriter, FrameWriter, WriterMetrics, WriterConfig, 
        BackpressureStrategy, WriterFactory, ConnectionMonitor,
        ConnectionState, ConnectionMetrics
    },
};
use async_trait::async_trait;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::{mpsc, Mutex, Semaphore},
    time::timeout,
};

/// Tokio-based implementation of StreamWriter
pub struct TokioStreamWriter<W: AsyncWrite + Unpin + Send> {
    writer: Arc<Mutex<W>>,
    config: WriterConfig,
    metrics: Arc<Mutex<WriterMetrics>>,
    semaphore: Arc<Semaphore>,
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl<W: AsyncWrite + Unpin + Send> TokioStreamWriter<W> {
    pub fn new(writer: W, config: WriterConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.buffer_size));
        
        Self {
            writer: Arc::new(Mutex::new(writer)),
            config,
            metrics: Arc::new(Mutex::new(WriterMetrics::default())),
            semaphore,
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

#[async_trait]
impl<W: AsyncWrite + Unpin + Send + Sync + 'static> StreamWriter for TokioStreamWriter<W> {
    async fn write_frame(&mut self, frame: Frame) -> DomainResult<()> {
        if self.closed.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(DomainError::Io("Writer is closed".to_string()));
        }

        // Acquire semaphore permit for backpressure control
        let _permit = self.semaphore.acquire().await
            .map_err(|_| DomainError::Io("Semaphore closed".to_string()))?;

        let start_time = Instant::now();
        
        // Serialize frame to bytes
        let frame_bytes = self.serialize_frame(&frame)?;
        
        // Write with timeout
        let write_result = timeout(
            self.config.write_timeout,
            self.write_bytes(&frame_bytes)
        ).await;

        match write_result {
            Ok(Ok(())) => {
                // Update metrics
                let write_duration = start_time.elapsed();
                self.update_metrics(frame_bytes.len(), write_duration, false).await;
                Ok(())
            },
            Ok(Err(e)) => {
                self.update_metrics(0, Duration::ZERO, true).await;
                Err(e)
            },
            Err(_) => {
                self.update_metrics(0, Duration::ZERO, true).await;
                Err(DomainError::Io("Write timeout".to_string()))
            }
        }
    }

    async fn write_frames(&mut self, frames: Vec<Frame>) -> DomainResult<()> {
        if frames.is_empty() {
            return Ok(());
        }

        // Batch serialize all frames
        let mut batch_bytes = Vec::new();
        for frame in &frames {
            let frame_bytes = self.serialize_frame(frame)?;
            batch_bytes.extend_from_slice(&frame_bytes);
        }

        // Single write operation for better performance
        let start_time = Instant::now();
        let write_result = timeout(
            self.config.write_timeout,
            self.write_bytes(&batch_bytes)
        ).await;

        match write_result {
            Ok(Ok(())) => {
                let write_duration = start_time.elapsed();
                self.update_metrics(batch_bytes.len(), write_duration, false).await;
                Ok(())
            },
            Ok(Err(e)) => {
                self.update_metrics(0, Duration::ZERO, true).await;
                Err(e)
            },
            Err(_) => {
                self.update_metrics(0, Duration::ZERO, true).await;
                Err(DomainError::Io("Write timeout".to_string()))
            }
        }
    }

    async fn flush(&mut self) -> DomainResult<()> {
        let mut writer = self.writer.lock().await;
        writer.flush().await
            .map_err(|e| DomainError::Io(format!("Flush failed: {}", e)))
    }

    fn capacity(&self) -> Option<usize> {
        Some(self.config.buffer_size)
    }

    fn is_ready(&self) -> bool {
        !self.closed.load(std::sync::atomic::Ordering::Relaxed) &&
        self.semaphore.available_permits() > 0
    }

    async fn close(&mut self) -> DomainResult<()> {
        self.closed.store(true, std::sync::atomic::Ordering::Relaxed);
        self.flush().await
    }
}

impl<W: AsyncWrite + Unpin + Send> TokioStreamWriter<W> {
    fn serialize_frame(&self, frame: &Frame) -> DomainResult<Vec<u8>> {
        // Simple JSON serialization - could be optimized with binary formats
        serde_json::to_vec(frame)
            .map_err(|e| DomainError::Io(format!("Serialization failed: {}", e)))
    }

    async fn write_bytes(&self, bytes: &[u8]) -> DomainResult<()> {
        let mut writer = self.writer.lock().await;
        writer.write_all(bytes).await
            .map_err(|e| DomainError::Io(format!("Write failed: {}", e)))
    }

    async fn update_metrics(&self, bytes_written: usize, duration: Duration, error: bool) {
        {
            let mut metrics = self.metrics.lock().await;
            if error {
                metrics.error_count += 1;
            } else {
                metrics.frames_written += 1;
                metrics.bytes_written += bytes_written as u64;
                
                // Update rolling average of write latency
                let count = metrics.frames_written;
                let old_avg = metrics.avg_write_latency;
                metrics.avg_write_latency = Duration::from_nanos(
                    ((old_avg.as_nanos() as u64 * (count - 1)) + duration.as_nanos() as u64) / count
                );
            }
        }
    }
}

/// Tokio-based implementation of FrameWriter with priority support
pub struct TokioFrameWriter<W: AsyncWrite + Unpin + Send> {
    base_writer: TokioStreamWriter<W>,
    priority_queue: Arc<tokio::sync::Mutex<priority_queue::PriorityQueue<Frame, std::cmp::Reverse<u8>>>>,
    backpressure_threshold: usize,
    buffer_tx: mpsc::Sender<Frame>,
    buffer_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Frame>>>,
}

impl<W: AsyncWrite + Unpin + Send + Sync + 'static> TokioFrameWriter<W> {
    pub fn new(writer: W, config: WriterConfig) -> Self {
        let base_writer = TokioStreamWriter::new(writer, config.clone());
        let (buffer_tx, buffer_rx) = mpsc::channel(config.buffer_size);
        
        Self {
            base_writer,
            priority_queue: Arc::new(tokio::sync::Mutex::new(priority_queue::PriorityQueue::new())),
            backpressure_threshold: config.buffer_size / 2,
            buffer_tx,
            buffer_rx: Arc::new(tokio::sync::Mutex::new(buffer_rx)),
        }
    }
    
    async fn handle_backpressure(&self, frame: Frame) -> DomainResult<bool> {
        let queue = self.priority_queue.lock().await;
        if queue.len() >= self.backpressure_threshold {
                match self.base_writer.config.backpressure_strategy {
                    BackpressureStrategy::Block => {
                        // Will block until space available
                        Ok(false)
                    },
                    BackpressureStrategy::DropLowPriority => {
                        // Drop frame if priority is lower than threshold
                        let frame_priority = frame.priority().unwrap_or(0);
                        if frame_priority < 50 { // Medium priority threshold
                            return Ok(true); // Frame dropped
                        }
                        Ok(false)
                    },
                    BackpressureStrategy::DropOldest => {
                        // This would require additional logic to track frame age
                        Ok(false)
                    },
                    BackpressureStrategy::Error => {
                        return Err(DomainError::Io("Buffer full".to_string()));
                    }
                }
            } else {
                Ok(false)
        }
    }
}

#[async_trait]
impl<W: AsyncWrite + Unpin + Send + Sync + 'static> FrameWriter for TokioFrameWriter<W> {
    async fn write_prioritized_frame(&mut self, frame: Frame) -> DomainResult<()> {
        // Check for backpressure
        if self.handle_backpressure(frame.clone()).await? {
            // Frame was dropped due to backpressure
            {
                let mut metrics = self.base_writer.metrics.lock().await;
                metrics.frames_dropped += 1;
            }
            return Ok(());
        }

        // Add to priority queue
        {
            let mut queue = self.priority_queue.lock().await;
            let priority = frame.priority().unwrap_or(0);
            queue.push(frame, std::cmp::Reverse(priority));
        }

        // Process frames from queue
        self.process_priority_queue().await
    }

    async fn write_frames_by_priority(&mut self, frames: Vec<Frame>) -> DomainResult<()> {
        // Sort frames by priority and write in order
        let mut sorted_frames = frames;
        sorted_frames.sort_by(|a, b| {
            let priority_a = a.priority().unwrap_or(0);
            let priority_b = b.priority().unwrap_or(0);
            priority_b.cmp(&priority_a) // Higher priority first
        });

        for frame in sorted_frames {
            self.write_prioritized_frame(frame).await?;
        }
        
        Ok(())
    }

    async fn set_backpressure_threshold(&mut self, threshold: usize) -> DomainResult<()> {
        self.backpressure_threshold = threshold;
        Ok(())
    }

    async fn get_metrics(&self) -> DomainResult<WriterMetrics> {
        let metrics = self.base_writer.metrics.lock().await;
        let mut result = metrics.clone();
        
        // Add buffer size info
        {
            let queue = self.priority_queue.lock().await;
            result.buffer_size = queue.len();
        }
        
        Ok(result)
    }
}

impl<W: AsyncWrite + Unpin + Send + Sync + 'static> TokioFrameWriter<W> {
    async fn process_priority_queue(&mut self) -> DomainResult<()> {
        {
            let mut queue = self.priority_queue.lock().await;
            while let Some((frame, _priority)) = queue.pop() {
                // Use base writer to actually send the frame
                if let Err(e) = self.base_writer.write_frame(frame).await {
                    return Err(e);
                }
            }
        }
        Ok(())
    }
}

/// Factory for creating Tokio-based writers
pub struct TokioWriterFactory;

#[async_trait]
impl WriterFactory for TokioWriterFactory {
    async fn create_stream_writer(
        &self,
        _connection_id: &str,
        config: WriterConfig,
    ) -> DomainResult<Box<dyn StreamWriter>> {
        // For demo purposes, create a writer that writes to stdout
        // In real implementation, this would create appropriate network writers
        let writer = tokio::io::stdout();
        Ok(Box::new(TokioStreamWriter::new(writer, config)))
    }

    async fn create_frame_writer(
        &self,
        _connection_id: &str,
        config: WriterConfig,
    ) -> DomainResult<Box<dyn FrameWriter>> {
        // For demo purposes, create a writer that writes to stdout
        let writer = tokio::io::stdout();
        Ok(Box::new(TokioFrameWriter::new(writer, config)))
    }
}

/// Simple connection monitor implementation
pub struct TokioConnectionMonitor {
    connections: Arc<tokio::sync::Mutex<std::collections::HashMap<String, ConnectionState>>>,
}

impl TokioConnectionMonitor {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }
}

#[async_trait]
impl ConnectionMonitor for TokioConnectionMonitor {
    async fn get_connection_state(&self, connection_id: &str) -> DomainResult<ConnectionState> {
        let connections = self.connections.lock().await;
        Ok(connections.get(connection_id)
            .cloned()
            .unwrap_or(ConnectionState::Closed))
    }

    async fn is_connection_healthy(&self, connection_id: &str) -> DomainResult<bool> {
        let state = self.get_connection_state(connection_id).await?;
        Ok(matches!(state, ConnectionState::Active))
    }

    async fn get_connection_metrics(&self, _connection_id: &str) -> DomainResult<ConnectionMetrics> {
        // Return default metrics - would be implemented with real monitoring
        Ok(ConnectionMetrics::default())
    }

    async fn subscribe_to_state_changes(
        &self,
        _connection_id: &str,
        _callback: Box<dyn Fn(ConnectionState) + Send + Sync>,
    ) -> DomainResult<()> {
        // Would implement pub/sub mechanism for state changes
        Ok(())
    }
}