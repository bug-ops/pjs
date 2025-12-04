//! Tests for domain ports (trait definitions and supporting types)
//!
//! These tests ensure that the port interfaces are well-defined and
//! that supporting types behave correctly.

use pjson_rs::domain::ports::{
    BackpressureStrategy, CacheStatistics, ConnectionMetrics, ConnectionState, Pagination,
    SessionQueryCriteria, SessionQueryResult, SortOrder, StreamFilter, StreamMetadata,
    StreamStatistics, StreamStatus, SystemTimeProvider, TimeProvider, WriterConfig, WriterMetrics,
};
use pjson_rs::domain::value_objects::Priority;
use std::collections::HashMap;
use std::time::Duration;

// TimeProvider tests

#[test]
fn test_system_time_provider_now() {
    let provider = SystemTimeProvider;
    let now = provider.now();

    // Verify time is recent (within last minute)
    let diff = chrono::Utc::now() - now;
    assert!(diff.num_seconds().abs() < 60);
}

#[test]
fn test_system_time_provider_now_millis() {
    let provider = SystemTimeProvider;
    let millis = provider.now_millis();

    // Verify timestamp is reasonable (after 2020)
    assert!(millis > 1577836800000); // Jan 1, 2020
}

#[test]
fn test_system_time_provider_clone() {
    let provider = SystemTimeProvider;
    let provider2 = provider.clone();

    let time1 = provider.now_millis();
    let time2 = provider2.now_millis();

    // Times should be very close
    assert!((time1 as i64 - time2 as i64).abs() < 1000);
}

#[test]
fn test_system_time_provider_debug() {
    let provider = SystemTimeProvider;
    let debug_str = format!("{:?}", provider);
    assert!(debug_str.contains("SystemTimeProvider"));
}

// Pagination tests

#[test]
fn test_pagination_default() {
    let pagination = Pagination::default();
    assert_eq!(pagination.offset, 0);
    assert_eq!(pagination.limit, 50);
    assert_eq!(pagination.sort_by, None);
    assert_eq!(pagination.sort_order, SortOrder::Ascending);
}

#[test]
fn test_pagination_custom() {
    let pagination = Pagination {
        offset: 100,
        limit: 20,
        sort_by: Some("created_at".to_string()),
        sort_order: SortOrder::Descending,
    };

    assert_eq!(pagination.offset, 100);
    assert_eq!(pagination.limit, 20);
    assert_eq!(pagination.sort_by.unwrap(), "created_at");
    assert_eq!(pagination.sort_order, SortOrder::Descending);
}

#[test]
fn test_pagination_clone() {
    let pagination = Pagination {
        offset: 50,
        limit: 25,
        sort_by: Some("priority".to_string()),
        sort_order: SortOrder::Ascending,
    };

    let cloned = pagination.clone();
    assert_eq!(cloned.offset, 50);
    assert_eq!(cloned.limit, 25);
}

#[test]
fn test_pagination_debug() {
    let pagination = Pagination::default();
    let debug_str = format!("{:?}", pagination);
    assert!(debug_str.contains("Pagination"));
}

// SortOrder tests

#[test]
fn test_sort_order_equality() {
    assert_eq!(SortOrder::Ascending, SortOrder::Ascending);
    assert_eq!(SortOrder::Descending, SortOrder::Descending);
    assert_ne!(SortOrder::Ascending, SortOrder::Descending);
}

#[test]
fn test_sort_order_clone() {
    let order = SortOrder::Ascending;
    let cloned = order.clone();
    assert_eq!(order, cloned);
}

#[test]
fn test_sort_order_debug() {
    let ascending = SortOrder::Ascending;
    let descending = SortOrder::Descending;

    assert!(format!("{:?}", ascending).contains("Ascending"));
    assert!(format!("{:?}", descending).contains("Descending"));
}

// SessionQueryCriteria tests

#[test]
fn test_session_query_criteria_default() {
    let criteria = SessionQueryCriteria {
        states: None,
        created_after: None,
        created_before: None,
        client_info_pattern: None,
        has_active_streams: None,
        min_stream_count: None,
        max_stream_count: None,
    };

    assert!(criteria.states.is_none());
    assert!(criteria.created_after.is_none());
}

#[test]
fn test_session_query_criteria_with_filters() {
    let now = chrono::Utc::now();

    let criteria = SessionQueryCriteria {
        states: Some(vec!["active".to_string(), "pending".to_string()]),
        created_after: Some(now),
        created_before: Some(now + chrono::Duration::hours(1)),
        client_info_pattern: Some("test-*".to_string()),
        has_active_streams: Some(true),
        min_stream_count: Some(1),
        max_stream_count: Some(10),
    };

    assert_eq!(criteria.states.as_ref().unwrap().len(), 2);
    assert!(criteria.created_after.is_some());
    assert_eq!(criteria.client_info_pattern.unwrap(), "test-*");
}

#[test]
fn test_session_query_criteria_clone() {
    let criteria = SessionQueryCriteria {
        states: Some(vec!["active".to_string()]),
        created_after: None,
        created_before: None,
        client_info_pattern: Some("pattern".to_string()),
        has_active_streams: Some(true),
        min_stream_count: Some(5),
        max_stream_count: Some(20),
    };

    let cloned = criteria.clone();
    assert_eq!(cloned.states.unwrap().len(), 1);
    assert_eq!(cloned.min_stream_count, Some(5));
}

// StreamFilter tests

#[test]
fn test_stream_filter_empty() {
    let filter = StreamFilter {
        statuses: None,
        min_priority: None,
        max_priority: None,
        created_after: None,
        has_frames: None,
    };

    assert!(filter.statuses.is_none());
    assert!(filter.min_priority.is_none());
}

#[test]
fn test_stream_filter_with_criteria() {
    let filter = StreamFilter {
        statuses: Some(vec![StreamStatus::Active, StreamStatus::Completed]),
        min_priority: Some(Priority::LOW),
        max_priority: Some(Priority::HIGH),
        created_after: Some(chrono::Utc::now()),
        has_frames: Some(true),
    };

    assert_eq!(filter.statuses.as_ref().unwrap().len(), 2);
    assert!(filter.min_priority.is_some());
    assert!(filter.has_frames.unwrap());
}

#[test]
fn test_stream_filter_clone() {
    let filter = StreamFilter {
        statuses: Some(vec![StreamStatus::Active]),
        min_priority: Some(Priority::MEDIUM),
        max_priority: None,
        created_after: None,
        has_frames: Some(false),
    };

    let cloned = filter.clone();
    assert_eq!(cloned.statuses.as_ref().unwrap().len(), 1);
    assert_eq!(cloned.has_frames, Some(false));
}

// StreamStatus tests

#[test]
fn test_stream_status_variants() {
    let statuses = vec![
        StreamStatus::Created,
        StreamStatus::Active,
        StreamStatus::Paused,
        StreamStatus::Completed,
        StreamStatus::Failed,
        StreamStatus::Cancelled,
    ];

    assert_eq!(statuses.len(), 6);
}

#[test]
fn test_stream_status_equality() {
    assert_eq!(StreamStatus::Active, StreamStatus::Active);
    assert_ne!(StreamStatus::Active, StreamStatus::Paused);
}

#[test]
fn test_stream_status_clone() {
    let status = StreamStatus::Active;
    let cloned = status.clone();
    assert_eq!(status, cloned);
}

#[test]
fn test_stream_status_debug() {
    let status = StreamStatus::Completed;
    let debug_str = format!("{:?}", status);
    assert!(debug_str.contains("Completed"));
}

// StreamMetadata tests

#[test]
fn test_stream_metadata_empty() {
    let metadata = StreamMetadata {
        tags: HashMap::new(),
        content_type: None,
        estimated_size: None,
        priority_hints: vec![],
    };

    assert!(metadata.tags.is_empty());
    assert!(metadata.content_type.is_none());
}

#[test]
fn test_stream_metadata_with_data() {
    let mut tags = HashMap::new();
    tags.insert("environment".to_string(), "production".to_string());
    tags.insert("version".to_string(), "1.0".to_string());

    let metadata = StreamMetadata {
        tags,
        content_type: Some("application/json".to_string()),
        estimated_size: Some(1024 * 1024),
        priority_hints: vec![Priority::HIGH, Priority::MEDIUM],
    };

    assert_eq!(metadata.tags.len(), 2);
    assert_eq!(metadata.content_type.unwrap(), "application/json");
    assert_eq!(metadata.estimated_size.unwrap(), 1024 * 1024);
    assert_eq!(metadata.priority_hints.len(), 2);
}

#[test]
fn test_stream_metadata_clone() {
    let mut tags = HashMap::new();
    tags.insert("key".to_string(), "value".to_string());

    let metadata = StreamMetadata {
        tags: tags.clone(),
        content_type: Some("text/plain".to_string()),
        estimated_size: Some(512),
        priority_hints: vec![Priority::LOW],
    };

    let cloned = metadata.clone();
    assert_eq!(cloned.tags.len(), 1);
    assert_eq!(cloned.estimated_size, Some(512));
}

// WriterConfig tests

#[test]
fn test_writer_config_default() {
    let config = WriterConfig::default();

    assert_eq!(config.buffer_size, 1024);
    assert_eq!(config.write_timeout, Duration::from_secs(30));
    assert!(config.enable_compression);
    assert_eq!(config.max_frame_size, 1024 * 1024);
    assert_eq!(
        config.backpressure_strategy,
        BackpressureStrategy::DropLowPriority
    );
}

#[test]
fn test_writer_config_custom() {
    let config = WriterConfig {
        buffer_size: 2048,
        write_timeout: Duration::from_secs(60),
        enable_compression: false,
        max_frame_size: 512 * 1024,
        backpressure_strategy: BackpressureStrategy::Block,
    };

    assert_eq!(config.buffer_size, 2048);
    assert_eq!(config.write_timeout, Duration::from_secs(60));
    assert!(!config.enable_compression);
    assert_eq!(config.max_frame_size, 512 * 1024);
}

#[test]
fn test_writer_config_clone() {
    let config = WriterConfig::default();
    let cloned = config.clone();

    assert_eq!(cloned.buffer_size, config.buffer_size);
    assert_eq!(cloned.write_timeout, config.write_timeout);
}

#[test]
fn test_writer_config_debug() {
    let config = WriterConfig::default();
    let debug_str = format!("{:?}", config);
    assert!(debug_str.contains("WriterConfig"));
}

// BackpressureStrategy tests

#[test]
fn test_backpressure_strategy_variants() {
    let strategies = vec![
        BackpressureStrategy::Block,
        BackpressureStrategy::DropLowPriority,
        BackpressureStrategy::DropOldest,
        BackpressureStrategy::Error,
    ];

    assert_eq!(strategies.len(), 4);
}

#[test]
fn test_backpressure_strategy_equality() {
    assert_eq!(BackpressureStrategy::Block, BackpressureStrategy::Block);
    assert_ne!(
        BackpressureStrategy::Block,
        BackpressureStrategy::DropLowPriority
    );
}

#[test]
fn test_backpressure_strategy_clone() {
    let strategy = BackpressureStrategy::DropOldest;
    let cloned = strategy.clone();
    assert_eq!(strategy, cloned);
}

#[test]
fn test_backpressure_strategy_debug() {
    let strategy = BackpressureStrategy::Error;
    let debug_str = format!("{:?}", strategy);
    assert!(debug_str.contains("Error"));
}

// WriterMetrics tests

#[test]
fn test_writer_metrics_default() {
    let metrics = WriterMetrics::default();

    assert_eq!(metrics.frames_written, 0);
    assert_eq!(metrics.bytes_written, 0);
    assert_eq!(metrics.frames_dropped, 0);
    assert_eq!(metrics.buffer_size, 0);
    assert_eq!(metrics.avg_write_latency, Duration::ZERO);
    assert_eq!(metrics.error_count, 0);
}

#[test]
fn test_writer_metrics_with_data() {
    let metrics = WriterMetrics {
        frames_written: 1000,
        bytes_written: 1024 * 1024,
        frames_dropped: 5,
        buffer_size: 128,
        avg_write_latency: Duration::from_micros(500),
        error_count: 2,
    };

    assert_eq!(metrics.frames_written, 1000);
    assert_eq!(metrics.bytes_written, 1024 * 1024);
    assert_eq!(metrics.frames_dropped, 5);
    assert_eq!(metrics.error_count, 2);
}

#[test]
fn test_writer_metrics_equality() {
    let metrics1 = WriterMetrics::default();
    let metrics2 = WriterMetrics::default();

    assert_eq!(metrics1, metrics2);
}

#[test]
fn test_writer_metrics_clone() {
    let metrics = WriterMetrics {
        frames_written: 100,
        bytes_written: 1000,
        frames_dropped: 0,
        buffer_size: 50,
        avg_write_latency: Duration::from_millis(10),
        error_count: 1,
    };

    let cloned = metrics.clone();
    assert_eq!(cloned.frames_written, 100);
    assert_eq!(cloned.error_count, 1);
}

#[test]
fn test_writer_metrics_debug() {
    let metrics = WriterMetrics::default();
    let debug_str = format!("{:?}", metrics);
    assert!(debug_str.contains("WriterMetrics"));
}

// ConnectionState tests

#[test]
fn test_connection_state_variants() {
    let states = vec![
        ConnectionState::Active,
        ConnectionState::Unavailable,
        ConnectionState::Closing,
        ConnectionState::Closed,
        ConnectionState::Error("test error".to_string()),
    ];

    assert_eq!(states.len(), 5);
}

#[test]
fn test_connection_state_equality() {
    assert_eq!(ConnectionState::Active, ConnectionState::Active);
    assert_ne!(ConnectionState::Active, ConnectionState::Closed);

    assert_eq!(
        ConnectionState::Error("test".to_string()),
        ConnectionState::Error("test".to_string())
    );
}

#[test]
fn test_connection_state_clone() {
    let state = ConnectionState::Error("connection failed".to_string());
    let cloned = state.clone();

    assert_eq!(state, cloned);
}

#[test]
fn test_connection_state_debug() {
    let state = ConnectionState::Active;
    let debug_str = format!("{:?}", state);
    assert!(debug_str.contains("Active"));

    let error_state = ConnectionState::Error("test".to_string());
    let error_debug = format!("{:?}", error_state);
    assert!(error_debug.contains("Error"));
    assert!(error_debug.contains("test"));
}

// ConnectionMetrics tests

#[test]
fn test_connection_metrics_default() {
    let metrics = ConnectionMetrics::default();

    assert_eq!(metrics.rtt, Duration::from_millis(50));
    assert_eq!(metrics.bandwidth, 1_000_000);
    assert_eq!(metrics.uptime, Duration::ZERO);
    assert_eq!(metrics.reconnect_count, 0);
    assert!(metrics.last_error.is_none());
}

#[test]
fn test_connection_metrics_with_data() {
    let metrics = ConnectionMetrics {
        rtt: Duration::from_millis(100),
        bandwidth: 10_000_000,
        uptime: Duration::from_secs(3600),
        reconnect_count: 3,
        last_error: Some("timeout".to_string()),
    };

    assert_eq!(metrics.rtt, Duration::from_millis(100));
    assert_eq!(metrics.bandwidth, 10_000_000);
    assert_eq!(metrics.uptime, Duration::from_secs(3600));
    assert_eq!(metrics.reconnect_count, 3);
    assert_eq!(metrics.last_error.unwrap(), "timeout");
}

#[test]
fn test_connection_metrics_equality() {
    let metrics1 = ConnectionMetrics::default();
    let metrics2 = ConnectionMetrics::default();

    assert_eq!(metrics1, metrics2);
}

#[test]
fn test_connection_metrics_clone() {
    let metrics = ConnectionMetrics {
        rtt: Duration::from_millis(25),
        bandwidth: 5_000_000,
        uptime: Duration::from_secs(1800),
        reconnect_count: 1,
        last_error: None,
    };

    let cloned = metrics.clone();
    assert_eq!(cloned.rtt, Duration::from_millis(25));
    assert_eq!(cloned.bandwidth, 5_000_000);
}

#[test]
fn test_connection_metrics_debug() {
    let metrics = ConnectionMetrics::default();
    let debug_str = format!("{:?}", metrics);
    assert!(debug_str.contains("ConnectionMetrics"));
}

// CacheStatistics tests

#[test]
fn test_cache_statistics_default() {
    let stats = CacheStatistics::default();

    assert_eq!(stats.hit_rate, 0.0);
    assert_eq!(stats.miss_rate, 0.0);
    assert_eq!(stats.total_keys, 0);
    assert_eq!(stats.memory_usage_bytes, 0);
    assert_eq!(stats.eviction_count, 0);
}

#[test]
fn test_cache_statistics_with_data() {
    let stats = CacheStatistics {
        hit_rate: 0.85,
        miss_rate: 0.15,
        total_keys: 1000,
        memory_usage_bytes: 1024 * 1024,
        eviction_count: 50,
    };

    assert_eq!(stats.hit_rate, 0.85);
    assert_eq!(stats.miss_rate, 0.15);
    assert_eq!(stats.total_keys, 1000);
    assert_eq!(stats.memory_usage_bytes, 1024 * 1024);
    assert_eq!(stats.eviction_count, 50);
}

#[test]
fn test_cache_statistics_clone() {
    let stats = CacheStatistics {
        hit_rate: 0.9,
        miss_rate: 0.1,
        total_keys: 500,
        memory_usage_bytes: 512 * 1024,
        eviction_count: 10,
    };

    let cloned = stats.clone();
    assert_eq!(cloned.hit_rate, 0.9);
    assert_eq!(cloned.total_keys, 500);
}

#[test]
fn test_cache_statistics_debug() {
    let stats = CacheStatistics::default();
    let debug_str = format!("{:?}", stats);
    assert!(debug_str.contains("CacheStatistics"));
}

// SessionQueryResult tests

#[test]
fn test_session_query_result_empty() {
    let result = SessionQueryResult {
        sessions: vec![],
        total_count: 0,
        has_more: false,
        query_duration_ms: 10,
    };

    assert_eq!(result.sessions.len(), 0);
    assert_eq!(result.total_count, 0);
    assert!(!result.has_more);
}

#[test]
fn test_session_query_result_clone() {
    let result = SessionQueryResult {
        sessions: vec![],
        total_count: 100,
        has_more: true,
        query_duration_ms: 50,
    };

    let cloned = result.clone();
    assert_eq!(cloned.total_count, 100);
    assert!(cloned.has_more);
}

#[test]
fn test_session_query_result_debug() {
    let result = SessionQueryResult {
        sessions: vec![],
        total_count: 0,
        has_more: false,
        query_duration_ms: 10,
    };

    let debug_str = format!("{:?}", result);
    assert!(debug_str.contains("SessionQueryResult"));
}

// StreamStatistics tests

#[test]
fn test_stream_statistics_creation() {
    let now = chrono::Utc::now();

    let stats = StreamStatistics {
        total_frames: 100,
        total_bytes: 1024 * 1024,
        priority_distribution: pjson_rs::domain::events::PriorityDistribution::default(),
        avg_frame_size: 10240.0,
        creation_time: now,
        completion_time: Some(now + chrono::Duration::seconds(60)),
        processing_duration: Some(Duration::from_secs(60)),
    };

    assert_eq!(stats.total_frames, 100);
    assert_eq!(stats.total_bytes, 1024 * 1024);
    assert_eq!(stats.avg_frame_size, 10240.0);
    assert!(stats.completion_time.is_some());
}

#[test]
fn test_stream_statistics_clone() {
    let now = chrono::Utc::now();

    let stats = StreamStatistics {
        total_frames: 50,
        total_bytes: 512 * 1024,
        priority_distribution: pjson_rs::domain::events::PriorityDistribution::default(),
        avg_frame_size: 10240.0,
        creation_time: now,
        completion_time: None,
        processing_duration: None,
    };

    let cloned = stats.clone();
    assert_eq!(cloned.total_frames, 50);
    assert!(cloned.completion_time.is_none());
}

#[test]
fn test_stream_statistics_debug() {
    let now = chrono::Utc::now();

    let stats = StreamStatistics {
        total_frames: 10,
        total_bytes: 1024,
        priority_distribution: pjson_rs::domain::events::PriorityDistribution::default(),
        avg_frame_size: 102.4,
        creation_time: now,
        completion_time: None,
        processing_duration: None,
    };

    let debug_str = format!("{:?}", stats);
    assert!(debug_str.contains("StreamStatistics"));
}
