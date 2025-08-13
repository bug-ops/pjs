//! WebSocket security integration tests

use pjs_core::{
    infrastructure::websocket::{AdaptiveStreamController, SecureWebSocketHandler, WsMessage, StreamOptions},
    security::{RateLimitConfig, RateLimitError},
    config::SecurityConfig,
};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use tokio::time::sleep;

/// Test WebSocket rate limiting integration
#[tokio::test]
async fn test_websocket_rate_limiting_integration() {
    let rate_config = RateLimitConfig {
        max_requests_per_window: 2,
        window_duration: Duration::from_secs(1),
        max_connections_per_ip: 1,
        max_messages_per_second: 2,
        burst_allowance: 0,
        max_frame_size: 1024,
    };
    
    let security_handler = SecureWebSocketHandler::new(rate_config);
    let stream_controller = AdaptiveStreamController::new();
    let client_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
    
    // Test upgrade rate limiting
    assert!(security_handler.check_upgrade_request(client_ip).is_ok());
    assert!(security_handler.check_upgrade_request(client_ip).is_ok());
    
    // Third request should be rate limited
    let result = security_handler.check_upgrade_request(client_ip);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), RateLimitError::LimitExceeded { .. }));
    
    // Test connection limiting
    let guard = security_handler.create_connection_guard(client_ip).unwrap();
    
    // Second connection should fail
    let result = security_handler.create_connection_guard(client_ip);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), RateLimitError::ConnectionLimitExceeded { .. }));
    
    // Test streaming session with rate limiting
    let test_data = json!({
        "id": 1,
        "name": "Test User",
        "data": {
            "details": "Some large data payload for testing"
        }
    });
    
    let session_id = stream_controller
        .create_session(test_data, StreamOptions::default())
        .await
        .unwrap();
    
    // Associate rate limit guard with session
    stream_controller
        .set_rate_limit_guard(&session_id, guard)
        .await
        .unwrap();
    
    // Test message validation
    assert!(stream_controller.validate_message(&session_id, 512).await.is_ok());
    assert!(stream_controller.validate_message(&session_id, 512).await.is_ok());
    
    // Should be rate limited after burst
    let result = stream_controller.validate_message(&session_id, 512).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Rate limit violation"));
    
    // Test frame size limits
    let result = stream_controller.validate_message(&session_id, 2048).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Frame size"));
}

/// Test security configuration profiles
#[tokio::test]
async fn test_security_configuration_profiles() {
    let default_handler = SecureWebSocketHandler::with_default_security();
    let high_traffic_handler = SecureWebSocketHandler::with_high_traffic_security();
    let low_resource_handler = SecureWebSocketHandler::with_low_resource_security();
    
    let client_ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
    
    // All profiles should work for basic requests
    assert!(default_handler.check_upgrade_request(client_ip).is_ok());
    assert!(high_traffic_handler.check_upgrade_request(client_ip).is_ok());
    assert!(low_resource_handler.check_upgrade_request(client_ip).is_ok());
    
    // Test different connection limits
    let _guard1 = default_handler.create_connection_guard(client_ip).unwrap();
    let _guard2 = high_traffic_handler.create_connection_guard(client_ip).unwrap();
    let _guard3 = low_resource_handler.create_connection_guard(client_ip).unwrap();
    
    // Get stats from all handlers
    let default_stats = default_handler.get_security_stats();
    let high_traffic_stats = high_traffic_handler.get_security_stats();
    let low_resource_stats = low_resource_handler.get_security_stats();
    
    assert!(default_stats.total_clients > 0);
    assert!(high_traffic_stats.total_clients > 0);
    assert!(low_resource_stats.total_clients > 0);
}

/// Test rate limiting with bursts
#[tokio::test]
async fn test_burst_rate_limiting() {
    let rate_config = RateLimitConfig {
        max_messages_per_second: 1,
        burst_allowance: 3,
        max_frame_size: 1024,
        ..Default::default()
    };
    
    let security_handler = SecureWebSocketHandler::new(rate_config);
    let stream_controller = AdaptiveStreamController::new();
    let client_ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
    
    // Create session
    let test_data = json!({"test": "burst data"});
    let session_id = stream_controller
        .create_session(test_data, StreamOptions::default())
        .await
        .unwrap();
    
    let guard = security_handler.create_connection_guard(client_ip).unwrap();
    stream_controller
        .set_rate_limit_guard(&session_id, guard)
        .await
        .unwrap();
    
    // Should allow burst messages
    assert!(stream_controller.validate_message(&session_id, 100).await.is_ok());
    assert!(stream_controller.validate_message(&session_id, 100).await.is_ok());
    assert!(stream_controller.validate_message(&session_id, 100).await.is_ok());
    assert!(stream_controller.validate_message(&session_id, 100).await.is_ok()); // Burst allowance
    
    // Should be rate limited now
    let result = stream_controller.validate_message(&session_id, 100).await;
    assert!(result.is_err());
}

/// Test rate limiting cleanup and recovery
#[tokio::test]
async fn test_rate_limiting_recovery() {
    let rate_config = RateLimitConfig {
        max_requests_per_window: 1,
        window_duration: Duration::from_millis(100),
        max_connections_per_ip: 1,
        ..Default::default()
    };
    
    let security_handler = SecureWebSocketHandler::new(rate_config);
    let client_ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1));
    
    // First request should succeed
    assert!(security_handler.check_upgrade_request(client_ip).is_ok());
    
    // Second request should be rate limited
    assert!(security_handler.check_upgrade_request(client_ip).is_err());
    
    // Wait for window to reset
    sleep(Duration::from_millis(150)).await;
    
    // Should work again after window reset
    assert!(security_handler.check_upgrade_request(client_ip).is_ok());
    
    // Test cleanup
    security_handler.force_cleanup();
    
    // Should still work after cleanup
    assert!(security_handler.check_upgrade_request(client_ip).is_err()); // Still in window
}

/// Test frame size validation
#[tokio::test]
async fn test_frame_size_validation() {
    let rate_config = RateLimitConfig {
        max_frame_size: 512, // Small frame limit
        ..Default::default()
    };
    
    let security_handler = SecureWebSocketHandler::new(rate_config);
    let stream_controller = AdaptiveStreamController::new();
    let client_ip = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1));
    
    let test_data = json!({"frame": "size test"});
    let session_id = stream_controller
        .create_session(test_data, StreamOptions::default())
        .await
        .unwrap();
    
    let guard = security_handler.create_connection_guard(client_ip).unwrap();
    stream_controller
        .set_rate_limit_guard(&session_id, guard)
        .await
        .unwrap();
    
    // Small frame should work
    assert!(stream_controller.validate_message(&session_id, 256).await.is_ok());
    
    // Large frame should be rejected
    let result = stream_controller.validate_message(&session_id, 1024).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Frame size"));
}

/// Test concurrent rate limiting
#[tokio::test]
async fn test_concurrent_rate_limiting() {
    let rate_config = RateLimitConfig {
        max_connections_per_ip: 2,
        max_messages_per_second: 10,
        burst_allowance: 5,
        ..Default::default()
    };
    
    let security_handler = SecureWebSocketHandler::new(rate_config);
    let client_ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10));
    
    // Create multiple guards concurrently
    let guard1 = security_handler.create_connection_guard(client_ip).unwrap();
    let guard2 = security_handler.create_connection_guard(client_ip).unwrap();
    
    // Third connection should fail
    assert!(security_handler.create_connection_guard(client_ip).is_err());
    
    // Both guards should work for message validation
    assert!(security_handler.validate_message(&guard1, 100).is_ok());
    assert!(security_handler.validate_message(&guard2, 100).is_ok());
    
    // Stats should reflect multiple clients
    let stats = security_handler.get_security_stats();
    assert_eq!(stats.total_connections, 2);
    
    // Drop one guard
    drop(guard1);
    
    // Should be able to create new connection
    let _guard3 = security_handler.create_connection_guard(client_ip).unwrap();
}

/// Test security configuration integration
#[tokio::test]
async fn test_security_config_integration() {
    let security_config = SecurityConfig::high_throughput();
    
    // Rate limiting should use values from security config
    let rate_config = RateLimitConfig {
        max_requests_per_window: security_config.network.rate_limiting.max_requests_per_window,
        window_duration: Duration::from_secs(security_config.network.rate_limiting.window_duration_secs),
        max_connections_per_ip: security_config.network.rate_limiting.max_connections_per_ip,
        max_messages_per_second: security_config.network.rate_limiting.max_messages_per_second,
        burst_allowance: security_config.network.rate_limiting.burst_allowance,
        max_frame_size: security_config.network.max_websocket_frame_size,
    };
    
    let security_handler = SecureWebSocketHandler::new(rate_config);
    let client_ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10));
    
    // Should allow many requests for high throughput config
    for _ in 0..10 {
        assert!(security_handler.check_upgrade_request(client_ip).is_ok());
    }
    
    // Should allow many connections
    let mut guards = Vec::new();
    for _ in 0..5 {
        guards.push(security_handler.create_connection_guard(client_ip).unwrap());
    }
    
    // All guards should work
    for guard in &guards {
        assert!(security_handler.validate_message(guard, 1024).is_ok());
    }
}