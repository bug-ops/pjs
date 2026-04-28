//! Advanced streaming implementations for different protocols

use crate::domain::entities::Frame;
use async_stream::try_stream;
use axum::{
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use futures::{Stream, StreamExt};

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

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn frame_to_value(frame: &Frame) -> serde_json::Value {
    serde_json::json!({
        "type": format!("{:?}", frame.frame_type()),
        "priority": frame.priority().value(),
        "sequence": frame.sequence(),
        "timestamp": frame.timestamp().to_rfc3339(),
        "payload": frame.payload(),
        "metadata": frame.metadata()
    })
}

fn format_frame_owned(frame: &Frame, format: StreamFormat) -> Result<String, StreamError> {
    let v = frame_to_value(frame);
    match format {
        StreamFormat::Json => Ok(serde_json::to_string(&v)?),
        StreamFormat::NdJson => Ok(format!("{}\n", serde_json::to_string(&v)?)),
        StreamFormat::ServerSentEvents => Ok(format!("data: {}\n\n", serde_json::to_string(&v)?)),
        StreamFormat::Binary => Ok(serde_json::to_string(&v)?),
    }
}

/// Serializes a batch of frames.
///
/// Each batch is serialized as newline-delimited JSON objects (one object per
/// frame). `StreamFormat::Json` and `StreamFormat::NdJson` produce identical
/// wire bytes; only `content_type()` differs.
fn format_batch_owned(frames: &[Frame], format: StreamFormat) -> Result<String, StreamError> {
    let values: Vec<_> = frames.iter().map(frame_to_value).collect();
    match format {
        // #167: NDJSON-of-objects — one JSON object per line per frame.
        // Identical wire bytes for Json and NdJson; only content_type() differs.
        StreamFormat::Json | StreamFormat::NdJson => {
            let mut out = String::new();
            for v in values {
                out.push_str(&serde_json::to_string(&v)?);
                out.push('\n');
            }
            Ok(out)
        }
        StreamFormat::ServerSentEvents => {
            let mut out = String::new();
            for v in values {
                out.push_str(&format!("data: {}\n\n", serde_json::to_string(&v)?));
            }
            Ok(out)
        }
        StreamFormat::Binary => Ok(serde_json::to_string(&values)?),
    }
}

fn maybe_compress_owned(s: String, enabled: bool) -> Result<String, StreamError> {
    #[cfg(feature = "compression")]
    if enabled {
        use crate::compression::secure::{ByteCodec, SecureCompressor};
        let compressor = SecureCompressor::with_default_security(ByteCodec::Gzip);
        let compressed = compressor
            .compress(s.as_bytes())
            .map_err(|e| StreamError::Io(e.to_string()))?;
        return Ok(String::from_utf8_lossy(&compressed.data).into_owned());
    }
    #[cfg(not(feature = "compression"))]
    let _ = enabled;
    Ok(s)
}

// ---------------------------------------------------------------------------
// AdaptiveFrameStream
// ---------------------------------------------------------------------------

/// Adaptive frame stream that optimizes based on client capabilities.
///
/// Frames are prefetched in batches of up to `buffer_size` items per executor
/// wakeup via `StreamExt::ready_chunks`, matching the documented prefetch
/// semantics from #163.
pub struct AdaptiveFrameStream<S> {
    inner: S,
    format: StreamFormat,
    compression: bool,
    buffer_size: usize,
}

impl<S> AdaptiveFrameStream<S>
where
    S: Stream<Item = Frame> + Unpin + Send + 'static,
{
    pub fn new(stream: S, format: StreamFormat) -> Self {
        Self {
            inner: stream,
            format,
            compression: false,
            buffer_size: 10,
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

    /// Consume the builder and return a `Stream` of formatted, optionally
    /// compressed frame strings.
    ///
    /// `ready_chunks(buffer_size)` polls the inner stream up to `buffer_size`
    /// times per wakeup, preserving the prefetch semantics of the original
    /// hand-rolled `poll_next` buffer loop.
    pub fn into_stream(self) -> impl Stream<Item = Result<String, StreamError>> + Send + 'static {
        let Self {
            inner,
            format,
            compression,
            buffer_size,
        } = self;
        try_stream! {
            let mut chunked = inner.ready_chunks(buffer_size);
            while let Some(batch) = chunked.next().await {
                for frame in batch {
                    let s = format_frame_owned(&frame, format)?;
                    let s = maybe_compress_owned(s, compression)?;
                    yield s;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// BatchFrameStream
// ---------------------------------------------------------------------------

/// Batch frame stream for improved throughput.
pub struct BatchFrameStream<S> {
    inner: S,
    format: StreamFormat,
    batch_size: usize,
}

impl<S> BatchFrameStream<S>
where
    S: Stream<Item = Frame> + Unpin + Send + 'static,
{
    pub fn new(stream: S, format: StreamFormat, batch_size: usize) -> Self {
        Self {
            inner: stream,
            format,
            batch_size,
        }
    }

    /// Returns the `Content-Type` that accurately describes what this stream emits.
    ///
    /// `BatchFrameStream` serializes each batch as newline-delimited JSON objects,
    /// so `StreamFormat::Json` is promoted to `application/x-ndjson` — the output
    /// is not a single well-formed JSON document and must not be advertised as one.
    pub fn content_type(&self) -> &'static str {
        match self.format {
            StreamFormat::Json => "application/x-ndjson",
            other => other.content_type(),
        }
    }

    /// Consume the builder and return a `Stream` of formatted batch strings.
    ///
    /// Each string contains one full batch. For `StreamFormat::Json` and
    /// `StreamFormat::NdJson` the string holds one JSON object per frame,
    /// one per line (NDJSON-of-objects, #167).
    pub fn into_stream(self) -> impl Stream<Item = Result<String, StreamError>> + Send + 'static {
        let Self {
            inner,
            format,
            batch_size,
        } = self;
        try_stream! {
            let mut batch: Vec<Frame> = Vec::with_capacity(batch_size);
            futures::pin_mut!(inner);

            while let Some(frame) = inner.next().await {
                batch.push(frame);
                if batch.len() >= batch_size {
                    let s = format_batch_owned(&batch, format)?;
                    batch.clear();
                    yield s;
                }
            }

            if !batch.is_empty() {
                let s = format_batch_owned(&batch, format)?;
                yield s;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PriorityFrameStream
// ---------------------------------------------------------------------------

/// Priority-based frame stream that orders frames by importance.
pub struct PriorityFrameStream<S> {
    inner: S,
    format: StreamFormat,
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
    S: Stream<Item = Frame> + Unpin + Send + 'static,
{
    pub fn new(stream: S, format: StreamFormat, buffer_size: usize) -> Self {
        Self {
            inner: stream,
            format,
            buffer_size,
        }
    }

    /// Consume the builder and return a `Stream` of priority-ordered, formatted
    /// frame strings.
    ///
    /// Frames are buffered up to `buffer_size` and emitted highest-priority first.
    pub fn into_stream(self) -> impl Stream<Item = Result<String, StreamError>> + Send + 'static {
        let Self {
            inner,
            format,
            buffer_size,
        } = self;
        try_stream! {
            let mut heap = std::collections::BinaryHeap::<PriorityFrame>::with_capacity(buffer_size);
            let mut inner_done = false;
            futures::pin_mut!(inner);

            loop {
                // Fill the buffer until full or inner stream pauses/ends.
                while !inner_done && heap.len() < buffer_size {
                    match inner.next().await {
                        Some(frame) => {
                            let priority = frame.priority().value();
                            heap.push(PriorityFrame { frame, priority });
                        }
                        None => inner_done = true,
                    }
                }

                match heap.pop() {
                    Some(pf) => {
                        let s = format_frame_owned(&pf.frame, format)?;
                        yield s;
                    }
                    None if inner_done => break,
                    // Buffer empty but inner not done: inner.next().await above
                    // will re-enter the fill loop on the next iteration.
                    None => unreachable!("loop above guarantees inner_done or non-empty heap"),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stream error types
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Response helper
// ---------------------------------------------------------------------------

/// Create a response with appropriate headers for the given streaming format.
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

    match format {
        StreamFormat::ServerSentEvents => {
            response = response
                .header(header::CONNECTION, "keep-alive")
                .header("X-Accel-Buffering", "no");
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
    use crate::domain::entities::Frame;
    use crate::domain::value_objects::{JsonData, JsonPath, Priority, StreamId};
    use futures::StreamExt;
    use futures::stream;
    use pjson_rs_domain::entities::frame::FramePatch;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    fn make_skeleton_frame() -> Frame {
        Frame::skeleton(StreamId::new(), 1, JsonData::Null)
    }

    fn make_patch_frame(priority: Priority) -> Frame {
        let path = JsonPath::new("$.x").expect("valid path");
        let patch = FramePatch::set(path, JsonData::Null);
        Frame::patch(StreamId::new(), 1, priority, vec![patch]).expect("valid patch frame")
    }

    // -----------------------------------------------------------------------
    // PendingThenReady: adversarial test stream
    //
    // Returns `Poll::Pending` exactly `pending_per_item` times before each
    // item, then `Poll::Ready(Some(item))`. After exhaustion, always returns
    // `Poll::Ready(None)` (done short-circuit prevents spurious Pending phases
    // after completion, making it compatible with fused-stream consumers).
    // -----------------------------------------------------------------------

    struct PendingThenReady<I: Iterator> {
        iter: I,
        pending_remaining: usize,
        pending_per_item: usize,
        /// Short-circuit: once the inner iterator is exhausted, never return
        /// Pending again so that fused consumers and select!-driven code work.
        done: bool,
    }

    impl<I: Iterator> PendingThenReady<I> {
        fn new(iter: I, pending_per_item: usize) -> Self {
            Self {
                iter,
                pending_remaining: pending_per_item,
                pending_per_item,
                done: false,
            }
        }
    }

    impl<I: Iterator + Unpin> Stream for PendingThenReady<I> {
        type Item = I::Item;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            if self.done {
                return Poll::Ready(None);
            }
            if self.pending_remaining > 0 {
                self.pending_remaining -= 1;
                // CRITICAL: re-arm the waker so the executor will poll again.
                // Without this the stream stalls forever — exactly the pattern
                // that exposes #166 in hand-rolled poll_next impls.
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            match self.iter.next() {
                Some(item) => {
                    self.pending_remaining = self.pending_per_item;
                    Poll::Ready(Some(item))
                }
                None => {
                    self.done = true;
                    Poll::Ready(None)
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Existing tests (updated to use .into_stream())
    // -----------------------------------------------------------------------

    #[test]
    fn test_stream_format_detection() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, "text/event-stream".parse().unwrap());

        let format = StreamFormat::from_accept_header(&headers);
        assert!(matches!(format, StreamFormat::ServerSentEvents));
    }

    #[tokio::test]
    async fn test_adaptive_stream_empty() {
        let frame_stream = stream::iter(Vec::<Frame>::new());
        let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::Json);
        let collected: Vec<_> = adaptive.into_stream().collect().await;
        assert!(collected.is_empty());
    }

    /// Each output line must be a valid JSON object (NDJSON-of-objects, #167).
    #[tokio::test]
    async fn test_batch_frame_stream_multiple_batches() {
        let frames: Vec<Frame> = (0..5).map(|_| make_skeleton_frame()).collect();
        let frame_stream = stream::iter(frames);

        // batch_size=2 → two full batches of 2 and one remainder batch of 1
        let batch_stream = BatchFrameStream::new(frame_stream, StreamFormat::Json, 2);
        let collected: Vec<Result<String, StreamError>> =
            batch_stream.into_stream().collect().await;

        assert_eq!(
            collected.len(),
            3,
            "expected 3 batches for 5 frames with batch_size=2"
        );

        let mut total_objects = 0usize;
        for result in &collected {
            let batch_str = result.as_ref().expect("batch should not error");
            for line in batch_str.lines() {
                if line.is_empty() {
                    continue;
                }
                let parsed: serde_json::Value =
                    serde_json::from_str(line).expect("each line must be valid JSON");
                assert!(
                    parsed.is_object(),
                    "each line must be a JSON object (NDJSON-of-objects), got: {line}"
                );
                total_objects += 1;
            }
        }
        assert_eq!(
            total_objects, 5,
            "total parsed objects across all batches must equal 5"
        );
    }

    /// After the inner stream ends and the buffer drains, `PriorityFrameStream` must
    /// return `Poll::Ready(None)` — not hang on `Poll::Pending`.
    #[tokio::test]
    async fn test_priority_stream_terminates() {
        let frames: Vec<Frame> = (0..4).map(|_| make_skeleton_frame()).collect();
        let frame_stream = stream::iter(frames);

        let priority_stream = PriorityFrameStream::new(frame_stream, StreamFormat::Json, 8);
        let collected: Vec<Result<String, StreamError>> =
            priority_stream.into_stream().collect().await;

        assert_eq!(collected.len(), 4);
        for result in &collected {
            assert!(result.is_ok());
        }
    }

    /// Frames must be emitted in descending priority order (highest first).
    #[tokio::test]
    async fn test_priority_stream_ordering() {
        let frames = vec![
            make_patch_frame(Priority::new(10).unwrap()),
            make_patch_frame(Priority::new(50).unwrap()),
            make_patch_frame(Priority::new(30).unwrap()),
        ];
        let frame_stream = stream::iter(frames);

        let priority_stream = PriorityFrameStream::new(frame_stream, StreamFormat::Json, 8);
        let collected: Vec<_> = priority_stream
            .into_stream()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|r| r.expect("no error"))
            .collect();

        let priorities: Vec<u64> = collected
            .iter()
            .map(|s| {
                let v: serde_json::Value = serde_json::from_str(s).unwrap();
                v["priority"].as_u64().unwrap()
            })
            .collect();

        assert_eq!(
            priorities,
            vec![50, 30, 10],
            "frames must be ordered highest priority first"
        );
    }

    // -----------------------------------------------------------------------
    // New tests using PendingThenReady (#168)
    // -----------------------------------------------------------------------

    /// `AdaptiveFrameStream` must yield all N frames when the inner stream
    /// returns `Poll::Pending` between items (tests that `ready_chunks` does
    /// not stall the waker contract).
    #[test]
    fn test_adaptive_stream_makes_progress_under_pending() {
        tokio_test::block_on(async {
            let frames: Vec<Frame> = (0..6).map(|_| make_skeleton_frame()).collect();
            let inner = PendingThenReady::new(frames.into_iter(), 3);
            let adaptive = AdaptiveFrameStream::new(inner, StreamFormat::Json);
            let collected: Vec<_> = adaptive.into_stream().collect().await;
            assert_eq!(collected.len(), 6);
            for r in collected {
                assert!(r.is_ok());
            }
        });
    }

    /// `BatchFrameStream` with batch_size=3 over 6 frames must emit exactly 2
    /// batches, even when the inner stream interleaves `Poll::Pending`.
    /// The half-batch-on-Pending heuristic (removed) would have emitted more.
    #[test]
    fn test_batch_stream_emits_only_full_batches_under_pending() {
        tokio_test::block_on(async {
            let frames: Vec<Frame> = (0..6).map(|_| make_skeleton_frame()).collect();
            let inner = PendingThenReady::new(frames.into_iter(), 2);
            let batch = BatchFrameStream::new(inner, StreamFormat::Json, 3);
            let collected: Vec<_> = batch.into_stream().collect().await;
            assert_eq!(
                collected.len(),
                2,
                "6 frames at batch_size=3 must yield exactly 2 batches"
            );
            for r in collected {
                assert!(r.is_ok());
            }
        });
    }

    /// Validates the r3 wire format for all four `StreamFormat` variants of
    /// `BatchFrameStream`:
    /// - `Json` → one JSON object per line per frame (NDJSON-of-objects)
    /// - `NdJson` → identical bytes to `Json`
    /// - `ServerSentEvents` → `data: <object>\n\n` per frame
    /// - `Binary` → single JSON array, no trailing newline
    #[tokio::test]
    async fn test_batch_stream_ndjson_objects_per_line() {
        let make_frames = || -> Vec<Frame> { (0..3).map(|_| make_skeleton_frame()).collect() };

        // Json: one object per line
        let result_json: Vec<_> =
            BatchFrameStream::new(stream::iter(make_frames()), StreamFormat::Json, 10)
                .into_stream()
                .collect()
                .await;
        assert_eq!(result_json.len(), 1);
        let json_str = result_json[0].as_ref().unwrap();
        for line in json_str.lines() {
            if line.is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(v.is_object(), "Json format: each line must be an object");
        }

        // NdJson: same wire shape as Json — one object per line per frame.
        let result_ndjson: Vec<_> =
            BatchFrameStream::new(stream::iter(make_frames()), StreamFormat::NdJson, 10)
                .into_stream()
                .collect()
                .await;
        assert_eq!(result_ndjson.len(), 1);
        let ndjson_str = result_ndjson[0].as_ref().unwrap();
        for line in ndjson_str.lines() {
            if line.is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(v.is_object(), "NdJson format: each line must be an object");
        }
        // Both formats must produce the same number of objects per batch
        let json_count = json_str.lines().filter(|l| !l.is_empty()).count();
        let ndjson_count = ndjson_str.lines().filter(|l| !l.is_empty()).count();
        assert_eq!(
            json_count, ndjson_count,
            "Json and NdJson must produce the same object count"
        );

        // SSE: data: <object>\n\n per frame
        let result_sse: Vec<_> = BatchFrameStream::new(
            stream::iter(make_frames()),
            StreamFormat::ServerSentEvents,
            10,
        )
        .into_stream()
        .collect()
        .await;
        assert_eq!(result_sse.len(), 1);
        let sse_str = result_sse[0].as_ref().unwrap();
        let sse_frames: Vec<&str> = sse_str.split("\n\n").filter(|s| !s.is_empty()).collect();
        assert_eq!(sse_frames.len(), 3);
        for frame_str in sse_frames {
            assert!(frame_str.starts_with("data: "));
            let json_part = &frame_str["data: ".len()..];
            let v: serde_json::Value = serde_json::from_str(json_part).unwrap();
            assert!(v.is_object());
        }

        // Binary: single JSON array
        let result_binary: Vec<_> =
            BatchFrameStream::new(stream::iter(make_frames()), StreamFormat::Binary, 10)
                .into_stream()
                .collect()
                .await;
        assert_eq!(result_binary.len(), 1);
        let binary_str = result_binary[0].as_ref().unwrap();
        let v: serde_json::Value = serde_json::from_str(binary_str).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 3);
    }

    /// `PriorityFrameStream` must drain its heap and return `None` when the
    /// inner stream interleaves `Poll::Pending` (regression for the `inner_done`
    /// fix from commit `a0a8d83`).
    #[test]
    fn test_priority_stream_terminates_under_pending() {
        tokio_test::block_on(async {
            let frames: Vec<Frame> = (0..5).map(|_| make_skeleton_frame()).collect();
            let inner = PendingThenReady::new(frames.into_iter(), 4);
            let priority = PriorityFrameStream::new(inner, StreamFormat::Json, 8);
            let collected: Vec<_> = priority.into_stream().collect().await;
            assert_eq!(collected.len(), 5);
            for r in collected {
                assert!(r.is_ok());
            }
        });
    }

    /// Priority ordering is preserved when buffer fill is interleaved with
    /// `Poll::Pending` from the inner stream.
    #[test]
    fn test_priority_stream_ordering_preserved_under_pending() {
        tokio_test::block_on(async {
            let frames = vec![
                make_patch_frame(Priority::new(10).unwrap()),
                make_patch_frame(Priority::new(50).unwrap()),
                make_patch_frame(Priority::new(30).unwrap()),
                make_patch_frame(Priority::new(80).unwrap()),
            ];
            let inner = PendingThenReady::new(frames.into_iter(), 2);
            let priority = PriorityFrameStream::new(inner, StreamFormat::Json, 10);
            let collected: Vec<_> = priority
                .into_stream()
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|r| r.expect("no error"))
                .collect();

            assert_eq!(collected.len(), 4);

            let priorities: Vec<u64> = collected
                .iter()
                .map(|s| {
                    let v: serde_json::Value = serde_json::from_str(s).unwrap();
                    v["priority"].as_u64().unwrap()
                })
                .collect();

            // All frames fit in the buffer (size=10) so they must arrive fully sorted.
            assert_eq!(priorities, vec![80, 50, 30, 10]);
        });
    }
}
