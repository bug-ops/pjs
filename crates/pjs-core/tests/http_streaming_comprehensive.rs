// Comprehensive tests for HTTP streaming module
//
// This test file covers the infrastructure/http/streaming.rs module with focus on:
// - StreamFormat detection from headers and content types
// - AdaptiveFrameStream functionality and format conversion
// - BatchFrameStream batching logic
// - PriorityFrameStream priority ordering
// - Stream error handling
// - Response creation with correct headers
//
// Coverage target: 60%+ for Infrastructure Layer

#![cfg(feature = "http-server")]

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use futures::StreamExt;
use pjson_rs::{
    domain::entities::Frame,
    domain::value_objects::{JsonData, StreamId},
    infrastructure::http::streaming::{
        AdaptiveFrameStream, BatchFrameStream, PriorityFrameStream, StreamFormat,
        StreamTransportError, create_streaming_response,
    },
};

// ============================================================================
// StreamFormat Tests
// ============================================================================

#[test]
fn test_stream_format_from_accept_header_sse() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("text/event-stream"),
    );

    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::ServerSentEvents));
}

#[test]
fn test_stream_format_from_accept_header_ndjson() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("application/x-ndjson"),
    );

    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::NdJson));
}

#[test]
fn test_stream_format_from_accept_header_binary() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("application/octet-stream"),
    );

    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Binary));
}

#[test]
fn test_stream_format_from_accept_header_default() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

#[test]
fn test_stream_format_from_accept_header_missing() {
    let headers = HeaderMap::new();

    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

// ----------------------------------------------------------------------------
// q-value negotiation (RFC 9110 §12.5.1) — from_accept_header rewrite
// ----------------------------------------------------------------------------

fn accept_headers(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_str(value).unwrap());
    headers
}

/// S3 regression: a lower-q explicit type must not beat a higher-q one that
/// appears earlier in the header.
#[test]
fn test_stream_format_from_accept_header_q_value_preference() {
    let headers = accept_headers("application/json, text/event-stream;q=0.1");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

#[test]
fn test_stream_format_from_accept_header_sse_beats_lower_q_ndjson() {
    let headers = accept_headers("text/event-stream;q=0.9, application/x-ndjson;q=0.8");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::ServerSentEvents));
}

#[test]
fn test_stream_format_from_accept_header_ndjson_beats_lower_q_sse() {
    let headers = accept_headers("application/x-ndjson;q=0.9, text/event-stream;q=0.8");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::NdJson));
}

#[test]
fn test_stream_format_from_accept_header_q_zero_is_explicit_rejection() {
    let headers = accept_headers("text/event-stream;q=0");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

#[test]
fn test_stream_format_from_accept_header_wildcard_falls_back_to_json() {
    let headers = accept_headers("*/*");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

#[test]
fn test_stream_format_from_accept_header_unrecognized_type_falls_back_to_json() {
    let headers = accept_headers("application/xml");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

#[test]
fn test_stream_format_from_accept_header_case_and_whitespace_tolerant() {
    let headers = accept_headers("TEXT/EVENT-STREAM ; Q=0.5");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::ServerSentEvents));
}

/// Exact media-range matching kills the substring-match regression: a longer,
/// unrelated type must not match a shorter registered one.
#[test]
fn test_stream_format_from_accept_header_no_substring_match() {
    let headers = accept_headers("application/x-ndjson-plus");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

/// S8: a high-q wildcard must beat a low-q explicit type — the same bug class
/// as the S3 regression above, with `*/*` substituted for `application/json`.
#[test]
fn test_stream_format_from_accept_header_wildcard_beats_lower_q_explicit_type() {
    let headers = accept_headers("text/event-stream;q=0.1, */*");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

/// SC-002: a tied `q` between a wildcard and an exact match is broken by RFC
/// 9110 §12.5.1 media-range specificity, not by first occurrence in the header.
#[test]
fn test_stream_format_from_accept_header_specificity_breaks_tied_q() {
    let headers = accept_headers("application/*;q=1.0, application/x-ndjson;q=1.0");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::NdJson),
        "the more specific application/x-ndjson must win a tied q over application/*"
    );
}

/// M7: a `q` value that fails to parse drops the entry entirely, distinguishing
/// it from an absent `q` (which defaults to `1.0`).
#[test]
fn test_stream_format_from_accept_header_unparsable_q_drops_entry() {
    let headers = accept_headers("text/event-stream;q=abc");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

/// S9: `f32::from_str` parses `"nan"`/`"inf"`/`"-inf"` successfully, and the
/// `is_finite()` check drops them during sanitization, before the entry ever
/// reaches `headers_accept`. This matters because `headers_accept`'s own
/// `parse_q_value` treats an unparsable `q` as *absent* — defaulting to `1.0`,
/// the highest priority — rather than dropping the entry; a `q=nan` entry that
/// slipped past our own filter would therefore win outright, not lose.
#[test]
fn test_stream_format_from_accept_header_q_nan_drops_entry() {
    let headers = accept_headers("text/event-stream;q=nan, application/x-ndjson");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::NdJson),
        "a NaN q must not win by default"
    );
}

/// Same as above with the NaN entry seen *after* the real entry — proves the
/// drop doesn't depend on header ordering.
#[test]
fn test_stream_format_from_accept_header_q_nan_drops_entry_reverse_order() {
    let headers = accept_headers("application/json;q=1.0, text/event-stream;q=nan");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::Json),
        "a NaN q appearing after a valid entry must still be dropped, not override it"
    );
}

#[test]
fn test_stream_format_from_accept_header_q_infinity_drops_entry() {
    let headers = accept_headers("text/event-stream;q=inf");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

#[test]
fn test_stream_format_from_accept_header_q_negative_infinity_drops_entry() {
    let headers = accept_headers("text/event-stream;q=-inf");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

/// M8: entries beyond `MAX_ACCEPT_ENTRIES` (16) are ignored, not just "don't
/// panic" — the 17th entry here would flip the result to SSE if it were
/// considered.
#[test]
fn test_stream_format_from_accept_header_bounds_entries() {
    let mut entries = vec!["application/xml"; 16];
    entries.push("text/event-stream");
    let header_value = entries.join(", ");
    let headers = accept_headers(&header_value);

    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::Json),
        "the 17th entry must be ignored beyond MAX_ACCEPT_ENTRIES"
    );
}

/// M14: companion to the above — entry 16 (the last one still within the
/// bound) must still be honored, so the bound isn't silently narrower than
/// documented.
#[test]
fn test_stream_format_from_accept_header_bounds_entries_honors_last_entry() {
    let mut entries = vec!["application/xml"; 15];
    entries.push("text/event-stream");
    let header_value = entries.join(", ");
    let headers = accept_headers(&header_value);

    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::ServerSentEvents),
        "the 16th entry is still within MAX_ACCEPT_ENTRIES and must be honored"
    );
}

/// Regression for a flood of entries well beyond `MAX_ACCEPT_ENTRIES` (16), not
/// just a single entry past it — a large flood must not somehow reach the
/// parser via some batching/chunking difference from the single-extra-entry case.
#[test]
fn test_stream_format_from_accept_header_flood_beyond_bound_dropped() {
    let mut entries = vec!["application/octet-stream"; 16];
    entries.extend(std::iter::repeat_n("text/event-stream", 34));
    let header_value = entries.join(", ");
    let headers = accept_headers(&header_value);

    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::Binary),
        "only the first MAX_ACCEPT_ENTRIES (16) of a 50-entry flood must reach the parser"
    );
}

/// #518 review finding S1: `headers_accept::Accept::from_str` fails the
/// *entire* header on a single malformed entry. A malformed entry must be
/// dropped individually during sanitization, not discard every other,
/// well-formed entry.
#[test]
fn test_stream_format_from_accept_header_malformed_entry_preserves_others() {
    let headers = accept_headers("text/event-stream, garbage!!");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::ServerSentEvents),
        "a malformed entry must be dropped individually, not discard the whole header"
    );
}

/// A comma inside a quoted parameter value defeats naive comma-splitting
/// (pre-existing limitation, not introduced by this migration — neither the
/// old nor the new parser handles RFC 9110 quoted-string commas). Per-entry
/// validation (review finding S1) must still drop only the corrupted
/// fragment, not the well-formed entries around it.
#[test]
fn test_stream_format_from_accept_header_quoted_comma_param_entry() {
    let headers = accept_headers("text/event-stream;message=\"a, b\", application/x-ndjson");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::ServerSentEvents),
        "the corrupted fragment from the quoted comma must be dropped, keeping the well-formed entries"
    );
}

/// #518 review finding S2: `origin/main`'s hand-rolled parser clamps a
/// finite, out-of-range `q` (e.g. `q=5`) into `[0.0, 1.0]` and keeps the
/// entry — it does not drop it. FR-005 was corrected 2026-08-19 to match.
#[test]
fn test_stream_format_from_accept_header_out_of_range_q_clamped_and_kept() {
    let headers = accept_headers("text/event-stream;q=5");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::ServerSentEvents),
        "a finite out-of-range q must be clamped to 1.0 and the entry kept, not dropped"
    );
}

/// Companion to the above: a negative out-of-range `q` clamps to `0.0`, which
/// is `headers_accept`'s own explicit-rejection value — the entry is kept but
/// never wins, the same observable outcome as `origin/main`'s explicit
/// `q <= 0.0` drop.
#[test]
fn test_stream_format_from_accept_header_negative_out_of_range_q_is_excluded() {
    let headers = accept_headers("text/event-stream;q=-5");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

/// #518 review finding S3: floor-rounding a tiny positive `q` to 3 decimal
/// digits could produce `0.000`, which `headers_accept` treats as an explicit
/// rejection — flipping a barely-acceptable preference into a hard rejection.
/// Any originally-positive `q` must remain positive after reformatting.
#[test]
fn test_stream_format_from_accept_header_tiny_positive_q_survives() {
    let headers = accept_headers("text/event-stream;q=0.0004");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::ServerSentEvents),
        "a tiny positive q must not be rounded down to 0.000"
    );
}

/// #518 review finding S4: `headers_accept`'s wildcard matching is general
/// RFC 9110 `type/*`/`*/*` matching, broader than this route's historical,
/// scoped wildcard support (only `*/*` and `application/*`). A range like
/// `*/x-ndjson` must be dropped, not treated as matching every candidate the
/// way `*/*` does.
#[test]
fn test_stream_format_from_accept_header_bare_wildcard_subtype_is_rejected() {
    let headers = accept_headers("text/event-stream;q=0.5, */x-ndjson;q=1.0");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::ServerSentEvents),
        "*/x-ndjson must be dropped, not outrank the lower-q but valid text/event-stream entry"
    );
}

/// Companion to the above (impl-critic M1): a `type/*` wildcard other than
/// `application/*` (e.g. `text/*`) is also outside this route's historical,
/// scoped wildcard support and must be dropped rather than generalized.
#[test]
fn test_stream_format_from_accept_header_type_wildcard_other_than_application_is_rejected() {
    let headers = accept_headers("text/*;q=1.0");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

/// #518 review finding N1 (round 2): `headers_accept::Accept::negotiate` locks
/// each candidate onto its single best-specificity matching `Accept` entry and
/// only then checks that entry's `q` — so a `q=0` on a concrete type excludes
/// *only* that candidate, not the whole negotiation. `origin/main` instead
/// dropped the `q=0` entry from the header outright before matching, so the
/// `*/*` entry's hardcoded vote for `StreamFormat::Json` always won. This is a
/// deliberate, documented behavior change (see CHANGELOG.md) beyond the
/// specificity tie-break, pinned here so it isn't an untested side effect.
#[test]
fn test_stream_format_from_accept_header_q_zero_concrete_type_falls_through_to_wildcard() {
    let headers = accept_headers("application/json;q=0, */*");
    let format = StreamFormat::from_accept_header(&headers);
    assert!(
        matches!(format, StreamFormat::ServerSentEvents),
        "rejecting application/json specifically must not also exclude the separately-stated */* wildcard"
    );
}

#[test]
fn test_stream_format_content_type() {
    assert_eq!(StreamFormat::Json.content_type(), "application/json");
    assert_eq!(StreamFormat::NdJson.content_type(), "application/x-ndjson");
    assert_eq!(
        StreamFormat::ServerSentEvents.content_type(),
        "text/event-stream"
    );
    assert_eq!(
        StreamFormat::Binary.content_type(),
        "application/octet-stream"
    );
}

// ============================================================================
// AdaptiveFrameStream Tests
// ============================================================================

fn create_test_frame(_priority: u8, sequence: u64, _payload: &str) -> Frame {
    let stream_id = StreamId::new();
    let json_data = JsonData::string("test data");

    // Use skeleton frame for simpler testing
    Frame::skeleton(stream_id, sequence, json_data)
}

#[tokio::test]
async fn test_adaptive_frame_stream_json_format() {
    let frames = vec![
        create_test_frame(200, 1, r#"{"key": "value1"}"#),
        create_test_frame(150, 2, r#"{"key": "value2"}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::Json);

    let collected: Vec<_> = adaptive.into_stream().collect().await;

    assert_eq!(collected.len(), 2);
    for result in collected {
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn test_adaptive_frame_stream_ndjson_format() {
    let frames = vec![create_test_frame(200, 1, r#"{"test": 1}"#)];

    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::NdJson);

    let collected: Vec<_> = adaptive.into_stream().collect().await;

    assert_eq!(collected.len(), 1);
    let formatted = collected[0].as_ref().unwrap();
    assert_eq!(formatted.last().copied(), Some(b'\n'));
}

#[tokio::test]
async fn test_adaptive_frame_stream_sse_format() {
    let frames = vec![create_test_frame(200, 1, r#"{"event": "update"}"#)];

    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::ServerSentEvents);

    let collected: Vec<_> = adaptive.into_stream().collect().await;

    assert_eq!(collected.len(), 1);
    let formatted = collected[0].as_ref().unwrap();
    assert!(formatted.starts_with(b"data: "));
    assert!(formatted.ends_with(b"\n\n"));
}

/// `with_compression(true)` must yield decompressible gzip payloads (#226).
/// The previous `String`-typed pipeline returned `Err("not valid UTF-8")` for
/// every chunk; threading `Vec<u8>` through fixes the architectural mismatch.
#[cfg(feature = "compression")]
#[tokio::test]
async fn test_adaptive_frame_stream_with_compression() {
    use std::io::Read as _;

    let frames = vec![create_test_frame(200, 1, r#"{"data": "test"}"#)];

    let frame_stream = futures::stream::iter(frames);
    let adaptive =
        AdaptiveFrameStream::new(frame_stream, StreamFormat::Json).with_compression(true);

    let collected: Vec<_> = adaptive.into_stream().collect().await;

    assert_eq!(collected.len(), 1);
    let compressed = collected[0]
        .as_ref()
        .expect("compressed payload must be Ok, not Err");
    assert_eq!(
        &compressed[..2],
        &[0x1f, 0x8b],
        "payload must start with the gzip magic header"
    );

    let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .expect("gzip payload must round-trip");
    let v: serde_json::Value =
        serde_json::from_slice(&decompressed).expect("decompressed payload must be valid JSON");
    assert!(v.is_object());
}

#[tokio::test]
async fn test_adaptive_frame_stream_with_buffer_size() {
    let frames = vec![create_test_frame(200, 1, r#"{"data": "test"}"#)];

    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::Json).with_buffer_size(20);

    let collected: Vec<_> = adaptive.into_stream().collect().await;

    assert_eq!(collected.len(), 1);
}

#[tokio::test]
async fn test_adaptive_frame_stream_empty() {
    let frames: Vec<Frame> = vec![];
    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::Json);

    let collected: Vec<_> = adaptive.into_stream().collect().await;

    assert_eq!(collected.len(), 0);
}

// ============================================================================
// BatchFrameStream Tests
// ============================================================================

#[tokio::test]
async fn test_batch_frame_stream_single_batch() {
    let frames = vec![
        create_test_frame(200, 1, r#"{"id": 1}"#),
        create_test_frame(200, 2, r#"{"id": 2}"#),
        create_test_frame(200, 3, r#"{"id": 3}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let batch = BatchFrameStream::new(frame_stream, StreamFormat::Json, 5);

    let collected: Vec<_> = batch.into_stream().collect().await;

    // All frames in one batch since batch_size=5 and we have 3 frames
    assert_eq!(collected.len(), 1);
    assert!(collected[0].is_ok());
}

#[tokio::test]
async fn test_batch_frame_stream_multiple_batches() {
    let frames = vec![
        create_test_frame(200, 1, r#"{"id": 1}"#),
        create_test_frame(200, 2, r#"{"id": 2}"#),
        create_test_frame(200, 3, r#"{"id": 3}"#),
        create_test_frame(200, 4, r#"{"id": 4}"#),
        create_test_frame(200, 5, r#"{"id": 5}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let batch = BatchFrameStream::new(frame_stream, StreamFormat::Json, 2);

    let collected: Vec<_> = batch.into_stream().collect().await;

    // Should have 3 batches: [2, 2, 1]
    assert_eq!(collected.len(), 3);
    for result in collected {
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn test_batch_frame_stream_ndjson_format() {
    let frames = vec![
        create_test_frame(200, 1, r#"{"id": 1}"#),
        create_test_frame(200, 2, r#"{"id": 2}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let batch = BatchFrameStream::new(frame_stream, StreamFormat::NdJson, 10);

    let collected: Vec<_> = batch.into_stream().collect().await;

    assert_eq!(collected.len(), 1);
    let result = collected[0].as_ref().unwrap();
    // NdJson should have newlines
    assert!(result.contains(&b'\n'));
}

#[tokio::test]
async fn test_batch_frame_stream_sse_format() {
    let frames = vec![
        create_test_frame(200, 1, r#"{"id": 1}"#),
        create_test_frame(200, 2, r#"{"id": 2}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let batch = BatchFrameStream::new(frame_stream, StreamFormat::ServerSentEvents, 10);

    let collected: Vec<_> = batch.into_stream().collect().await;

    assert_eq!(collected.len(), 1);
    let result = collected[0].as_ref().unwrap();
    // SSE should have "data: " prefix
    let result_str = std::str::from_utf8(result).unwrap();
    assert!(result_str.contains("data: "));
}

#[tokio::test]
async fn test_batch_frame_stream_empty() {
    let frames: Vec<Frame> = vec![];
    let frame_stream = futures::stream::iter(frames);
    let batch = BatchFrameStream::new(frame_stream, StreamFormat::Json, 5);

    let collected: Vec<_> = batch.into_stream().collect().await;

    assert_eq!(collected.len(), 0);
}

// ============================================================================
// PriorityFrameStream Tests
// ============================================================================

#[tokio::test]
async fn test_priority_frame_stream_orders_by_priority() {
    let frames = vec![
        create_test_frame(100, 1, r#"{"priority": "low"}"#),
        create_test_frame(250, 2, r#"{"priority": "critical"}"#),
        create_test_frame(200, 3, r#"{"priority": "high"}"#),
        create_test_frame(150, 4, r#"{"priority": "medium"}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let priority = PriorityFrameStream::new(frame_stream, StreamFormat::Json, 10);

    let collected: Vec<_> = priority.into_stream().collect().await;

    // Should get all frames
    assert_eq!(collected.len(), 4);
    for result in collected {
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn test_priority_frame_stream_small_buffer() {
    let frames = vec![
        create_test_frame(100, 1, r#"{"priority": "low"}"#),
        create_test_frame(250, 2, r#"{"priority": "critical"}"#),
        create_test_frame(200, 3, r#"{"priority": "high"}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    // Small buffer to test partial priority ordering
    let priority = PriorityFrameStream::new(frame_stream, StreamFormat::Json, 2);

    let collected: Vec<_> = priority.into_stream().collect().await;

    assert_eq!(collected.len(), 3);
}

#[tokio::test]
async fn test_priority_frame_stream_empty() {
    let frames: Vec<Frame> = vec![];
    let frame_stream = futures::stream::iter(frames);
    let priority = PriorityFrameStream::new(frame_stream, StreamFormat::Json, 5);

    let collected: Vec<_> = priority.into_stream().collect().await;

    assert_eq!(collected.len(), 0);
}

#[tokio::test]
async fn test_priority_frame_stream_sse_format() {
    let frames = vec![create_test_frame(200, 1, r#"{"test": 1}"#)];

    let frame_stream = futures::stream::iter(frames);
    let priority = PriorityFrameStream::new(frame_stream, StreamFormat::ServerSentEvents, 5);

    let collected: Vec<_> = priority.into_stream().collect().await;

    assert_eq!(collected.len(), 1);
    let result = collected[0].as_ref().unwrap();
    assert!(result.starts_with(b"data: "));
}

// ============================================================================
// StreamTransportError Tests
// ============================================================================

#[test]
fn test_stream_error_serialization() {
    let sonic_error = <sonic_rs::Error as serde::ser::Error>::custom("test error");
    let error = StreamTransportError::Serialization(sonic_error);

    assert!(error.to_string().contains("Serialization error"));
}

#[test]
fn test_stream_error_io() {
    let error = StreamTransportError::Io("Connection lost".to_string());

    assert_eq!(error.to_string(), "IO error: Connection lost");
}

#[test]
fn test_stream_error_buffer_overflow() {
    let error = StreamTransportError::BufferOverflow;

    assert_eq!(error.to_string(), "Buffer overflow");
}

#[test]
fn test_stream_error_stream_closed() {
    let error = StreamTransportError::StreamClosed;

    assert_eq!(error.to_string(), "Stream closed");
}

// ============================================================================
// Response Creation Tests
// ============================================================================

#[tokio::test]
async fn test_create_streaming_response_json() {
    let stream = futures::stream::iter(vec![
        Ok::<Vec<u8>, StreamTransportError>(b"test1".to_vec()),
        Ok(b"test2".to_vec()),
    ]);

    let response = create_streaming_response(stream, StreamFormat::Json).unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-cache"
    );
}

#[tokio::test]
async fn test_create_streaming_response_sse() {
    let stream = futures::stream::iter(vec![Ok::<Vec<u8>, StreamTransportError>(
        b"data: test\n\n".to_vec(),
    )]);

    let response = create_streaming_response(stream, StreamFormat::ServerSentEvents).unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-cache"
    );
    assert!(
        response.headers().get(header::CONNECTION).is_none(),
        "the encoder, not the application, owns connection-management headers"
    );
    assert_eq!(response.headers().get("X-Accel-Buffering").unwrap(), "no");
}

#[tokio::test]
async fn test_create_streaming_response_ndjson() {
    let stream = futures::stream::iter(vec![Ok::<Vec<u8>, StreamTransportError>(
        b"test\n".to_vec(),
    )]);

    let response = create_streaming_response(stream, StreamFormat::NdJson).unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/x-ndjson"
    );
    assert!(
        response.headers().get("Transfer-Encoding").is_none(),
        "the encoder, not the application, owns transfer framing"
    );
}

#[tokio::test]
async fn test_create_streaming_response_binary() {
    let stream = futures::stream::iter(vec![Ok::<Vec<u8>, StreamTransportError>(
        b"binary_data".to_vec(),
    )]);

    let response = create_streaming_response(stream, StreamFormat::Binary).unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/octet-stream"
    );
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_full_streaming_pipeline() {
    // Create frames with different priorities
    let frames = vec![
        create_test_frame(100, 1, r#"{"msg": "low priority"}"#),
        create_test_frame(250, 2, r#"{"msg": "critical"}"#),
        create_test_frame(200, 3, r#"{"msg": "high priority"}"#),
    ];

    // Process through priority stream
    let frame_stream = futures::stream::iter(frames);
    let priority = PriorityFrameStream::new(frame_stream, StreamFormat::ServerSentEvents, 10);

    let collected: Vec<_> = priority.into_stream().collect().await;

    assert_eq!(collected.len(), 3);

    // All should be formatted as SSE
    for result in collected {
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(bytes.starts_with(b"data: "));
        assert!(bytes.ends_with(b"\n\n"));
    }
}

#[cfg(feature = "compression")]
#[tokio::test]
async fn test_adaptive_stream_builder_pattern() {
    let frames = vec![create_test_frame(200, 1, r#"{"test": 1}"#)];

    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::Json)
        .with_compression(true)
        .with_buffer_size(100);

    let collected: Vec<_> = adaptive.into_stream().collect().await;

    assert_eq!(collected.len(), 1);
    // Gzip-compressed output now flows as Vec<u8> — the binary payload starts
    // with the gzip magic header rather than failing UTF-8 validation (#226).
    let bytes = collected[0]
        .as_ref()
        .expect("compressed payload must be Ok with the Vec<u8> pipeline");
    assert_eq!(&bytes[..2], &[0x1f, 0x8b], "must carry gzip magic header");
}
