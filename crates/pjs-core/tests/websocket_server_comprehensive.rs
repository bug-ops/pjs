// Comprehensive tests for WebSocket server module
//
// This test file covers the infrastructure/websocket/server.rs module with focus on:
// - AxumWebSocketTransport creation and initialization
// - WebSocket message handling (StreamInit, FrameAck, Ping, Error)
// - Session management and controller integration
// - Connection lifecycle management
// - WebSocket router creation
// - Async streaming functionality
//
// Coverage target: 60%+ for Infrastructure Layer

#![cfg(feature = "http-server")]

use pjson_rs::infrastructure::websocket::{
    AdaptiveStreamController, AxumWebSocketTransport, StreamOptions, WebSocketTransport, WsMessage,
};
use serde_json::json;
use std::sync::Arc;

// ============================================================================
// AxumWebSocketTransport Tests
// ============================================================================

#[tokio::test]
async fn test_axum_websocket_transport_creation() {
    let transport = AxumWebSocketTransport::new();

    assert!(Arc::strong_count(&transport.controller()) >= 1);
}

#[tokio::test]
async fn test_axum_websocket_transport_default() {
    let transport = AxumWebSocketTransport::default();

    assert!(Arc::strong_count(&transport.controller()) >= 1);
}

#[tokio::test]
async fn test_axum_websocket_transport_controller_access() {
    let transport = AxumWebSocketTransport::new();
    let controller = transport.controller();

    // Controller should be functional
    let data = json!({
        "test": "data",
        "value": 123
    });

    let session_id = controller
        .create_session(data, StreamOptions::default())
        .await
        .unwrap();

    assert!(!session_id.is_empty());
}

// ============================================================================
// WebSocketTransport Implementation Tests
// ============================================================================

#[tokio::test]
async fn test_websocket_transport_start_stream() {
    let transport = AxumWebSocketTransport::new();
    let connection = Arc::new("test-connection-1".to_string());

    let data = json!({
        "critical": {"id": 1, "status": "active"},
        "metadata": {"created": "2024-01-15T12:00:00Z"}
    });

    let result = transport
        .start_stream(connection, data, StreamOptions::default())
        .await;

    assert!(result.is_ok());
    let session_id = result.unwrap();
    assert!(!session_id.is_empty());
}

#[tokio::test]
async fn test_websocket_transport_start_stream_with_custom_options() {
    let transport = AxumWebSocketTransport::new();
    let connection = Arc::new("test-connection-2".to_string());

    let data = json!({
        "data": [1, 2, 3, 4, 5]
    });

    let options = StreamOptions {
        max_frame_size: 1024,
        client_fps: Some(30),
        compression: false,
        priority_mapping: None,
    };

    let result = transport.start_stream(connection, data, options).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_websocket_transport_handle_message_stream_init() {
    let transport = AxumWebSocketTransport::new();
    let connection = Arc::new("test-connection-3".to_string());

    let message = WsMessage::StreamInit {
        session_id: "test-session-init".to_string(),
        data: json!({"test": "value"}),
        options: StreamOptions::default(),
    };

    let result = transport.handle_message(connection, message).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_websocket_transport_handle_message_frame_ack() {
    let transport = AxumWebSocketTransport::new();
    let controller = transport.controller();

    // First create a session
    let data = json!({"test": "data"});
    let session_id = controller
        .create_session(data, StreamOptions::default())
        .await
        .unwrap();

    let connection = Arc::new("test-connection-4".to_string());

    // Send frame acknowledgment
    let message = WsMessage::FrameAck {
        session_id: session_id.clone(),
        frame_id: 0,
        processing_time_ms: 50,
    };

    let result = transport.handle_message(connection, message).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_websocket_transport_handle_message_ping() {
    let transport = AxumWebSocketTransport::new();
    let connection = Arc::new("test-connection-5".to_string());

    let message = WsMessage::Ping {
        timestamp: 1234567890,
    };

    let result = transport.handle_message(connection, message).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_websocket_transport_handle_message_error() {
    let transport = AxumWebSocketTransport::new();
    let connection = Arc::new("test-connection-6".to_string());

    let message = WsMessage::Error {
        session_id: Some("test-session".to_string()),
        error: "Test error message".to_string(),
        code: 500,
    };

    let result = transport.handle_message(connection, message).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_websocket_transport_handle_message_pong() {
    let transport = AxumWebSocketTransport::new();
    let connection = Arc::new("test-connection-7".to_string());

    let message = WsMessage::Pong {
        timestamp: 1234567890,
    };

    let result = transport.handle_message(connection, message).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_websocket_transport_close_stream() {
    let transport = AxumWebSocketTransport::new();

    let result = transport.close_stream("test-session-close").await;

    assert!(result.is_ok());
}

// ============================================================================
// AdaptiveStreamController Integration Tests
// ============================================================================

#[tokio::test]
async fn test_controller_create_session() {
    let controller = AdaptiveStreamController::new();

    let data = json!({
        "critical": {"id": 1, "status": "active"},
        "details": {"name": "test", "description": "test data"}
    });

    let session_id = controller
        .create_session(data, StreamOptions::default())
        .await
        .unwrap();

    assert!(!session_id.is_empty());
}

#[tokio::test]
async fn test_controller_start_streaming() {
    let controller = AdaptiveStreamController::new();

    let data = json!({
        "test": "data",
        "values": [1, 2, 3]
    });

    let session_id = controller
        .create_session(data, StreamOptions::default())
        .await
        .unwrap();

    let result = controller.start_streaming(&session_id).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_controller_start_streaming_invalid_session() {
    let controller = AdaptiveStreamController::new();

    let result = controller.start_streaming("invalid-session-id").await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_controller_handle_frame_ack() {
    let controller = AdaptiveStreamController::new();

    let data = json!({"test": "data"});
    let session_id = controller
        .create_session(data, StreamOptions::default())
        .await
        .unwrap();

    let result = controller.handle_frame_ack(&session_id, 0, 50).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_controller_handle_frame_ack_multiple() {
    let controller = AdaptiveStreamController::new();

    let data = json!({"test": "data"});
    let session_id = controller
        .create_session(data, StreamOptions::default())
        .await
        .unwrap();

    // Acknowledge multiple frames
    controller
        .handle_frame_ack(&session_id, 0, 50)
        .await
        .unwrap();
    controller
        .handle_frame_ack(&session_id, 1, 60)
        .await
        .unwrap();
    controller
        .handle_frame_ack(&session_id, 2, 55)
        .await
        .unwrap();

    // All should succeed
}

#[tokio::test]
async fn test_controller_handle_frame_ack_invalid_session() {
    let controller = AdaptiveStreamController::new();

    let result = controller.handle_frame_ack("invalid-session", 0, 50).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_controller_subscribe_frames() {
    let controller = AdaptiveStreamController::new();

    let mut receiver = controller.subscribe_frames();

    // Create and start a session
    let data = json!({"test": "data"});
    let session_id = controller
        .create_session(data, StreamOptions::default())
        .await
        .unwrap();

    controller.start_streaming(&session_id).await.unwrap();

    // Give some time for frames to be sent
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Try to receive a frame
    let _result = receiver.try_recv();
    // Should receive something or get Lagged/Closed error (which is okay in tests)
    // We're just testing that the subscription mechanism works
}

// ============================================================================
// StreamOptions Tests
// ============================================================================

#[test]
fn test_stream_options_default() {
    let options = StreamOptions::default();

    assert_eq!(options.max_frame_size, 64 * 1024);
    assert_eq!(options.client_fps, None);
    assert!(options.compression);
    assert!(options.priority_mapping.is_none());
}

#[test]
fn test_stream_options_custom() {
    use std::collections::HashMap;

    let mut priority_mapping = HashMap::new();
    priority_mapping.insert("critical".to_string(), 255);
    priority_mapping.insert("high".to_string(), 200);

    let options = StreamOptions {
        max_frame_size: 1024 * 1024,
        client_fps: Some(60),
        compression: false,
        priority_mapping: Some(priority_mapping.clone()),
    };

    assert_eq!(options.max_frame_size, 1024 * 1024);
    assert_eq!(options.client_fps, Some(60));
    assert!(!options.compression);
    assert_eq!(
        options.priority_mapping.as_ref().unwrap().get("critical"),
        Some(&255)
    );
}

#[test]
fn test_stream_options_serialization() {
    let options = StreamOptions::default();

    let json = serde_json::to_value(&options).unwrap();

    assert!(json.is_object());
    assert!(json.get("max_frame_size").is_some());
    assert!(json.get("compression").is_some());
}

#[test]
fn test_stream_options_deserialization() {
    let json = json!({
        "max_frame_size": 2048,
        "client_fps": 30,
        "compression": false,
        "priority_mapping": null
    });

    let options: StreamOptions = serde_json::from_value(json).unwrap();

    assert_eq!(options.max_frame_size, 2048);
    assert_eq!(options.client_fps, Some(30));
    assert!(!options.compression);
}

// ============================================================================
// WsMessage Tests
// ============================================================================

#[test]
fn test_ws_message_stream_init_serialization() {
    let message = WsMessage::StreamInit {
        session_id: "test-123".to_string(),
        data: json!({"key": "value"}),
        options: StreamOptions::default(),
    };

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["type"], "StreamInit");
    assert!(json["data"].is_object());
}

#[test]
fn test_ws_message_stream_frame_serialization() {
    let message = WsMessage::StreamFrame {
        session_id: "test-456".to_string(),
        frame_id: 5,
        priority: 200,
        payload: json!({"frame": "data"}),
        is_complete: false,
    };

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["type"], "StreamFrame");
    assert_eq!(json["data"]["frame_id"], 5);
    assert_eq!(json["data"]["priority"], 200);
}

#[test]
fn test_ws_message_frame_ack_serialization() {
    let message = WsMessage::FrameAck {
        session_id: "test-789".to_string(),
        frame_id: 3,
        processing_time_ms: 125,
    };

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["type"], "FrameAck");
    assert_eq!(json["data"]["frame_id"], 3);
    assert_eq!(json["data"]["processing_time_ms"], 125);
}

#[test]
fn test_ws_message_stream_complete_serialization() {
    let message = WsMessage::StreamComplete {
        session_id: "test-complete".to_string(),
        checksum: "sha256:abcd1234".to_string(),
    };

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["type"], "StreamComplete");
    assert_eq!(json["data"]["checksum"], "sha256:abcd1234");
}

#[test]
fn test_ws_message_error_serialization() {
    let message = WsMessage::Error {
        session_id: Some("error-session".to_string()),
        error: "Connection lost".to_string(),
        code: 1006,
    };

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["type"], "Error");
    assert_eq!(json["data"]["error"], "Connection lost");
    assert_eq!(json["data"]["code"], 1006);
}

#[test]
fn test_ws_message_ping_serialization() {
    let message = WsMessage::Ping {
        timestamp: 1234567890,
    };

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["type"], "Ping");
    assert_eq!(json["data"]["timestamp"], 1234567890);
}

#[test]
fn test_ws_message_pong_serialization() {
    let message = WsMessage::Pong {
        timestamp: 987654321,
    };

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["type"], "Pong");
    assert_eq!(json["data"]["timestamp"], 987654321_u64);
}

#[test]
fn test_ws_message_deserialization() {
    let json = json!({
        "type": "FrameAck",
        "data": {
            "session_id": "test",
            "frame_id": 1,
            "processing_time_ms": 50
        }
    });

    let message: WsMessage = serde_json::from_value(json).unwrap();

    match message {
        WsMessage::FrameAck {
            session_id,
            frame_id,
            processing_time_ms,
        } => {
            assert_eq!(session_id, "test");
            assert_eq!(frame_id, 1);
            assert_eq!(processing_time_ms, 50);
        }
        _ => panic!("Expected FrameAck message"),
    }
}

// ============================================================================
// Router Creation Tests
// ============================================================================

#[cfg(feature = "http-server")]
#[test]
fn test_create_websocket_router() {
    use pjson_rs::infrastructure::websocket::create_websocket_router;

    let _router = create_websocket_router();

    // Router should be created successfully
    // Actual routing functionality would be tested in integration tests
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_full_websocket_lifecycle() {
    let transport = AxumWebSocketTransport::new();
    let controller = transport.controller();

    // 1. Create session
    let data = json!({
        "critical": {"id": 1, "status": "active"},
        "details": {"name": "test"}
    });

    let session_id = controller
        .create_session(data, StreamOptions::default())
        .await
        .unwrap();

    assert!(!session_id.is_empty());

    // 2. Start streaming
    controller.start_streaming(&session_id).await.unwrap();

    // 3. Simulate frame acknowledgments
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    controller
        .handle_frame_ack(&session_id, 0, 30)
        .await
        .unwrap();

    controller
        .handle_frame_ack(&session_id, 1, 35)
        .await
        .unwrap();

    // 4. Close stream
    transport.close_stream(&session_id).await.unwrap();
}

#[tokio::test]
async fn test_multiple_concurrent_sessions() {
    let transport = AxumWebSocketTransport::new();
    let controller = transport.controller();

    let mut session_ids = Vec::new();

    // Create multiple sessions
    for i in 0..5 {
        let data = json!({
            "session": i,
            "data": format!("test-{}", i)
        });

        let session_id = controller
            .create_session(data, StreamOptions::default())
            .await
            .unwrap();

        session_ids.push(session_id);
    }

    assert_eq!(session_ids.len(), 5);

    // All sessions should be unique
    let unique_count = session_ids
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(unique_count, 5);

    // Start streaming for all sessions
    for session_id in &session_ids {
        controller.start_streaming(session_id).await.unwrap();
    }
}

#[tokio::test]
async fn test_websocket_error_handling() {
    let transport = AxumWebSocketTransport::new();

    // Try to close non-existent stream
    let result = transport.close_stream("non-existent").await;
    assert!(result.is_ok()); // Should not fail, just log

    // Try to handle frame ack for non-existent session
    let controller = transport.controller();
    let result = controller.handle_frame_ack("non-existent", 0, 50).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_websocket_send_frame() {
    let transport = AxumWebSocketTransport::new();
    let connection = Arc::new("test-send-frame".to_string());

    let message = WsMessage::StreamFrame {
        session_id: "test".to_string(),
        frame_id: 1,
        priority: 200,
        payload: json!({"test": "data"}),
        is_complete: false,
    };

    let result = transport.send_frame(connection, message).await;

    // Should succeed (or at least not panic)
    assert!(result.is_ok());
}
