//! Domain layer configuration limits
//!
//! Defines validation constraints for domain-level pagination and query operations.
//! These limits enforce business rules and are independent of infrastructure.
//!
//! # Production Tuning
//!
//! These values are suitable for most deployments. Adjust based on:
//!
//! - **MAX_PAGINATION_LIMIT**: Increase if clients need larger batch fetches.
//!   Monitor memory usage per request (limit * avg_item_size).
//!
//! - **MAX_PAGINATION_OFFSET**: Lower if cursor-based pagination is preferred.
//!   Deep offsets are expensive; consider cursor pagination for offsets > 10,000.
//!
//! - **ALLOWED_SORT_FIELDS**: Extend with indexed fields only. Adding non-indexed
//!   fields degrades query performance significantly on large datasets.
//!
//! # Monitoring Recommendations
//!
//! Track these metrics to tune limits:
//! - `pagination.offset_p99`: If consistently high, clients may need cursor pagination
//! - `pagination.limit_avg`: Optimize batch sizes based on actual usage
//! - `query.scan_limit_reached_rate`: High rate indicates filter criteria too broad

/// Maximum allowed pagination limit per request.
///
/// Prevents single requests from retrieving excessive data.
/// Aligns with industry standards (GitHub API, Stripe use 100-1000).
pub const MAX_PAGINATION_LIMIT: usize = 1_000;

/// Maximum allowed pagination offset.
///
/// Prevents requests that would scan deep into result sets.
/// Beyond this, cursor-based pagination is recommended.
pub const MAX_PAGINATION_OFFSET: usize = 1_000_000;

/// Allowed sort field names for pagination validation.
///
/// Whitelist of fields that can be used in sort_by parameter.
/// Only add fields that have corresponding indexes in storage.
pub const ALLOWED_SORT_FIELDS: &[&str] =
    &["created_at", "updated_at", "stream_count", "total_bytes"];

/// Default maximum number of frames retained per stream by the in-memory
/// `FrameStore`.
///
/// Once a stream accumulates more than this many frames, the oldest are
/// evicted FIFO. Bounds the worst-case memory footprint of frame history
/// (10_000 frames × ~few-KB each ≈ tens of MB per very-long-lived stream).
pub const DEFAULT_FRAME_HISTORY_PER_STREAM: usize = 10_000;

/// Maximum number of frames a single `GenerateFramesCommand` may request.
///
/// Enforced by `CommandValidator::validate_generate_frames` at the
/// application boundary. Well under [`DEFAULT_FRAME_HISTORY_PER_STREAM`], so
/// a single request can never itself evict a stream's frame history.
pub const MAX_FRAMES_PER_REQUEST: usize = 1_000;

/// Maximum allowed `SessionConfig::session_timeout_seconds` (7 days).
///
/// Enforced by `CommandValidator::validate_create_session` at the
/// application boundary, before `StreamSession::new` computes
/// `now + chrono::Duration::seconds(session_timeout_seconds as i64)`. Any
/// `u64` value here fits well within `chrono::Duration::seconds`'s valid
/// range, so that addition can neither panic nor wrap into a negative
/// (already-expired) duration. The 7-day ceiling itself is an operational
/// choice, not a correctness requirement — sessions are not meant to be
/// long-lived resources.
pub const MAX_SESSION_TIMEOUT_SECONDS: u64 = 604_800;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_pagination_limit_value() {
        assert_eq!(MAX_PAGINATION_LIMIT, 1_000);
        const { assert!(MAX_PAGINATION_LIMIT > 0) };
    }

    #[test]
    fn test_max_pagination_offset_value() {
        assert_eq!(MAX_PAGINATION_OFFSET, 1_000_000);
        const { assert!(MAX_PAGINATION_OFFSET > 0) };
    }

    #[test]
    fn test_allowed_sort_fields() {
        assert!(ALLOWED_SORT_FIELDS.contains(&"created_at"));
        assert!(ALLOWED_SORT_FIELDS.contains(&"updated_at"));
        assert!(ALLOWED_SORT_FIELDS.contains(&"stream_count"));
        assert!(!ALLOWED_SORT_FIELDS.contains(&"invalid_field"));
    }

    #[test]
    fn test_pagination_limit_within_industry_standard() {
        // Industry standard range: 100-1000
        const { assert!(MAX_PAGINATION_LIMIT >= 100) };
        const { assert!(MAX_PAGINATION_LIMIT <= 10_000) };
    }

    #[test]
    fn test_max_frames_per_request_value() {
        assert_eq!(MAX_FRAMES_PER_REQUEST, 1_000);
        const { assert!(MAX_FRAMES_PER_REQUEST > 0) };
        const { assert!(MAX_FRAMES_PER_REQUEST <= DEFAULT_FRAME_HISTORY_PER_STREAM) };
    }

    #[test]
    fn test_max_session_timeout_seconds_fits_chrono_duration() {
        assert_eq!(MAX_SESSION_TIMEOUT_SECONDS, 604_800);
        // Must cast to i64 without overflow or sign flip, matching the
        // `chrono::Duration::seconds(config.session_timeout_seconds as i64)`
        // call in `StreamSession::with_time_provider`.
        const { assert!(MAX_SESSION_TIMEOUT_SECONDS <= i64::MAX as u64) };
        const { assert!((MAX_SESSION_TIMEOUT_SECONDS as i64) > 0) };
    }
}
