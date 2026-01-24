//! Advanced streaming implementations for different protocols

use crate::domain::entities::Frame;
use axum::{
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use futures::Stream;
use serde_json::Value as JsonValue;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

/// Streaming format types
#[derive(Debug, Clone, Copy)]
pub enum StreamFormat {
    /// Standard JSON array streaming
    Json,
    /// Newline-delimited JSON
    NdJson,
    /// Server-Sent Events
    ServerSentEvents,
    /// Binary PJS protocol
    Binary,
}

impl StreamFormat {
    pub fn from_accept_header(headers: &HeaderMap) -> Self {
        if let Some(accept) = headers.get(header::ACCEPT)
            && let Ok(accept_str) = accept.to_str()
        {
            if accept_str.contains("text/event-stream") {
                return Self::ServerSentEvents;
            } else if accept_str.contains("application/x-ndjson") {
                return Self::NdJson;
            } else if accept_str.contains("application/octet-stream") {
                return Self::Binary;
            }
        }
        Self::Json
    }

    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::NdJson => "application/x-ndjson",
            Self::ServerSentEvents => "text/event-stream",
            Self::Binary => "application/octet-stream",
        }
    }
}

/// Adaptive frame stream that optimizes based on client capabilities
pub struct AdaptiveFrameStream<S> {
    inner: S,
    format: StreamFormat,
    compression: bool,
    buffer_size: usize,
    #[allow(dead_code)] // Future feature: adaptive batching implementation
    current_buffer: Vec<String>,
}

impl<S> AdaptiveFrameStream<S>
where
    S: Stream<Item = Frame> + Unpin,
{
    pub fn new(stream: S, format: StreamFormat) -> Self {
        Self {
            inner: stream,
            format,
            compression: false,
            buffer_size: 10,
            current_buffer: Vec::new(),
        }
    }

    pub fn with_compression(mut self, enabled: bool) -> Self {
        self.compression = enabled;
        self
    }

    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    fn format_frame(&self, frame: &Frame) -> Result<String, StreamError> {
        let frame_data = serde_json::json!({
            "type": format!("{:?}", frame.frame_type()),
            "priority": frame.priority().value(),
            "sequence": frame.sequence(),
            "timestamp": frame.timestamp().to_rfc3339(),
            "payload": frame.payload(),
            "metadata": frame.metadata()
        });

        match self.format {
            StreamFormat::Json => Ok(serde_json::to_string(&frame_data)?),
            StreamFormat::NdJson => Ok(format!("{}\n", serde_json::to_string(&frame_data)?)),
            StreamFormat::ServerSentEvents => {
                Ok(format!("data: {}\n\n", serde_json::to_string(&frame_data)?))
            }
            StreamFormat::Binary => {
                // Simplified binary format - would use more efficient encoding in production
                Ok(serde_json::to_string(&frame_data)?)
            }
        }
    }
}

impl<S> Stream for AdaptiveFrameStream<S>
where
    S: Stream<Item = Frame> + Unpin,
{
    type Item = Result<String, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(frame)) => {
                let formatted = self.format_frame(&frame);
                Poll::Ready(Some(formatted))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Batch frame stream for improved throughput
pub struct BatchFrameStream<S> {
    inner: S,
    format: StreamFormat,
    batch_size: usize,
    current_batch: Vec<Frame>,
    is_first_batch: bool,
}

impl<S> BatchFrameStream<S>
where
    S: Stream<Item = Frame> + Unpin,
{
    pub fn new(stream: S, format: StreamFormat, batch_size: usize) -> Self {
        Self {
            inner: stream,
            format,
            batch_size,
            current_batch: Vec::new(),
            is_first_batch: true,
        }
    }

    fn format_batch(&self, frames: &[Frame]) -> Result<String, StreamError> {
        let batch_data: Vec<JsonValue> = frames
            .iter()
            .map(|frame| {
                serde_json::json!({
                    "type": format!("{:?}", frame.frame_type()),
                    "priority": frame.priority().value(),
                    "sequence": frame.sequence(),
                    "timestamp": frame.timestamp().to_rfc3339(),
                    "payload": frame.payload(),
                    "metadata": frame.metadata()
                })
            })
            .collect();

        match self.format {
            StreamFormat::Json => {
                if self.is_first_batch {
                    Ok(format!("[{}]", serde_json::to_string(&batch_data)?))
                } else {
                    Ok(format!(",{}", serde_json::to_string(&batch_data)?))
                }
            }
            StreamFormat::NdJson => {
                let mut result = String::new();
                for item in batch_data {
                    result.push_str(&serde_json::to_string(&item)?);
                    result.push('\n');
                }
                Ok(result)
            }
            StreamFormat::ServerSentEvents => {
                let mut result = String::new();
                for item in batch_data {
                    result.push_str(&format!("data: {}\n\n", serde_json::to_string(&item)?));
                }
                Ok(result)
            }
            StreamFormat::Binary => Ok(serde_json::to_string(&batch_data)?),
        }
    }
}

impl<S> Stream for BatchFrameStream<S>
where
    S: Stream<Item = Frame> + Unpin,
{
    type Item = Result<String, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(frame)) => {
                    self.current_batch.push(frame);

                    if self.current_batch.len() >= self.batch_size {
                        let batch = std::mem::take(&mut self.current_batch);
                        let formatted = self.format_batch(&batch);
                        self.is_first_batch = false;
                        return Poll::Ready(Some(formatted));
                    }
                }
                Poll::Ready(None) => {
                    if !self.current_batch.is_empty() {
                        let batch = std::mem::take(&mut self.current_batch);
                        let formatted = self.format_batch(&batch);
                        return Poll::Ready(Some(formatted));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => {
                    if !self.current_batch.is_empty()
                        && self.current_batch.len() >= self.batch_size / 2
                    {
                        // Send partial batch if we have some frames and are waiting
                        let batch = std::mem::take(&mut self.current_batch);
                        let formatted = self.format_batch(&batch);
                        self.is_first_batch = false;
                        return Poll::Ready(Some(formatted));
                    }
                    return Poll::Pending;
                }
            }
        }
    }
}

/// Priority-based frame stream that orders frames by importance
pub struct PriorityFrameStream<S> {
    inner: S,
    format: StreamFormat,
    priority_buffer: std::collections::BinaryHeap<PriorityFrame>,
    buffer_size: usize,
}

#[derive(Debug, Clone)]
struct PriorityFrame {
    frame: Frame,
    priority: u8,
}

impl PartialEq for PriorityFrame {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for PriorityFrame {}

impl PartialOrd for PriorityFrame {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityFrame {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl<S> PriorityFrameStream<S>
where
    S: Stream<Item = Frame> + Unpin,
{
    pub fn new(stream: S, format: StreamFormat, buffer_size: usize) -> Self {
        Self {
            inner: stream,
            format,
            priority_buffer: std::collections::BinaryHeap::new(),
            buffer_size,
        }
    }

    fn format_frame(&self, frame: &Frame) -> Result<String, StreamError> {
        let frame_data = serde_json::json!({
            "type": format!("{:?}", frame.frame_type()),
            "priority": frame.priority().value(),
            "sequence": frame.sequence(),
            "timestamp": frame.timestamp().to_rfc3339(),
            "payload": frame.payload(),
            "metadata": frame.metadata()
        });

        match self.format {
            StreamFormat::Json => Ok(serde_json::to_string(&frame_data)?),
            StreamFormat::NdJson => Ok(format!("{}\n", serde_json::to_string(&frame_data)?)),
            StreamFormat::ServerSentEvents => {
                Ok(format!("data: {}\n\n", serde_json::to_string(&frame_data)?))
            }
            StreamFormat::Binary => Ok(serde_json::to_string(&frame_data)?),
        }
    }
}

impl<S> Stream for PriorityFrameStream<S>
where
    S: Stream<Item = Frame> + Unpin,
{
    type Item = Result<String, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Fill buffer with frames
        while self.priority_buffer.len() < self.buffer_size {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(frame)) => {
                    let priority = frame.priority().value();
                    self.priority_buffer.push(PriorityFrame { frame, priority });
                }
                Poll::Ready(None) => break,
                Poll::Pending => break,
            }
        }

        // Return highest priority frame
        if let Some(priority_frame) = self.priority_buffer.pop() {
            let formatted = self.format_frame(&priority_frame.frame);
            Poll::Ready(Some(formatted))
        } else if self.priority_buffer.is_empty() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

/// Stream error types
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Buffer overflow")]
    BufferOverflow,

    #[error("Stream closed")]
    StreamClosed,
}

/// Create response with appropriate headers for streaming format
pub fn create_streaming_response<S>(
    stream: S,
    format: StreamFormat,
) -> Result<Response, StreamError>
where
    S: Stream<Item = Result<String, StreamError>> + Send + 'static,
{
    let body = axum::body::Body::from_stream(stream);

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, format.content_type())
        .header(header::CACHE_CONTROL, "no-cache");

    // Add format-specific headers
    match format {
        StreamFormat::ServerSentEvents => {
            response = response
                .header(header::CONNECTION, "keep-alive")
                .header("X-Accel-Buffering", "no"); // Disable nginx buffering
        }
        StreamFormat::NdJson => {
            response = response.header("Transfer-Encoding", "chunked");
        }
        _ => {}
    }

    response
        .body(body)
        .map_err(|e| StreamError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[test]
    fn test_stream_format_detection() {
        let mut headers = HeaderMap::new();
        // TODO: Handle unwrap() - add proper error handling for header value parsing in tests
        headers.insert(header::ACCEPT, "text/event-stream".parse().unwrap());

        let format = StreamFormat::from_accept_header(&headers);
        assert!(matches!(format, StreamFormat::ServerSentEvents));
    }

    #[tokio::test]
    async fn test_adaptive_stream() {
        use futures::StreamExt;

        // Create mock frame stream
        let frames = vec![
            // Would create actual Frame objects here
        ];
        let frame_stream = stream::iter(frames);

        let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::Json);
        let _collected: Vec<_> = adaptive.collect().await;

        // Test would verify format output
    }
}
