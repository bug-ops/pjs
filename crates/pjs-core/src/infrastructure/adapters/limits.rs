//! Security limits for repository operations
//!
//! Centralizes all bounded iteration and allocation limits for protection
//! against denial-of-service attacks and memory exhaustion.
//!
//! # Rationale
//!
//! These limits are chosen based on typical workload analysis:
//! - At ~1KB per session, MAX_SCAN_LIMIT caps memory at ~10MB
//! - Iteration time capped at ~10ms (assuming 1us/item)
//! - Pagination limits align with industry standards (GitHub API, Stripe)

/// Maximum number of items to scan in filter operations before aborting.
///
/// Prevents CPU exhaustion from queries scanning millions of items.
/// At ~1us per item, this caps iteration time at ~10ms.
pub const MAX_SCAN_LIMIT: usize = 10_000;

/// Maximum number of results to return from filter operations.
///
/// Prevents unbounded memory allocation for large result sets.
/// Matches MAX_SCAN_LIMIT for consistency.
pub const MAX_RESULTS_LIMIT: usize = 10_000;

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

/// Maximum number of health metrics per session.
///
/// Bounds HashMap allocation in get_session_health to prevent
/// unbounded growth. Currently 3 metrics; allows room for 13 more.
pub const MAX_HEALTH_METRICS: usize = 16;

/// Allowed sort field names for pagination validation.
pub const ALLOWED_SORT_FIELDS: &[&str] = &["created_at", "updated_at", "stream_count"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_scan_limit_value() {
        assert_eq!(MAX_SCAN_LIMIT, 10_000);
        assert!(MAX_SCAN_LIMIT > 0);
    }

    #[test]
    fn test_max_results_limit_value() {
        assert_eq!(MAX_RESULTS_LIMIT, 10_000);
        assert!(MAX_RESULTS_LIMIT > 0);
    }

    #[test]
    fn test_max_pagination_limit_value() {
        assert_eq!(MAX_PAGINATION_LIMIT, 1_000);
        assert!(MAX_PAGINATION_LIMIT > 0);
        assert!(MAX_PAGINATION_LIMIT <= MAX_RESULTS_LIMIT);
    }

    #[test]
    fn test_max_pagination_offset_value() {
        assert_eq!(MAX_PAGINATION_OFFSET, 1_000_000);
        assert!(MAX_PAGINATION_OFFSET > 0);
    }

    #[test]
    fn test_max_health_metrics_value() {
        assert_eq!(MAX_HEALTH_METRICS, 16);
        assert!(MAX_HEALTH_METRICS >= 3);
    }

    #[test]
    fn test_allowed_sort_fields() {
        assert!(ALLOWED_SORT_FIELDS.contains(&"created_at"));
        assert!(ALLOWED_SORT_FIELDS.contains(&"updated_at"));
        assert!(ALLOWED_SORT_FIELDS.contains(&"stream_count"));
        assert!(!ALLOWED_SORT_FIELDS.contains(&"invalid_field"));
    }
}
