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

#![cfg(feature = "websocket-server")]

use pjson_rs::{
    Error as PjsError,
    domain::{Priority, SessionId},
    infrastructure::byte_bounded_channel,
    infrastructure::websocket::{
        AdaptiveStreamController, AxumWebSocketTransport, ConnectionId, StreamOptions, WsMessage,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

async fn create(
    controller: &AdaptiveStreamController,
    owner: ConnectionId,
    data: Value,
) -> SessionId {
    let session_id = SessionId::new();
    controller
        .create_session(session_id, owner, data, StreamOptions::default())
        .await
        .unwrap();
    session_id
}

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

    let session_id = create(&controller, ConnectionId::new(), data).await;

    assert!(!session_id.as_uuid().is_nil());
}

// ============================================================================
// AxumWebSocketTransport Message Handling Tests
// ============================================================================

#[tokio::test]
async fn test_websocket_transport_handle_message_stream_init() {
    let transport = AxumWebSocketTransport::new();
    let connection = ConnectionId::new();

    let message = WsMessage::StreamInit {
        session_id: SessionId::new(),
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

    // First create a session owned by the acknowledging connection
    let connection = ConnectionId::new();
    let session_id = create(&controller, connection, json!({"test": "data"})).await;

    // Send frame acknowledgment
    let message = WsMessage::FrameAck {
        session_id,
        frame_id: 0,
        processing_time_ms: 50,
    };

    let result = transport.handle_message(connection, message).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_websocket_transport_handle_message_ping() {
    let transport = AxumWebSocketTransport::new();
    let connection = ConnectionId::new();

    let message = WsMessage::Ping {
        timestamp: 1234567890,
    };

    let result = transport.handle_message(connection, message).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_websocket_transport_handle_message_error() {
    let transport = AxumWebSocketTransport::new();
    let connection = ConnectionId::new();

    let message = WsMessage::Error {
        session_id: Some(SessionId::new()),
        error: "Test error message".to_string(),
        code: 500,
    };

    let result = transport.handle_message(connection, message).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_websocket_transport_handle_message_pong() {
    let transport = AxumWebSocketTransport::new();
    let connection = ConnectionId::new();

    let message = WsMessage::Pong {
        timestamp: 1234567890,
    };

    let result = transport.handle_message(connection, message).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_remove_unknown_session_is_a_no_op() {
    let transport = AxumWebSocketTransport::new();

    let removed = transport
        .controller()
        .remove_session(ConnectionId::new(), SessionId::new())
        .await;

    assert!(!removed);
}

#[tokio::test]
async fn test_remove_session_removes_session_from_controller() {
    let transport = AxumWebSocketTransport::new();
    let controller = transport.controller();

    let owner = ConnectionId::new();
    let session_id = create(&controller, owner, json!({"k": "v"})).await;

    assert!(controller.remove_session(owner, session_id).await);

    // Session must be gone — subsequent operations must return InvalidSession.
    let err = controller
        .handle_frame_ack(owner, session_id, 0, 10)
        .await
        .unwrap_err();
    assert!(
        matches!(err, PjsError::InvalidSession(_)),
        "expected InvalidSession, got {err:?}"
    );
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

    let session_id = create(&controller, ConnectionId::new(), data).await;

    assert!(!session_id.as_uuid().is_nil());
}

#[tokio::test]
async fn test_controller_start_streaming() {
    let controller = AdaptiveStreamController::new();

    let data = json!({
        "test": "data",
        "values": [1, 2, 3]
    });

    let owner = ConnectionId::new();
    let session_id = create(&controller, owner, data).await;

    let result = controller.start_streaming(owner, session_id).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_controller_start_streaming_invalid_session() {
    let controller = AdaptiveStreamController::new();

    let result = controller
        .start_streaming(ConnectionId::new(), SessionId::new())
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_controller_handle_frame_ack() {
    let controller = AdaptiveStreamController::new();

    let owner = ConnectionId::new();
    let session_id = create(&controller, owner, json!({"test": "data"})).await;

    let result = controller.handle_frame_ack(owner, session_id, 0, 50).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_controller_handle_frame_ack_multiple() {
    let controller = AdaptiveStreamController::new();

    let owner = ConnectionId::new();
    let session_id = create(&controller, owner, json!({"test": "data"})).await;

    // Acknowledge multiple frames
    controller
        .handle_frame_ack(owner, session_id, 0, 50)
        .await
        .unwrap();
    controller
        .handle_frame_ack(owner, session_id, 1, 60)
        .await
        .unwrap();
    controller
        .handle_frame_ack(owner, session_id, 2, 55)
        .await
        .unwrap();

    // All should succeed
}

#[tokio::test]
async fn test_controller_handle_frame_ack_invalid_session() {
    let controller = AdaptiveStreamController::new();

    let result = controller
        .handle_frame_ack(ConnectionId::new(), SessionId::new(), 0, 50)
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_controller_delivers_frames_to_the_attached_connection() {
    let controller = AdaptiveStreamController::new();
    let owner = ConnectionId::new();
    let (tx, mut rx) = byte_bounded_channel::<String>(1000, 16 * 1024 * 1024);
    controller.attach_connection(owner, tx);

    let session_id = create(&controller, owner, json!({"test": "data"})).await;
    controller.start_streaming(owner, session_id).await.unwrap();

    let envelope = tokio::time::timeout(tokio::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("a frame must be delivered")
        .expect("channel open");
    let message: WsMessage = serde_json::from_str(&envelope.into_inner()).unwrap();
    assert!(matches!(message, WsMessage::StreamFrame { .. }));
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
    priority_mapping.insert("critical".to_string(), Priority::CRITICAL);
    priority_mapping.insert("high".to_string(), Priority::HIGH);

    let options = StreamOptions {
        max_frame_size: 1024 * 1024,
        client_fps: Some(60),
        compression: false,
        priority_mapping: Some(priority_mapping.clone()),
        ..Default::default()
    };

    assert_eq!(options.max_frame_size, 1024 * 1024);
    assert_eq!(options.client_fps, Some(60));
    assert!(!options.compression);
    assert_eq!(
        options.priority_mapping.as_ref().unwrap().get("critical"),
        Some(&Priority::CRITICAL)
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
        session_id: SessionId::new(),
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
        session_id: SessionId::new(),
        frame_id: 5,
        priority: Priority::HIGH,
        payload: json!({"frame": "data"}),
        is_complete: false,
    };

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["type"], "StreamFrame");
    assert_eq!(json["data"]["frame_id"], 5);
    assert_eq!(json["data"]["priority"], 80);
}

#[test]
fn test_ws_message_frame_ack_serialization() {
    let message = WsMessage::FrameAck {
        session_id: SessionId::new(),
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
        session_id: SessionId::new(),
    };

    let json = serde_json::to_value(&message).unwrap();

    assert_eq!(json["type"], "StreamComplete");
    assert!(json["data"].get("checksum").is_none());
}

#[test]
fn test_ws_message_error_serialization() {
    let message = WsMessage::Error {
        session_id: Some(SessionId::new()),
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
    let id = SessionId::new();
    let json = json!({
        "type": "FrameAck",
        "data": {
            "session_id": id.as_uuid().to_string(),
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
            assert_eq!(session_id, id);
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

    let owner = ConnectionId::new();
    let session_id = create(&controller, owner, data).await;

    // 2. Simulate frame acknowledgments
    controller
        .handle_frame_ack(owner, session_id, 0, 30)
        .await
        .unwrap();

    controller
        .handle_frame_ack(owner, session_id, 1, 35)
        .await
        .unwrap();

    // 3. Close stream — session must be removed from the controller.
    assert!(controller.remove_session(owner, session_id).await);

    let err = controller
        .handle_frame_ack(owner, session_id, 0, 0)
        .await
        .unwrap_err();
    assert!(matches!(err, PjsError::InvalidSession(_)));
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

        let owner = ConnectionId::new();
        let session_id = create(&controller, owner, data).await;

        session_ids.push((owner, session_id));
    }

    assert_eq!(session_ids.len(), 5);

    // All sessions should be unique
    let unique_count = session_ids
        .iter()
        .map(|(_, id)| id)
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(unique_count, 5);

    // Start streaming for all sessions
    for (owner, session_id) in &session_ids {
        controller
            .start_streaming(*owner, *session_id)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn test_websocket_error_handling() {
    let transport = AxumWebSocketTransport::new();

    // Try to close non-existent stream
    assert!(
        !transport
            .controller()
            .remove_session(ConnectionId::new(), SessionId::new())
            .await
    );

    // Try to handle frame ack for non-existent session
    let controller = transport.controller();
    let result = controller
        .handle_frame_ack(ConnectionId::new(), SessionId::new(), 0, 50)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_websocket_send_frame() {
    let transport = AxumWebSocketTransport::new();
    let connection = ConnectionId::new();

    let message = WsMessage::StreamFrame {
        session_id: SessionId::new(),
        frame_id: 1,
        priority: Priority::HIGH,
        payload: json!({"test": "data"}),
        is_complete: false,
    };

    let result = transport.send_frame(connection, message).await;

    // Should succeed (or at least not panic)
    assert!(result.is_ok());
}
