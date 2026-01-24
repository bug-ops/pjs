//! Tests for HTTP request/response DTO serialization and deserialization
//!
//! Validates JSON format, field mapping, and type conversions

#![cfg(feature = "http-server")]

use pjson_rs::domain::aggregates::stream_session::SessionHealth;
use pjson_rs::infrastructure::http::axum_adapter::{
    CreateSessionRequest, CreateSessionResponse, PaginationParams, SessionHealthResponse,
    StartStreamRequest,
};
use serde_json::json;

// ===== CreateSessionRequest Tests =====

#[test]
fn test_create_session_request_deserialize_full() {
    let json = json!({
        "max_concurrent_streams": 5,
        "timeout_seconds": 1800,
        "client_info": "test-client"
    });

    let request: CreateSessionRequest = serde_json::from_value(json).unwrap();
    assert_eq!(request.max_concurrent_streams, Some(5));
    assert_eq!(request.timeout_seconds, Some(1800));
    assert_eq!(request.client_info, Some("test-client".to_string()));
}

#[test]
fn test_create_session_request_deserialize_minimal() {
    let json = json!({});

    let request: CreateSessionRequest = serde_json::from_value(json).unwrap();
    assert_eq!(request.max_concurrent_streams, None);
    assert_eq!(request.timeout_seconds, None);
    assert_eq!(request.client_info, None);
}

#[test]
fn test_create_session_request_deserialize_partial() {
    let json = json!({
        "max_concurrent_streams": 10
    });

    let request: CreateSessionRequest = serde_json::from_value(json).unwrap();
    assert_eq!(request.max_concurrent_streams, Some(10));
    assert_eq!(request.timeout_seconds, None);
    assert_eq!(request.client_info, None);
}

#[test]
fn test_create_session_request_invalid_type() {
    let json = json!({
        "max_concurrent_streams": "not-a-number"
    });

    let result: Result<CreateSessionRequest, _> = serde_json::from_value(json);
    assert!(result.is_err());
}

// ===== CreateSessionResponse Tests =====

#[test]
fn test_create_session_response_serialize() {
    let response = CreateSessionResponse {
        session_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        expires_at: chrono::Utc::now(),
    };

    let json = serde_json::to_value(&response).unwrap();
    assert!(json.get("session_id").is_some());
    assert!(json.get("expires_at").is_some());
}

#[test]
fn test_create_session_response_session_id_format() {
    let response = CreateSessionResponse {
        session_id: "test-session-id".to_string(),
        expires_at: chrono::Utc::now(),
    };

    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(
        json.get("session_id").and_then(|v| v.as_str()),
        Some("test-session-id")
    );
}

// ===== StartStreamRequest Tests =====

#[test]
fn test_start_stream_request_deserialize() {
    let json = json!({
        "data": {"test": "value"},
        "priority_threshold": 5,
        "max_frames": 100
    });

    let request: StartStreamRequest = serde_json::from_value(json).unwrap();
    assert!(request.data.is_object());
    assert_eq!(request.priority_threshold, Some(5));
    assert_eq!(request.max_frames, Some(100));
}

#[test]
fn test_start_stream_request_minimal() {
    let json = json!({
        "data": {"test": "value"}
    });

    let request: StartStreamRequest = serde_json::from_value(json).unwrap();
    assert!(request.data.is_object());
    assert_eq!(request.priority_threshold, None);
    assert_eq!(request.max_frames, None);
}

#[test]
fn test_start_stream_request_complex_data() {
    let json = json!({
        "data": {
            "nested": {
                "array": [1, 2, 3],
                "string": "value"
            }
        }
    });

    let request: StartStreamRequest = serde_json::from_value(json).unwrap();
    let nested = request.data.get("nested").unwrap();
    assert!(nested.get("array").is_some());
    assert!(nested.get("string").is_some());
}

#[test]
fn test_start_stream_request_missing_data() {
    let json = json!({
        "priority_threshold": 5
    });

    let result: Result<StartStreamRequest, _> = serde_json::from_value(json);
    assert!(result.is_err());
}

// ===== SessionHealthResponse Tests =====

#[test]
fn test_session_health_response_from_domain() {
    let health = SessionHealth {
        is_healthy: true,
        active_streams: 5,
        failed_streams: 2,
        is_expired: false,
        uptime_seconds: 3600,
    };

    let response: SessionHealthResponse = health.into();
    assert_eq!(response.is_healthy, true);
    assert_eq!(response.active_streams, 5);
    assert_eq!(response.failed_streams, 2);
    assert_eq!(response.is_expired, false);
    assert_eq!(response.uptime_seconds, 3600);
}

#[test]
fn test_session_health_response_serialize() {
    let response = SessionHealthResponse {
        is_healthy: true,
        active_streams: 3,
        failed_streams: 1,
        is_expired: false,
        uptime_seconds: 7200,
    };

    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json.get("is_healthy"), Some(&json!(true)));
    assert_eq!(json.get("active_streams"), Some(&json!(3)));
    assert_eq!(json.get("failed_streams"), Some(&json!(1)));
    assert_eq!(json.get("is_expired"), Some(&json!(false)));
    assert_eq!(json.get("uptime_seconds"), Some(&json!(7200)));
}

#[test]
fn test_session_health_response_expired_session() {
    let health = SessionHealth {
        is_healthy: false,
        active_streams: 0,
        failed_streams: 0,
        is_expired: true,
        uptime_seconds: 0,
    };

    let response: SessionHealthResponse = health.into();
    assert_eq!(response.is_healthy, false);
    assert_eq!(response.is_expired, true);
}

// ===== PaginationParams Tests =====

#[test]
fn test_pagination_params_deserialize_full() {
    let json = json!({
        "limit": 10,
        "offset": 20
    });

    let params: PaginationParams = serde_json::from_value(json).unwrap();
    assert_eq!(params.limit, Some(10));
    assert_eq!(params.offset, Some(20));
}

#[test]
fn test_pagination_params_deserialize_empty() {
    let json = json!({});

    let params: PaginationParams = serde_json::from_value(json).unwrap();
    assert_eq!(params.limit, None);
    assert_eq!(params.offset, None);
}

#[test]
fn test_pagination_params_limit_only() {
    let json = json!({
        "limit": 50
    });

    let params: PaginationParams = serde_json::from_value(json).unwrap();
    assert_eq!(params.limit, Some(50));
    assert_eq!(params.offset, None);
}

#[test]
fn test_pagination_params_offset_only() {
    let json = json!({
        "offset": 100
    });

    let params: PaginationParams = serde_json::from_value(json).unwrap();
    assert_eq!(params.limit, None);
    assert_eq!(params.offset, Some(100));
}

// ===== Edge Cases and Validation =====

#[test]
fn test_create_session_request_zero_values() {
    let json = json!({
        "max_concurrent_streams": 0,
        "timeout_seconds": 0
    });

    let request: CreateSessionRequest = serde_json::from_value(json).unwrap();
    assert_eq!(request.max_concurrent_streams, Some(0));
    assert_eq!(request.timeout_seconds, Some(0));
}

#[test]
fn test_start_stream_request_zero_priority() {
    let json = json!({
        "data": {},
        "priority_threshold": 0
    });

    let request: StartStreamRequest = serde_json::from_value(json).unwrap();
    assert_eq!(request.priority_threshold, Some(0));
}

#[test]
fn test_start_stream_request_max_priority() {
    let json = json!({
        "data": {},
        "priority_threshold": 255
    });

    let request: StartStreamRequest = serde_json::from_value(json).unwrap();
    assert_eq!(request.priority_threshold, Some(255));
}

#[test]
fn test_session_health_response_large_uptime() {
    let health = SessionHealth {
        is_healthy: true,
        active_streams: 1000,
        failed_streams: 500,
        is_expired: false,
        uptime_seconds: 86400 * 365,
    };

    let response: SessionHealthResponse = health.into();
    assert_eq!(response.uptime_seconds, 86400 * 365);
}
