//! Frame entity with streaming data

use crate::domain::{
    DomainError, DomainResult,
    value_objects::{JsonPath, Priority, StreamId},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
// TODO: Fix architecture violation - domain layer should not depend on serde_json::Value
// Create domain-specific value objects instead
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Frame types for different stages of streaming
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameType {
    /// Initial skeleton with structure
    Skeleton,
    /// Data patch update
    Patch,
    /// Stream completion signal
    Complete,
    /// Error notification
    Error,
}

/// Individual frame in a priority stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    stream_id: StreamId,
    frame_type: FrameType,
    priority: Priority,
    sequence: u64,
    timestamp: DateTime<Utc>,
    payload: JsonValue,
    metadata: HashMap<String, String>,
}

impl Frame {
    /// Create new skeleton frame
    pub fn skeleton(stream_id: StreamId, sequence: u64, skeleton_data: JsonValue) -> Self {
        Self {
            stream_id,
            frame_type: FrameType::Skeleton,
            priority: Priority::CRITICAL,
            sequence,
            timestamp: Utc::now(),
            payload: skeleton_data,
            metadata: HashMap::new(),
        }
    }

    /// Create new patch frame
    pub fn patch(
        stream_id: StreamId,
        sequence: u64,
        priority: Priority,
        patches: Vec<FramePatch>,
    ) -> DomainResult<Self> {
        if patches.is_empty() {
            return Err(DomainError::InvalidFrame(
                "Patch frame must contain at least one patch".to_string(),
            ));
        }

        let payload = serde_json::to_value(&PatchPayload { patches })
            .map_err(|e| DomainError::Logic(format!("JSON serialization error: {}", e)))?;

        Ok(Self {
            stream_id,
            frame_type: FrameType::Patch,
            priority,
            sequence,
            timestamp: Utc::now(),
            payload,
            metadata: HashMap::new(),
        })
    }

    /// Create completion frame
    pub fn complete(stream_id: StreamId, sequence: u64, checksum: Option<String>) -> Self {
        let payload = if let Some(checksum) = checksum {
            serde_json::json!({"checksum": checksum})
        } else {
            serde_json::json!({})
        };

        Self {
            stream_id,
            frame_type: FrameType::Complete,
            priority: Priority::CRITICAL,
            sequence,
            timestamp: Utc::now(),
            payload,
            metadata: HashMap::new(),
        }
    }

    /// Create error frame
    pub fn error(
        stream_id: StreamId,
        sequence: u64,
        error_message: String,
        error_code: Option<String>,
    ) -> Self {
        let payload = if let Some(code) = error_code {
            serde_json::json!({"message": error_message, "code": code})
        } else {
            serde_json::json!({"message": error_message})
        };

        Self {
            stream_id,
            frame_type: FrameType::Error,
            priority: Priority::CRITICAL,
            sequence,
            timestamp: Utc::now(),
            payload,
            metadata: HashMap::new(),
        }
    }

    /// Get stream ID
    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }

    /// Get frame type
    pub fn frame_type(&self) -> &FrameType {
        &self.frame_type
    }

    /// Get priority
    pub fn priority(&self) -> Priority {
        self.priority
    }

    /// Get sequence number
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Get timestamp
    pub fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }

    /// Get payload
    pub fn payload(&self) -> &JsonValue {
        &self.payload
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Get metadata
    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Check if frame is critical priority
    pub fn is_critical(&self) -> bool {
        self.priority.is_critical()
    }

    /// Check if frame is high priority or above
    pub fn is_high_priority(&self) -> bool {
        self.priority.is_high_or_above()
    }

    /// Estimate frame size in bytes (for network planning)
    pub fn estimated_size(&self) -> usize {
        // Rough estimation: JSON serialization + metadata overhead
        let payload_size = self.payload.to_string().len();
        let metadata_size: usize = self
            .metadata
            .iter()
            .map(|(k, v)| k.len() + v.len() + 4) // JSON overhead
            .sum();

        payload_size + metadata_size + 200 // Base frame overhead
    }

    /// Validate frame consistency
    pub fn validate(&self) -> DomainResult<()> {
        match &self.frame_type {
            FrameType::Skeleton => {
                if !self.priority.is_critical() {
                    return Err(DomainError::InvalidFrame(
                        "Skeleton frames must have critical priority".to_string(),
                    ));
                }
            }
            FrameType::Patch => {
                // Validate patch payload structure
                if !self.payload.is_object() {
                    return Err(DomainError::InvalidFrame(
                        "Patch frames must have object payload".to_string(),
                    ));
                }

                if !self.payload.get("patches").map_or(false, |p| p.is_array()) {
                    return Err(DomainError::InvalidFrame(
                        "Patch frames must contain patches array".to_string(),
                    ));
                }
            }
            FrameType::Complete => {
                if !self.priority.is_critical() {
                    return Err(DomainError::InvalidFrame(
                        "Complete frames must have critical priority".to_string(),
                    ));
                }
            }
            FrameType::Error => {
                if !self.priority.is_critical() {
                    return Err(DomainError::InvalidFrame(
                        "Error frames must have critical priority".to_string(),
                    ));
                }

                if !self.payload.get("message").map_or(false, |m| m.is_string()) {
                    return Err(DomainError::InvalidFrame(
                        "Error frames must contain message".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }
}

/// Individual patch within a frame
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FramePatch {
    pub path: JsonPath,
    pub operation: PatchOperation,
    pub value: JsonValue,
}

/// Patch operation types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatchOperation {
    Set,
    Append,
    Merge,
    Delete,
}

/// Patch payload structure
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatchPayload {
    patches: Vec<FramePatch>,
}

impl FramePatch {
    /// Create set operation patch
    pub fn set(path: JsonPath, value: JsonValue) -> Self {
        Self {
            path,
            operation: PatchOperation::Set,
            value,
        }
    }

    /// Create append operation patch
    pub fn append(path: JsonPath, value: JsonValue) -> Self {
        Self {
            path,
            operation: PatchOperation::Append,
            value,
        }
    }

    /// Create merge operation patch
    pub fn merge(path: JsonPath, value: JsonValue) -> Self {
        Self {
            path,
            operation: PatchOperation::Merge,
            value,
        }
    }

    /// Create delete operation patch
    pub fn delete(path: JsonPath) -> Self {
        Self {
            path,
            operation: PatchOperation::Delete,
            value: JsonValue::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skeleton_frame_creation() {
        let stream_id = StreamId::new();
        let skeleton_data = serde_json::json!({
            "users": [],
            "total": 0
        });

        let frame = Frame::skeleton(stream_id, 1, skeleton_data.clone());

        assert_eq!(frame.frame_type(), &FrameType::Skeleton);
        assert_eq!(frame.priority(), Priority::CRITICAL);
        assert_eq!(frame.sequence(), 1);
        assert_eq!(frame.stream_id(), stream_id);
        assert!(frame.validate().is_ok());
    }

    #[test]
    fn test_patch_frame_creation() {
        let stream_id = StreamId::new();
        // TODO: Handle unwrap() - add proper error handling for JsonPath construction in tests
        let path = JsonPath::new("$.users[0].name").unwrap();
        let patch = FramePatch::set(path, JsonValue::String("John".to_string()));

        // TODO: Handle unwrap() - add proper error handling for Frame construction in tests
        let frame = Frame::patch(stream_id, 2, Priority::HIGH, vec![patch]).unwrap();

        assert_eq!(frame.frame_type(), &FrameType::Patch);
        assert_eq!(frame.priority(), Priority::HIGH);
        assert_eq!(frame.sequence(), 2);
        assert!(frame.validate().is_ok());
    }

    #[test]
    fn test_complete_frame_creation() {
        let stream_id = StreamId::new();
        let frame = Frame::complete(stream_id, 10, Some("abc123".to_string()));

        assert_eq!(frame.frame_type(), &FrameType::Complete);
        assert_eq!(frame.priority(), Priority::CRITICAL);
        assert_eq!(frame.sequence(), 10);
        assert!(frame.validate().is_ok());
    }

    #[test]
    fn test_frame_with_metadata() {
        let stream_id = StreamId::new();
        let skeleton_data = serde_json::json!({});
        let frame = Frame::skeleton(stream_id, 1, skeleton_data)
            .with_metadata("source".to_string(), "api".to_string())
            .with_metadata("version".to_string(), "1.0".to_string());

        assert_eq!(frame.get_metadata("source"), Some(&"api".to_string()));
        assert_eq!(frame.get_metadata("version"), Some(&"1.0".to_string()));
        assert_eq!(frame.metadata().len(), 2);
    }

    #[test]
    fn test_empty_patch_validation() {
        let stream_id = StreamId::new();
        let result = Frame::patch(stream_id, 1, Priority::MEDIUM, vec![]);

        assert!(result.is_err());
    }
}
