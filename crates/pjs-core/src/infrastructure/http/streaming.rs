//! Advanced streaming implementations for different protocols

use crate::domain::entities::Frame;
use async_stream::try_stream;
use axum::{
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use futures::{Stream, StreamExt};
use headers_accept::Accept;
use mediatype::{MediaType, MediaTypeBuf, Name, names};
use std::str::FromStr;

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

/// Maximum number of comma-separated `Accept` entries considered during content
/// negotiation.
///
/// Bounds iteration over untrusted input (project security invariant); entries
/// beyond this count are ignored, not rejected. Enforced by truncating the raw
/// header string *before* it reaches [`headers_accept`]'s parser, since that
/// crate does not itself bound entry count.
const MAX_ACCEPT_ENTRIES: usize = 16;

/// Non-standard "ndjson" subtype name.
///
/// Not present in [`mediatype`]'s built-in IANA registry constants (`x-`
/// prefixed extension types aren't registered), so it is declared here.
const X_NDJSON: Name<'static> = Name::new_unchecked("x-ndjson");

/// Server-supported media ranges for this route, paired with the
/// [`StreamFormat`] each selects.
///
/// Order is the tie-break preference used when [`Accept::negotiate`] finds
/// multiple candidates matching the same `Accept` entry at equal specificity
/// and `q` (e.g. a bare `*/*` or `application/*`, which match every entry
/// here equally) — [`StreamFormat::Json`] listed first preserves the
/// permissive default fallback.
static SUPPORTED_MEDIA_TYPES: [(MediaType<'static>, StreamFormat); 4] = [
    (
        MediaType::new(names::APPLICATION, names::JSON),
        StreamFormat::Json,
    ),
    (
        MediaType::new(names::TEXT, names::EVENT_STREAM),
        StreamFormat::ServerSentEvents,
    ),
    (
        MediaType::new(names::APPLICATION, X_NDJSON),
        StreamFormat::NdJson,
    ),
    (
        MediaType::new(names::APPLICATION, names::OCTET_STREAM),
        StreamFormat::Binary,
    ),
];

/// Whether `media_range` is eligible for negotiation.
///
/// Wildcards are restricted to exactly `*/*` and `application/*`
/// (case-insensitive) — the only two forms this route has ever supported.
/// Any other range containing a `*` (e.g. `text/*`, `*/x-ndjson`) is rejected
/// here rather than handed to `headers_accept`, whose wildcard matching is
/// general RFC 9110 `type/*`/`*/*` matching — broader than this route's
/// historical, scoped wildcard support, and would otherwise let e.g.
/// `*/x-ndjson` match every candidate the same as `*/*`. Non-wildcard ranges
/// are always eligible; an unrecognized concrete type (e.g. `application/xml`)
/// simply never matches any candidate in [`SUPPORTED_MEDIA_TYPES`] and falls
/// through to the permissive fallback.
fn is_supported_media_range(media_range: &str) -> bool {
    !media_range.contains('*')
        || media_range.eq_ignore_ascii_case("*/*")
        || media_range.eq_ignore_ascii_case("application/*")
}

/// Formats an already-clamped `q` (`[0.0, 1.0]`) to the `<=3` fractional digits
/// `headers_accept`'s `QValue` grammar requires.
///
/// Rounds to the nearest representable value, but never rounds a positive `q`
/// down to `0.000` — `headers_accept` treats `q=0` as an explicit rejection of
/// the entry, so floor-rounding e.g. `q=0.0004` to `0.000` would flip a
/// (barely) acceptable preference into a hard rejection.
fn format_q(q: f32) -> String {
    if q <= 0.0 {
        return "0.000".to_string();
    }
    let milli = ((q * 1000.0).round() as u32).max(1);
    if milli >= 1000 {
        "1.000".to_string()
    } else {
        format!("0.{milli:03}")
    }
}

impl StreamFormat {
    /// Picks a streaming format for the request's `Accept` header.
    ///
    /// Negotiation is RFC 9110 §12.5.1-conformant for this route's supported media
    /// ranges, delegated to [`headers_accept::Accept::negotiate`]: candidates are
    /// ranked by `q` value first and by media-range specificity as the tiebreaker
    /// (exact match > `application/*` > `*/*`).
    ///
    /// - `q=0` is an explicit rejection of that media range and drops the entry.
    /// - A `q` parameter that is non-finite (`nan`/`inf`/`-inf`) or otherwise fails
    ///   to parse as a float drops its entry — a malformed preference is treated
    ///   as "no preference stated", not "highest preference". This is validated
    ///   before delegating to `headers_accept`, whose own default treats an
    ///   unparsable `q` as absent (i.e. `1.0`, the highest priority) rather than
    ///   dropping the entry.
    /// - A finite but out-of-range `q` (e.g. `q=5`) is clamped to `[0.0, 1.0]` and
    ///   the entry is kept — matching the pre-migration hand-rolled parser's
    ///   `.clamp(0.0, 1.0)` exactly (not dropped; only `q=0`, non-finite, and
    ///   unparsable `q` are dropped).
    /// - Wildcard matching is restricted to exactly `*/*` and `application/*`
    ///   (case-insensitive), which vote for [`Self::Json`] at their own `q`.
    ///   Any other range containing a `*` (e.g. `text/*`, `*/x-ndjson`) is
    ///   rejected rather than delegated to `headers_accept`'s more general
    ///   `type/*`/`*/*` matching, which is broader than this route's
    ///   historical, scoped wildcard support.
    /// - Each surviving entry is validated as a well-formed media type
    ///   independently, before negotiation; a malformed entry (e.g. `garbage!!`)
    ///   is dropped individually and does not discard the rest of the header —
    ///   `headers_accept::Accept::from_str` itself fails the *entire* header on a
    ///   single bad entry, so this crate is never handed anything but
    ///   already-validated, surviving entries.
    /// - At most `MAX_ACCEPT_ENTRIES` (16) comma-separated entries are considered;
    ///   any beyond that bound are silently ignored. This bound is enforced by
    ///   truncating the raw header string before it reaches `headers_accept`'s
    ///   parser, since that crate does not itself bound entry count.
    /// - A missing header, an unparsable header value, or no entry surviving
    ///   negotiation all fall back to [`Self::Json`].
    pub fn from_accept_header(headers: &HeaderMap) -> Self {
        let Some(accept) = headers.get(header::ACCEPT) else {
            return Self::Json;
        };
        let Ok(accept_str) = accept.to_str() else {
            return Self::Json;
        };

        let mut sanitized_entries: Vec<String> = Vec::new();
        for entry in accept_str.split(',').take(MAX_ACCEPT_ENTRIES) {
            let mut parts = entry.split(';');
            let media_range = parts.next().unwrap_or("").trim();
            if media_range.is_empty() || !is_supported_media_range(media_range) {
                continue;
            }

            let mut q_str: Option<&str> = None;
            for param in parts {
                let mut kv = param.splitn(2, '=');
                let name = kv.next().unwrap_or("").trim();
                if name.eq_ignore_ascii_case("q") {
                    q_str = Some(kv.next().unwrap_or("").trim());
                    break;
                }
            }

            let sanitized = match q_str {
                None => media_range.to_string(),
                Some(raw_q) => {
                    let Ok(q) = raw_q.parse::<f32>() else {
                        continue;
                    };
                    if !q.is_finite() {
                        continue;
                    }
                    format!("{media_range};q={}", format_q(q.clamp(0.0, 1.0)))
                }
            };

            // Validate independently, per entry: `Accept::from_str` fails the
            // whole header on a single malformed entry (S1), so a bad entry must
            // be dropped here, before the survivors are ever joined together.
            if MediaTypeBuf::from_str(&sanitized).is_err() {
                continue;
            }
            sanitized_entries.push(sanitized);
        }

        if sanitized_entries.is_empty() {
            return Self::Json;
        }
        let Ok(accept) = Accept::from_str(&sanitized_entries.join(",")) else {
            return Self::Json;
        };

        let Some(best) = accept.negotiate(SUPPORTED_MEDIA_TYPES.iter().map(|(mt, _)| mt)) else {
            return Self::Json;
        };
        SUPPORTED_MEDIA_TYPES
            .iter()
            .find(|(mt, _)| mt == best)
            .map_or(Self::Json, |(_, format)| *format)
    }

    /// MIME type that corresponds to this streaming format.
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

/// Serializes a frame directly via [`Frame`]'s own derived
/// [`Serialize`](serde::Serialize) impl — this is what keeps the per-frame
/// wire shape (field names, `stream_id` presence, `frame_type`
/// representation) identical to the buffered `/frames` route, and avoids
/// materializing an intermediate `serde_json::Value` tree per frame.
fn format_frame_owned(
    frame: &Frame,
    format: StreamFormat,
) -> Result<Vec<u8>, StreamTransportError> {
    match format {
        StreamFormat::Json | StreamFormat::Binary => Ok(sonic_rs::to_vec(frame)?),
        StreamFormat::NdJson => {
            let mut out = sonic_rs::to_vec(frame)?;
            out.push(b'\n');
            Ok(out)
        }
        StreamFormat::ServerSentEvents => {
            let mut out = Vec::from(b"data: ".as_slice());
            out.extend_from_slice(&sonic_rs::to_vec(frame)?);
            out.extend_from_slice(b"\n\n");
            Ok(out)
        }
    }
}

/// Serializes a batch of frames.
///
/// Each batch is serialized as newline-delimited JSON objects (one object per
/// frame). `StreamFormat::Json` and `StreamFormat::NdJson` produce identical
/// wire bytes; only `content_type()` differs.
fn format_batch_owned(
    frames: &[Frame],
    format: StreamFormat,
) -> Result<Vec<u8>, StreamTransportError> {
    match format {
        // #167: NDJSON-of-objects — one JSON object per line per frame.
        // Identical wire bytes for Json and NdJson; only content_type() differs.
        StreamFormat::Json | StreamFormat::NdJson => {
            let mut out = Vec::new();
            for frame in frames {
                out.extend_from_slice(&sonic_rs::to_vec(frame)?);
                out.push(b'\n');
            }
            Ok(out)
        }
        StreamFormat::ServerSentEvents => {
            let mut out = Vec::new();
            for frame in frames {
                out.extend_from_slice(b"data: ");
                out.extend_from_slice(&sonic_rs::to_vec(frame)?);
                out.extend_from_slice(b"\n\n");
            }
            Ok(out)
        }
        StreamFormat::Binary => Ok(sonic_rs::to_vec(frames)?),
    }
}

/// Optionally gzip-compresses `bytes` in place.
///
/// When `enabled` and the `compression` feature is active, returns the gzip
/// payload of `bytes`. The output is binary — callers must propagate it as
/// `Vec<u8>`/`Bytes`, never as `String`. See #226 for the architectural fix
/// that replaced the previous UTF-8-only path.
fn maybe_compress(bytes: Vec<u8>, enabled: bool) -> Result<Vec<u8>, StreamTransportError> {
    #[cfg(feature = "compression")]
    if enabled {
        use crate::compression::secure::{ByteCodec, SecureCompressor};
        let compressor = SecureCompressor::with_default_security(ByteCodec::Gzip);
        let compressed = compressor
            .compress(&bytes)
            .map_err(|e| StreamTransportError::Io(e.to_string()))?;
        return Ok(compressed.data);
    }
    #[cfg(not(feature = "compression"))]
    let _ = enabled;
    Ok(bytes)
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
    /// Wrap a frame stream and produce frame bytes in `format`.
    pub fn new(stream: S, format: StreamFormat) -> Self {
        Self {
            inner: stream,
            format,
            compression: false,
            buffer_size: 10,
        }
    }

    /// Toggle gzip compression of each frame payload (requires the `compression` feature).
    ///
    /// This produces a raw gzip *payload* stream, not an HTTP `Content-Encoding: gzip`
    /// body — [`create_streaming_response`] and
    /// [`create_streaming_response_with_content_type`] never set that header, so a
    /// compressed stream must not be handed to either without setting it first (#226).
    pub fn with_compression(mut self, enabled: bool) -> Self {
        self.compression = enabled;
        self
    }

    /// Override how many frames are prefetched per executor wake-up.
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Consume the builder and return a `Stream` of formatted, optionally
    /// compressed frame payloads.
    ///
    /// Items are emitted as `Vec<u8>` because the optional gzip compression
    /// step produces binary bytes that are not valid UTF-8 (#226). Callers
    /// that need a textual view of an uncompressed frame can decode each
    /// payload with `std::str::from_utf8` — but the stream type must remain
    /// binary to support the compressed path.
    ///
    /// `ready_chunks(buffer_size)` polls the inner stream up to `buffer_size`
    /// times per wakeup, preserving the prefetch semantics of the original
    /// hand-rolled `poll_next` buffer loop.
    pub fn into_stream(
        self,
    ) -> impl Stream<Item = Result<Vec<u8>, StreamTransportError>> + Send + 'static {
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
                    let bytes = format_frame_owned(&frame, format)?;
                    let bytes = maybe_compress(bytes, compression)?;
                    yield bytes;
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
    /// Wrap a frame stream and emit batches of up to `batch_size` frames.
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

    /// Consume the builder and return a `Stream` of formatted batch payloads.
    ///
    /// Each item is one full batch as `Vec<u8>`. For `StreamFormat::Json` and
    /// `StreamFormat::NdJson` the bytes hold one JSON object per frame, one
    /// per line (NDJSON-of-objects, #167). The stream item type is binary
    /// (`Vec<u8>`, not `String`) for symmetry with `AdaptiveFrameStream` and
    /// to leave room for future per-batch compression (#226).
    pub fn into_stream(
        self,
    ) -> impl Stream<Item = Result<Vec<u8>, StreamTransportError>> + Send + 'static {
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
                    let bytes = format_batch_owned(&batch, format)?;
                    batch.clear();
                    yield bytes;
                }
            }

            if !batch.is_empty() {
                let bytes = format_batch_owned(&batch, format)?;
                yield bytes;
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
}

impl PartialEq for PriorityFrame {
    fn eq(&self, other: &Self) -> bool {
        self.frame.priority() == other.frame.priority()
            && self.frame.sequence() == other.frame.sequence()
    }
}

impl Eq for PriorityFrame {}

impl PartialOrd for PriorityFrame {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Orders by priority first (highest first out of the max-heap); frames of
/// equal priority tie-break on `sequence` (reversed, since the heap pops the
/// *greatest* element and lower sequence — the earlier frame — must come out
/// first), giving deterministic FIFO order within a priority tier.
impl Ord for PriorityFrame {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.frame
            .priority()
            .cmp(&other.frame.priority())
            .then_with(|| other.frame.sequence().cmp(&self.frame.sequence()))
    }
}

impl<S> PriorityFrameStream<S>
where
    S: Stream<Item = Frame> + Unpin + Send + 'static,
{
    /// Wrap a frame stream that reorders frames by priority before emitting them.
    pub fn new(stream: S, format: StreamFormat, buffer_size: usize) -> Self {
        Self {
            inner: stream,
            format,
            buffer_size,
        }
    }

    /// Consume the builder and return a `Stream` of priority-ordered, formatted
    /// frame payloads.
    ///
    /// Frames are buffered up to `buffer_size` and emitted highest-priority first.
    /// Items are `Vec<u8>` for symmetry with the rest of the streaming pipeline
    /// (#226).
    pub fn into_stream(
        self,
    ) -> impl Stream<Item = Result<Vec<u8>, StreamTransportError>> + Send + 'static {
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
                            heap.push(PriorityFrame { frame });
                        }
                        None => inner_done = true,
                    }
                }

                match heap.pop() {
                    Some(pf) => {
                        let bytes = format_frame_owned(&pf.frame, format)?;
                        yield bytes;
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
pub enum StreamTransportError {
    /// Frame failed to serialize to JSON.
    #[error("Serialization error: {0}")]
    Serialization(#[from] sonic_rs::Error),

    /// Underlying I/O or transport failure.
    #[error("IO error: {0}")]
    Io(String),

    /// Internal buffer overflowed before consumers could drain it.
    #[error("Buffer overflow")]
    BufferOverflow,

    /// The stream was closed before completing the operation.
    #[error("Stream closed")]
    StreamClosed,
}

// ---------------------------------------------------------------------------
// Response helper
// ---------------------------------------------------------------------------

/// Create a response with appropriate headers for the given streaming format.
///
/// The stream item type is `Vec<u8>` (binary). This is the canonical type for
/// both UTF-8 textual formats (`Json`, `NdJson`, `ServerSentEvents`) and binary
/// payloads (`Binary`, gzip-compressed output from
/// [`AdaptiveFrameStream::with_compression`]).
pub fn create_streaming_response<S>(
    stream: S,
    format: StreamFormat,
) -> Result<Response, StreamTransportError>
where
    S: Stream<Item = Result<Vec<u8>, StreamTransportError>> + Send + 'static,
{
    let body = axum::body::Body::from_stream(stream);

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, format.content_type())
        .header(header::CACHE_CONTROL, "no-cache");

    // No manual `Transfer-Encoding` or `Connection` here: the response body encoder
    // (hyper) owns transfer framing and connection-management headers, and
    // applications must not set either by hand. On HTTP/2 both are illegal and
    // hyper logs a WARN per response when either is present (verified: hyper
    // `proto/h2/mod.rs:50`) — real, current log spam in any HTTP/2 deployment of
    // this route, since SSE (which previously set `Connection: keep-alive`) is a
    // first-class negotiated format here. On HTTP/1.1 a manually-set
    // `Transfer-Encoding` can also collide with body-length-derived framing in
    // other code paths; that hazard is latent here (`Body::from_stream` reports
    // `BodyLength::Unknown`, so nothing on this response path derives a
    // `Content-Length` to collide with) but is not the reason for this rule — the
    // encoder owning framing is. `X-Accel-Buffering` is a reverse-proxy hint, not a
    // connection-management header, and is unaffected by either concern.
    if let StreamFormat::ServerSentEvents = format {
        response = response.header("X-Accel-Buffering", "no");
    }

    response
        .body(body)
        .map_err(|e| StreamTransportError::Io(e.to_string()))
}

/// Create a streaming response with an explicit `Content-Type`.
///
/// Use this when the stream's content-type cannot be derived from [`StreamFormat`]
/// alone — for example, when a [`BatchFrameStream`] promotes `StreamFormat::Json`
/// to `application/x-ndjson` via [`BatchFrameStream::content_type()`].
///
/// # Example
///
/// ```rust,no_run
/// # use pjson_rs::infrastructure::http::streaming::{
/// #     BatchFrameStream, StreamFormat, create_streaming_response_with_content_type,
/// # };
/// # use futures::stream;
/// # use pjson_rs::domain::entities::Frame;
/// # async fn example() -> Result<axum::response::Response, Box<dyn std::error::Error>> {
/// let frames = stream::iter(Vec::<Frame>::new());
/// let batch = BatchFrameStream::new(frames, StreamFormat::Json, 10);
/// let content_type = batch.content_type();
/// let response = create_streaming_response_with_content_type(batch.into_stream(), content_type)?;
/// # Ok(response)
/// # }
/// ```
pub fn create_streaming_response_with_content_type<S>(
    stream: S,
    content_type: &str,
) -> Result<Response, StreamTransportError>
where
    S: Stream<Item = Result<Vec<u8>, StreamTransportError>> + Send + 'static,
{
    let body = axum::body::Body::from_stream(stream);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .map_err(|e| StreamTransportError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::Frame;
    use crate::domain::value_objects::{JsonData, JsonPath, Priority, StreamId};
    use axum::http::header;
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
        let collected: Vec<Result<Vec<u8>, StreamTransportError>> =
            batch_stream.into_stream().collect().await;

        assert_eq!(
            collected.len(),
            3,
            "expected 3 batches for 5 frames with batch_size=2"
        );

        let mut total_objects = 0usize;
        for result in &collected {
            let batch_bytes = result.as_ref().expect("batch should not error");
            let batch_str = std::str::from_utf8(batch_bytes).expect("uncompressed batch is UTF-8");
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
        let collected: Vec<Result<Vec<u8>, StreamTransportError>> =
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
            .map(|bytes| {
                let v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
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
        let json_bytes = result_json[0].as_ref().unwrap();
        let json_str = std::str::from_utf8(json_bytes).unwrap();
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
        let ndjson_bytes = result_ndjson[0].as_ref().unwrap();
        let ndjson_str = std::str::from_utf8(ndjson_bytes).unwrap();
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
        let sse_bytes = result_sse[0].as_ref().unwrap();
        let sse_str = std::str::from_utf8(sse_bytes).unwrap();
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
        let binary_bytes = result_binary[0].as_ref().unwrap();
        let v: serde_json::Value = serde_json::from_slice(binary_bytes).unwrap();
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

    /// `create_streaming_response_with_content_type` sets the exact content-type
    /// provided by the caller — specifically `application/x-ndjson` when wrapping
    /// a `BatchFrameStream` that promotes `StreamFormat::Json`.
    #[tokio::test]
    async fn test_create_streaming_response_with_content_type_uses_explicit_type() {
        let frames: Vec<Frame> = (0..2).map(|_| make_skeleton_frame()).collect();
        let batch = BatchFrameStream::new(stream::iter(frames), StreamFormat::Json, 10);
        let expected_ct = batch.content_type();
        assert_eq!(
            expected_ct, "application/x-ndjson",
            "BatchFrameStream with Json format must report application/x-ndjson"
        );

        let response =
            create_streaming_response_with_content_type(batch.into_stream(), expected_ct)
                .expect("response must be built");
        let ct = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Content-Type header must be present")
            .to_str()
            .unwrap();
        assert_eq!(ct, "application/x-ndjson");
    }

    /// `create_streaming_response` uses `format.content_type()` — for
    /// `StreamFormat::Json` this is `application/json`, demonstrating the API gap
    /// that `create_streaming_response_with_content_type` was introduced to close.
    #[tokio::test]
    async fn test_create_streaming_response_uses_format_content_type() {
        let frames: Vec<Frame> = (0..1).map(|_| make_skeleton_frame()).collect();
        let batch = BatchFrameStream::new(stream::iter(frames), StreamFormat::Json, 10);
        let response = create_streaming_response(batch.into_stream(), StreamFormat::Json)
            .expect("response must be built");
        let ct = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Content-Type header must be present")
            .to_str()
            .unwrap();
        // Without the new helper, the caller is stuck with application/json.
        assert_eq!(ct, "application/json");
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
                .map(|bytes| {
                    let v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                    v["priority"].as_u64().unwrap()
                })
                .collect();

            // All frames fit in the buffer (size=10) so they must arrive fully sorted.
            assert_eq!(priorities, vec![80, 50, 30, 10]);
        });
    }

    /// `sonic_rs::to_vec` must stay parse-equivalent to `serde_json::to_vec` for
    /// the range of JSON shapes a serialized [`Frame`] can produce — the two
    /// encoders are not guaranteed byte-identical (e.g. float formatting), so
    /// this asserts round-trip equivalence per edge case and records where the
    /// raw bytes happen to diverge (informational, not a failure).
    #[test]
    fn test_sonic_rs_matches_serde_json_semantics() {
        let cases: Vec<(&str, serde_json::Value)> = vec![
            ("empty_object", serde_json::json!({})),
            ("empty_array", serde_json::json!([])),
            ("null", serde_json::Value::Null),
            (
                "unicode_and_escapes",
                serde_json::json!({
                    "s": "héllo \"quoted\" \n \t \u{0} emoji \u{1F600} \u{2028}"
                }),
            ),
            (
                "numbers",
                serde_json::json!({
                    "u64_max": u64::MAX,
                    "i64_min": i64::MIN,
                    "zero": 0,
                    "neg_zero_float": -0.0_f64,
                    "integral_float": 1.0_f64,
                    "fractional": 1234.567890123_f64,
                    "small_exp": 1.5e-10_f64,
                    "large_exp": 1.5e300_f64,
                }),
            ),
            (
                "nested",
                serde_json::json!({
                    "a": [1, 2, {"b": [null, true, false, "x"]}],
                    "c": {}
                }),
            ),
        ];

        for (name, value) in cases {
            let serde_bytes = serde_json::to_vec(&value).expect("serde_json must serialize");
            let sonic_bytes = sonic_rs::to_vec(&value).expect("sonic_rs must serialize");

            let serde_roundtrip: serde_json::Value =
                serde_json::from_slice(&serde_bytes).expect("serde_json bytes must parse");
            let sonic_roundtrip: serde_json::Value =
                serde_json::from_slice(&sonic_bytes).expect("sonic_rs bytes must parse");

            assert_eq!(
                serde_roundtrip, sonic_roundtrip,
                "case `{name}`: sonic_rs and serde_json must be semantically equivalent"
            );

            if serde_bytes != sonic_bytes {
                eprintln!(
                    "note: case `{name}` byte output differs (semantically equal) — \
                     serde_json={:?} sonic_rs={:?}",
                    String::from_utf8_lossy(&serde_bytes),
                    String::from_utf8_lossy(&sonic_bytes)
                );
            }
        }
    }

    /// `with_compression(true)` must produce a payload that round-trips through
    /// gzip — the previous `String`-based pipeline rejected gzip output as
    /// invalid UTF-8 (#226). The fix threads `Vec<u8>` end-to-end so binary
    /// gzip bytes flow unmolested.
    #[cfg(feature = "compression")]
    #[tokio::test]
    async fn test_adaptive_stream_with_compression_round_trips() {
        use std::io::Read as _;

        let frames: Vec<Frame> = (0..5).map(|_| make_skeleton_frame()).collect();
        let frame_stream = stream::iter(frames);
        let adaptive =
            AdaptiveFrameStream::new(frame_stream, StreamFormat::Json).with_compression(true);

        let collected: Vec<Result<Vec<u8>, StreamTransportError>> =
            adaptive.into_stream().collect().await;

        assert_eq!(
            collected.len(),
            5,
            "5 frames in → 5 compressed payloads out"
        );

        for result in collected {
            let compressed = result.expect("compressed payload must be Ok");
            assert_eq!(
                &compressed[..2],
                &[0x1f, 0x8b],
                "every payload must carry the gzip magic header"
            );
            let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .expect("gzip payload must decode");
            let v: serde_json::Value =
                serde_json::from_slice(&decompressed).expect("decoded JSON must parse");
            assert!(v.is_object(), "decoded payload must be a JSON frame object");
        }
    }
}
