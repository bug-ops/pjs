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
pub const ALLOWED_SORT_FIELDS: &[&str] = &["created_at", "updated_at", "stream_count"];

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
}
