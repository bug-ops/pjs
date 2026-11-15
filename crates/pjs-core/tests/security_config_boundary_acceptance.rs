//! Boundary acceptance tests for the cross-field rules of `SecurityConfig::validate` (#614):
//! equal values are valid, only a strictly larger value is rejected.

use pjson_rs::SecurityConfig;

#[test]
fn test_validate_buffer_equal_to_total_memory_accepted() {
    let mut config = SecurityConfig::default();
    config.buffers.max_buffer_size = config.buffers.max_total_memory;
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_websocket_frame_equal_to_json_input_accepted() {
    let mut config = SecurityConfig::default();
    config.network.max_websocket_frame_size = config.json.max_input_size;
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_session_data_equal_to_total_memory_accepted() {
    let mut config = SecurityConfig::default();
    config.sessions.max_session_data_size = config.buffers.max_total_memory;
    assert!(config.validate().is_ok());
}
