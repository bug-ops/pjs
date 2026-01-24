//! Frame entity with streaming data

use crate::{
    DomainError, DomainResult,
    value_objects::{JsonData, JsonPath, Priority, StreamId},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Custom serde for StreamId within entities
mod serde_stream_id {
    use crate::value_objects::StreamId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(id: &StreamId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        id.as_uuid().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<StreamId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let uuid = uuid::Uuid::deserialize(deserializer)?;
        Ok(StreamId::from_uuid(uuid))
    }
}

/// Custom serde for Priority within entities
mod serde_priority {
    use crate::value_objects::Priority;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(priority: &Priority, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        priority.value().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Priority, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Priority::new(value).map_err(serde::de::Error::custom)
    }
}

/// Custom serde for JsonPath within entities
mod serde_json_path {
    use crate::value_objects::JsonPath;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(path: &JsonPath, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        path.as_str().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<JsonPath, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        JsonPath::new(s).map_err(serde::de::Error::custom)
    }
}

/// Frame types for different stages of streaming
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    #[serde(with = "serde_stream_id")]
    stream_id: StreamId,
    frame_type: FrameType,
    #[serde(with = "serde_priority")]
    priority: Priority,
    sequence: u64,
    timestamp: DateTime<Utc>,
    payload: JsonData,
    metadata: HashMap<String, String>,
}

impl std::hash::Hash for Frame {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.stream_id.hash(state);
        self.frame_type.hash(state);
        self.priority.hash(state);
        self.sequence.hash(state);
        self.timestamp.hash(state);
        self.payload.hash(state);

        // For HashMap, sort keys for consistent hashing
        let mut pairs: Vec<_> = self.metadata.iter().collect();
        pairs.sort_by_key(|(k, _)| *k);
        pairs.hash(state);
    }
}

impl Frame {
    /// Create new skeleton frame
    pub fn skeleton(stream_id: StreamId, sequence: u64, skeleton_data: JsonData) -> Self {
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

        // Create JsonData payload directly instead of using serde_json
        let mut payload_obj = HashMap::with_capacity(1);
        let patches_array: Vec<JsonData> = patches
            .into_iter()
            .map(|patch| {
                let mut patch_obj = HashMap::with_capacity(3);
                patch_obj.insert("path".into(), JsonData::String(patch.path.to_string()));
                patch_obj.insert(
                    "operation".into(),
                    JsonData::String(
                        match patch.operation {
                            PatchOperation::Set => "set",
                            PatchOperation::Append => "append",
                            PatchOperation::Merge => "merge",
                            PatchOperation::Delete => "delete",
                        }
                        .into(),
                    ),
                );
                patch_obj.insert("value".into(), patch.value);
                JsonData::Object(patch_obj)
            })
            .collect();

        payload_obj.insert("patches".into(), JsonData::Array(patches_array));
        let payload = JsonData::Object(payload_obj);

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
            let mut obj = HashMap::new();
            obj.insert("checksum".to_string(), JsonData::String(checksum));
            JsonData::Object(obj)
        } else {
            JsonData::Object(HashMap::new())
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
            let mut obj = HashMap::new();
            obj.insert("message".to_string(), JsonData::String(error_message));
            obj.insert("code".to_string(), JsonData::String(code));
            JsonData::Object(obj)
        } else {
            let mut obj = HashMap::new();
            obj.insert("message".to_string(), JsonData::String(error_message));
            JsonData::Object(obj)
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
    pub fn payload(&self) -> &JsonData {
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

                if !self.payload.get("patches").is_some_and(|p| p.is_array()) {
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

                if !self.payload.get("message").is_some_and(|m| m.is_string()) {
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
    /// JSON path to the target location
    #[serde(with = "serde_json_path")]
    pub path: JsonPath,
    /// Operation to perform at the path
    pub operation: PatchOperation,
    /// Value to apply with the operation
    pub value: JsonData,
}

/// Patch operation types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatchOperation {
    /// Set a value at the path
    Set,
    /// Append to an array at the path
    Append,
    /// Merge object at the path
    Merge,
    /// Delete value at the path
    Delete,
}

/// Patch payload structure (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatchPayload {
    patches: Vec<FramePatch>,
}

impl FramePatch {
    /// Create set operation patch
    pub fn set(path: JsonPath, value: JsonData) -> Self {
        Self {
            path,
            operation: PatchOperation::Set,
            value,
        }
    }

    /// Create append operation patch
    pub fn append(path: JsonPath, value: JsonData) -> Self {
        Self {
            path,
            operation: PatchOperation::Append,
            value,
        }
    }

    /// Create merge operation patch
    pub fn merge(path: JsonPath, value: JsonData) -> Self {
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
            value: JsonData::Null,
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

        let frame = Frame::skeleton(stream_id, 1, skeleton_data.clone().into());

        assert_eq!(frame.frame_type(), &FrameType::Skeleton);
        assert_eq!(frame.priority(), Priority::CRITICAL);
        assert_eq!(frame.sequence(), 1);
        assert_eq!(frame.stream_id(), stream_id);
        assert!(frame.validate().is_ok());
    }

    #[test]
    fn test_patch_frame_creation() {
        let stream_id = StreamId::new();
        let path = JsonPath::new("$.users[0].name").expect("Failed to create JsonPath in test");
        let patch = FramePatch::set(path, JsonData::String("John".to_string()));

        let frame = Frame::patch(stream_id, 2, Priority::HIGH, vec![patch])
            .expect("Failed to create patch frame in test");

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
        let frame = Frame::skeleton(stream_id, 1, skeleton_data.into())
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
