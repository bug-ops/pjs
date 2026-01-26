//! Integration tests for bounded iteration security flow
//!
//! Tests the complete security path from entry validation through
//! bounded iteration to scan_limit_reached propagation.
//!
//! Addresses TEST-001 from security review.

use pjson_rs::domain::{
    DomainError,
    config::limits::{ALLOWED_SORT_FIELDS, MAX_PAGINATION_LIMIT, MAX_PAGINATION_OFFSET},
    ports::repositories::{Pagination, SessionQueryCriteria, SessionQueryResult, SortOrder},
};
use pjson_rs::infrastructure::adapters::{
    generic_store::InMemoryStore,
    limits::{MAX_RESULTS_LIMIT, MAX_SCAN_LIMIT},
};

/// Test the complete security path: validation at entry point
mod entry_validation_tests {
    use super::*;

    #[test]
    fn test_pagination_validation_rejects_zero_limit() {
        let pagination = Pagination {
            offset: 0,
            limit: 0,
            sort_by: None,
            sort_order: SortOrder::Ascending,
        };

        let result = pagination.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, DomainError::InvalidInput(_)));
    }

    #[test]
    fn test_pagination_validation_rejects_excessive_limit() {
        let pagination = Pagination {
            offset: 0,
            limit: MAX_PAGINATION_LIMIT + 1,
            sort_by: None,
            sort_order: SortOrder::Ascending,
        };

        let result = pagination.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_pagination_validation_rejects_excessive_offset() {
        let pagination = Pagination {
            offset: MAX_PAGINATION_OFFSET + 1,
            limit: 10,
            sort_by: None,
            sort_order: SortOrder::Ascending,
        };

        let result = pagination.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_pagination_validation_rejects_invalid_sort_field() {
        let pagination = Pagination {
            offset: 0,
            limit: 10,
            sort_by: Some("malicious_field; DROP TABLE".to_string()),
            sort_order: SortOrder::Ascending,
        };

        let result = pagination.validate();
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("invalid sort_by"));
    }

    #[test]
    fn test_pagination_validation_accepts_allowed_sort_fields() {
        for field in ALLOWED_SORT_FIELDS {
            let pagination = Pagination {
                offset: 0,
                limit: 10,
                sort_by: Some(field.to_string()),
                sort_order: SortOrder::Ascending,
            };

            assert!(
                pagination.validate().is_ok(),
                "field '{}' should be allowed",
                field
            );
        }
    }

    #[test]
    fn test_criteria_validation_rejects_invalid_range() {
        let criteria = SessionQueryCriteria {
            min_stream_count: Some(100),
            max_stream_count: Some(10), // Invalid: min > max
            ..Default::default()
        };

        let result = criteria.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_criteria_validation_rejects_empty_states() {
        let criteria = SessionQueryCriteria {
            states: Some(vec![]), // Invalid: empty vec
            ..Default::default()
        };

        let result = criteria.validate();
        assert!(result.is_err());
    }
}

/// Test bounded iteration in InMemoryStore
mod bounded_iteration_tests {
    use super::*;

    #[test]
    fn test_filter_limited_respects_scan_limit() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        // Insert many items
        for i in 0..1000 {
            store.insert(i, i);
        }

        // Filter with scan limit that stops early
        let scan_limit = 100;
        let (results, limit_reached) = store.filter_limited(|v| *v > 500, 1000, scan_limit);

        // Should have stopped after scanning 100 items
        assert!(limit_reached, "scan_limit should be reached");
        // Results depend on iteration order, but should be limited
        assert!(results.len() <= scan_limit);
    }

    #[test]
    fn test_filter_limited_respects_result_limit() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        // Insert items that all match
        for i in 0..100 {
            store.insert(i, i);
        }

        // Filter with result limit smaller than matching items
        let result_limit = 10;
        let (results, limit_reached) = store.filter_limited(|_| true, result_limit, 10000);

        assert_eq!(results.len(), result_limit);
        assert!(limit_reached, "result_limit should be reached");
    }

    #[test]
    fn test_filter_limited_no_limit_reached_for_small_dataset() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        // Insert few items
        for i in 0..5 {
            store.insert(i, i);
        }

        // Filter with generous limits
        let (results, limit_reached) = store.filter_limited(|_| true, 1000, 1000);

        assert_eq!(results.len(), 5);
        assert!(!limit_reached, "no limit should be reached");
    }

    #[test]
    fn test_filter_limited_with_production_limits() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        // Simulate large dataset
        for i in 0..(MAX_SCAN_LIMIT + 1000) as i32 {
            store.insert(i, i);
        }

        // Use production limits
        let (_, limit_reached) = store.filter_limited(|_| true, MAX_RESULTS_LIMIT, MAX_SCAN_LIMIT);

        // Should hit scan limit before result limit
        assert!(limit_reached, "production scan limit should be respected");
    }
}

/// Test scan_limit_reached propagation
mod scan_limit_propagation_tests {
    use super::*;

    #[test]
    fn test_session_query_result_propagates_scan_limit_flag() {
        // Simulate a query result where scan limit was reached
        let result = SessionQueryResult {
            sessions: vec![],
            total_count: 100,
            has_more: true,
            query_duration_ms: 50,
            scan_limit_reached: true, // This flag indicates incomplete scan
        };

        assert!(result.scan_limit_reached);
        assert!(result.has_more);
    }

    #[test]
    fn test_session_query_result_no_scan_limit_for_complete_scan() {
        let result = SessionQueryResult {
            sessions: vec![],
            total_count: 10,
            has_more: false,
            query_duration_ms: 5,
            scan_limit_reached: false, // Complete scan
        };

        assert!(!result.scan_limit_reached);
        assert!(!result.has_more);
    }
}

/// End-to-end integration test combining all security checks
mod end_to_end_security_tests {
    use super::*;

    /// Tests the complete security flow from user input to bounded query result
    /// with a dataset smaller than the requested limit (no limits hit)
    #[test]
    fn test_complete_security_flow_small_dataset() {
        // Step 1: Create store with data smaller than limit
        let store: InMemoryStore<i32, String> = InMemoryStore::new();
        for i in 0..30 {
            store.insert(i, format!("item_{}", i));
        }

        // Step 2: Simulate user pagination input - should pass validation
        let user_pagination = Pagination {
            offset: 0,
            limit: 50,                               // Within limits, larger than dataset
            sort_by: Some("created_at".to_string()), // Valid field
            sort_order: SortOrder::Descending,
        };

        assert!(
            user_pagination.validate().is_ok(),
            "valid pagination should pass"
        );

        // Step 3: Execute bounded query
        let (results, limit_reached) = store.filter_limited(
            |v| v.contains("item_"),
            user_pagination.limit,
            MAX_SCAN_LIMIT,
        );

        // Step 4: Create result with propagated flag
        let query_result = SessionQueryResult {
            sessions: vec![],
            total_count: results.len(),
            has_more: results.len() == user_pagination.limit,
            query_duration_ms: 10,
            scan_limit_reached: limit_reached,
        };

        // Verify: small dataset should not hit any limits
        assert!(!query_result.scan_limit_reached);
        assert_eq!(query_result.total_count, 30); // Got all items
        assert!(!query_result.has_more); // No more items
    }

    /// Tests that limit_reached is properly set when result limit is hit
    #[test]
    fn test_security_flow_result_limit_hit() {
        // Create store with more data than limit
        let store: InMemoryStore<i32, String> = InMemoryStore::new();
        for i in 0..500 {
            store.insert(i, format!("item_{}", i));
        }

        // Request only 50 items
        let user_pagination = Pagination {
            offset: 0,
            limit: 50,
            sort_by: None,
            sort_order: SortOrder::Ascending,
        };

        assert!(user_pagination.validate().is_ok());

        let (results, limit_reached) = store.filter_limited(
            |v| v.contains("item_"),
            user_pagination.limit,
            MAX_SCAN_LIMIT,
        );

        // limit_reached should be true because we hit result_limit
        // This indicates there may be more matching items
        assert!(limit_reached);
        assert_eq!(results.len(), 50);
    }

    /// Tests that malicious input is rejected at entry point
    #[test]
    fn test_malicious_input_rejected_at_entry() {
        // Attempt SQL injection in sort_by
        let malicious_pagination = Pagination {
            offset: 0,
            limit: 10,
            sort_by: Some("created_at; DELETE FROM sessions--".to_string()),
            sort_order: SortOrder::Ascending,
        };

        let result = malicious_pagination.validate();
        assert!(result.is_err(), "SQL injection attempt should be rejected");

        // Attempt resource exhaustion via huge offset
        let dos_pagination = Pagination {
            offset: usize::MAX / 2,
            limit: 10,
            sort_by: None,
            sort_order: SortOrder::Ascending,
        };

        let result = dos_pagination.validate();
        assert!(result.is_err(), "DoS attempt should be rejected");
    }

    /// Tests that scan limits protect against unbounded iteration
    #[test]
    fn test_scan_limit_prevents_unbounded_iteration() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        // Create dataset larger than scan limit
        let dataset_size = MAX_SCAN_LIMIT * 2;
        for i in 0..dataset_size {
            store.insert(i as i32, i as i32);
        }

        // Count iterations to verify bounded scan
        let (_, limit_reached) = store.filter_limited(|_| true, MAX_RESULTS_LIMIT, MAX_SCAN_LIMIT);

        // Must have hit scan limit, not processed entire dataset
        assert!(
            limit_reached,
            "scan limit must prevent processing entire dataset"
        );
    }

    /// Tests domain constants are properly exported
    #[test]
    fn test_domain_constants_are_accessible() {
        // Verify domain layer constants exist and have reasonable values
        const { assert!(MAX_PAGINATION_LIMIT > 0) };
        const { assert!(MAX_PAGINATION_LIMIT <= 10_000) }; // Industry standard range
        const { assert!(MAX_PAGINATION_OFFSET > 0) };
        assert!(!ALLOWED_SORT_FIELDS.is_empty());

        // Verify infrastructure constants
        const { assert!(MAX_SCAN_LIMIT > 0) };
        const { assert!(MAX_RESULTS_LIMIT > 0) };
    }
}
