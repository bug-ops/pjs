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

/// Re-export domain pagination limits to avoid duplication.
///
/// Infrastructure layer can depend on domain layer per Clean Architecture,
/// so we re-export these constants instead of duplicating them.
pub use crate::domain::config::limits::{
    ALLOWED_SORT_FIELDS, MAX_PAGINATION_LIMIT, MAX_PAGINATION_OFFSET,
};

/// Maximum number of health metrics per session.
///
/// Bounds HashMap allocation in get_session_health to prevent
/// unbounded growth. Currently 3 metrics; allows room for 13 more.
pub const MAX_HEALTH_METRICS: usize = 16;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_scan_limit_value() {
        assert_eq!(MAX_SCAN_LIMIT, 10_000);
        const { assert!(MAX_SCAN_LIMIT > 0) };
    }

    #[test]
    fn test_max_results_limit_value() {
        assert_eq!(MAX_RESULTS_LIMIT, 10_000);
        const { assert!(MAX_RESULTS_LIMIT > 0) };
    }

    #[test]
    fn test_domain_pagination_limits_accessible() {
        // Verify re-exported domain constants are accessible
        assert_eq!(MAX_PAGINATION_LIMIT, 1_000);
        const { assert!(MAX_PAGINATION_LIMIT > 0) };
        assert_eq!(MAX_PAGINATION_OFFSET, 1_000_000);
        const { assert!(MAX_PAGINATION_OFFSET > 0) };
        assert!(!ALLOWED_SORT_FIELDS.is_empty());
    }

    #[test]
    fn test_max_health_metrics_value() {
        assert_eq!(MAX_HEALTH_METRICS, 16);
        const { assert!(MAX_HEALTH_METRICS >= 3) };
    }
}
