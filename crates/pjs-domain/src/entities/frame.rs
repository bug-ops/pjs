//! Frame entity with streaming data

use crate::{
    DomainError, DomainResult,
    value_objects::{JsonData, JsonPath, Priority, StreamId},
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Frame types for different stages of streaming
///
/// # Wire format
///
/// `FrameType` is serialized as a bare JSON string (Serde's default for fieldless
/// variants). Adding a new variant changes the wire format — older deserializers
/// will fail on unknown frame-type strings. The `#[non_exhaustive]` marker prevents
/// downstream Rust crates from breaking on `match` exhaustiveness, but it does not
/// address the wire-format compatibility concern.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
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

/// Typed content of a [`Frame`], one variant per [`FrameType`].
///
/// A read view: a body is only obtained from a frame built through one of the
/// [`Frame`] constructors or decoded from the wire. Its `Serialize` output is
/// exactly the frame's wire `payload`.
///
/// # Examples
///
/// ```
/// use pjson_rs_domain::entities::frame::{Frame, FrameBody};
/// use pjson_rs_domain::value_objects::StreamId;
///
/// let frame = Frame::complete(StreamId::new(), 1, Some("abc".to_string()));
/// assert!(matches!(frame.body(), FrameBody::Complete { checksum: Some(c) } if c == "abc"));
/// assert_eq!(serde_json::to_string(frame.body()).unwrap(), r#"{"checksum":"abc"}"#);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FrameBody {
    /// Initial skeleton with structure
    Skeleton(JsonData),
    /// Data patches, never empty
    Patch(Vec<FramePatch>),
    /// Stream completion signal
    Complete {
        /// Optional checksum of the streamed document
        checksum: Option<String>,
    },
    /// Error notification
    Error {
        /// Human-readable error message
        message: String,
        /// Optional machine-readable error code
        code: Option<String>,
    },
}

impl FrameBody {
    fn frame_type(&self) -> &'static FrameType {
        match self {
            Self::Skeleton(_) => &FrameType::Skeleton,
            Self::Patch(_) => &FrameType::Patch,
            Self::Complete { .. } => &FrameType::Complete,
            Self::Error { .. } => &FrameType::Error,
        }
    }

    /// Decode the wire payload of a frame of `frame_type`, rejecting every
    /// shape the constructors cannot produce.
    fn from_wire(frame_type: &FrameType, payload: JsonData) -> DomainResult<Self> {
        match frame_type {
            FrameType::Skeleton => Ok(Self::Skeleton(payload)),
            FrameType::Patch => {
                let mut fields = payload_fields(payload, "Patch", &["patches"])?;
                let Some(JsonData::Array(items)) = fields.remove("patches") else {
                    return Err(invalid("Patch payload must contain a patches array"));
                };
                if items.is_empty() {
                    return Err(invalid("Patch frame must contain at least one patch"));
                }
                items
                    .into_iter()
                    .map(FramePatch::from_wire)
                    .collect::<DomainResult<Vec<_>>>()
                    .map(Self::Patch)
            }
            FrameType::Complete => {
                let mut fields = payload_fields(payload, "Complete", &["checksum"])?;
                Ok(Self::Complete {
                    checksum: optional_string(&mut fields, "checksum")?,
                })
            }
            FrameType::Error => {
                let mut fields = payload_fields(payload, "Error", &["message", "code"])?;
                let Some(JsonData::String(message)) = fields.remove("message") else {
                    return Err(invalid("Error payload must contain a string message"));
                };
                Ok(Self::Error {
                    message,
                    code: optional_string(&mut fields, "code")?,
                })
            }
        }
    }
}

fn invalid(message: &str) -> DomainError {
    DomainError::InvalidFrame(message.to_string())
}

/// Unwrap `payload` as an object whose keys are all in `allowed`.
fn payload_fields(
    payload: JsonData,
    kind: &str,
    allowed: &[&str],
) -> DomainResult<HashMap<String, JsonData>> {
    let JsonData::Object(fields) = payload else {
        return Err(DomainError::InvalidFrame(format!(
            "{kind} payload must be an object"
        )));
    };
    match fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(DomainError::InvalidFrame(format!(
            "{kind} payload has unknown key {:?}",
            key.chars().take(32).collect::<String>()
        ))),
        None => Ok(fields),
    }
}

fn optional_string(
    fields: &mut HashMap<String, JsonData>,
    key: &str,
) -> DomainResult<Option<String>> {
    match fields.remove(key) {
        None => Ok(None),
        Some(JsonData::String(value)) => Ok(Some(value)),
        Some(_) => Err(DomainError::InvalidFrame(format!(
            "`{key}` must be a string"
        ))),
    }
}

impl Serialize for FrameBody {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        match self {
            Self::Skeleton(data) => data.serialize(serializer),
            Self::Patch(patches) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("patches", patches)?;
                map.end()
            }
            Self::Complete { checksum } => {
                let mut map = serializer.serialize_map(Some(usize::from(checksum.is_some())))?;
                if let Some(checksum) = checksum {
                    map.serialize_entry("checksum", checksum)?;
                }
                map.end()
            }
            Self::Error { message, code } => {
                let mut map = serializer.serialize_map(Some(1 + usize::from(code.is_some())))?;
                map.serialize_entry("message", message)?;
                if let Some(code) = code {
                    map.serialize_entry("code", code)?;
                }
                map.end()
            }
        }
    }
}

/// Individual frame in a priority stream
///
/// # Wire format
///
/// A frame encodes as `stream_id`, `frame_type`, `priority`, `sequence`,
/// `timestamp`, `payload` (the [`FrameBody`] JSON) and `metadata`. Decoding
/// validates what the constructors guarantee: skeleton, complete and error
/// frames carry a critical priority, and the payload has the shape of its
/// `frame_type`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FrameWire")]
pub struct Frame {
    stream_id: StreamId,
    priority: Priority,
    sequence: u64,
    timestamp: Timestamp,
    body: FrameBody,
    metadata: HashMap<String, String>,
}

/// Private decoding layout of [`Frame`]; validated by `TryFrom`.
#[derive(Deserialize)]
struct FrameWire {
    #[serde(with = "crate::serde_support::stream_id")]
    stream_id: StreamId,
    frame_type: FrameType,
    #[serde(with = "crate::serde_support::priority")]
    priority: Priority,
    sequence: u64,
    timestamp: Timestamp,
    payload: JsonData,
    metadata: HashMap<String, String>,
}

impl TryFrom<FrameWire> for Frame {
    type Error = DomainError;

    fn try_from(wire: FrameWire) -> DomainResult<Self> {
        let body = FrameBody::from_wire(&wire.frame_type, wire.payload)?;
        if !matches!(body, FrameBody::Patch(_)) && !wire.priority.is_critical() {
            return Err(DomainError::InvalidFrame(format!(
                "{:?} frames must have critical priority",
                wire.frame_type
            )));
        }
        Ok(Self {
            stream_id: wire.stream_id,
            priority: wire.priority,
            sequence: wire.sequence,
            timestamp: wire.timestamp,
            body,
            metadata: wire.metadata,
        })
    }
}

/// Borrowed encoding view of [`Frame`], so serializing never clones it.
#[derive(Serialize)]
struct FrameRef<'a> {
    #[serde(with = "crate::serde_support::stream_id")]
    stream_id: StreamId,
    frame_type: &'a FrameType,
    #[serde(with = "crate::serde_support::priority")]
    priority: Priority,
    sequence: u64,
    timestamp: Timestamp,
    payload: &'a FrameBody,
    metadata: &'a HashMap<String, String>,
}

impl Serialize for Frame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        FrameRef {
            stream_id: self.stream_id,
            frame_type: self.frame_type(),
            priority: self.priority,
            sequence: self.sequence,
            timestamp: self.timestamp,
            payload: &self.body,
            metadata: &self.metadata,
        }
        .serialize(serializer)
    }
}

impl std::hash::Hash for Frame {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.stream_id.hash(state);
        self.priority.hash(state);
        self.sequence.hash(state);
        self.timestamp.hash(state);
        self.body.hash(state);

        // For HashMap, sort keys for consistent hashing
        let mut pairs: Vec<_> = self.metadata.iter().collect();
        pairs.sort_by_key(|(k, _)| *k);
        pairs.hash(state);
    }
}

impl Frame {
    fn with_body(stream_id: StreamId, sequence: u64, priority: Priority, body: FrameBody) -> Self {
        Self {
            stream_id,
            priority,
            sequence,
            timestamp: Timestamp::now(),
            body,
            metadata: HashMap::new(),
        }
    }

    /// Replace the creation timestamp; `Stream` uses it to apply its own clock.
    pub(crate) fn with_timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Create new skeleton frame
    pub fn skeleton(stream_id: StreamId, sequence: u64, skeleton_data: JsonData) -> Self {
        Self::with_body(
            stream_id,
            sequence,
            Priority::CRITICAL,
            FrameBody::Skeleton(skeleton_data),
        )
    }

    /// Create new patch frame
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidFrame`] when `patches` is empty.
    pub fn patch(
        stream_id: StreamId,
        sequence: u64,
        priority: Priority,
        patches: Vec<FramePatch>,
    ) -> DomainResult<Self> {
        if patches.is_empty() {
            return Err(invalid("Patch frame must contain at least one patch"));
        }
        Ok(Self::with_body(
            stream_id,
            sequence,
            priority,
            FrameBody::Patch(patches),
        ))
    }

    /// Create completion frame
    pub fn complete(stream_id: StreamId, sequence: u64, checksum: Option<String>) -> Self {
        Self::with_body(
            stream_id,
            sequence,
            Priority::CRITICAL,
            FrameBody::Complete { checksum },
        )
    }

    /// Create error frame
    pub fn error(
        stream_id: StreamId,
        sequence: u64,
        error_message: String,
        error_code: Option<String>,
    ) -> Self {
        Self::with_body(
            stream_id,
            sequence,
            Priority::CRITICAL,
            FrameBody::Error {
                message: error_message,
                code: error_code,
            },
        )
    }

    /// Get stream ID
    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }

    /// Typed body of this frame.
    pub fn body(&self) -> &FrameBody {
        &self.body
    }

    /// Get frame type, derived from the body
    pub fn frame_type(&self) -> &FrameType {
        self.body.frame_type()
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
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
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
    pub fn metadata_value(&self, key: &str) -> Option<&String> {
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
    ///
    /// The length of the body's compact JSON, plus metadata and a fixed
    /// 200-byte frame overhead; computed without building the JSON string.
    pub fn estimated_size(&self) -> usize {
        let mut counter = ByteCounter(0);
        // A `FrameBody` serializes into a counting writer without error: the
        // writer never fails and `JsonData` has only string keys.
        let _ = serde_json::to_writer(&mut counter, &self.body);
        let metadata_size: usize = self
            .metadata
            .iter()
            .map(|(k, v)| k.len() + v.len() + 4) // JSON overhead
            .sum();

        counter.0 + metadata_size + 200
    }
}

/// `io::Write` sink that only counts the bytes written to it.
struct ByteCounter(usize);

impl std::io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Set-patch within a patch frame: assigns `value` at `path`.
///
/// Fields are private; read them through [`FramePatch::path`] and
/// [`FramePatch::value`]. The patch has no standalone wire form: it encodes
/// inside a [`FrameBody::Patch`] payload as
/// `{"path": "<path text>", "operation": "set", "value": <value>}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FramePatch {
    path: JsonPath,
    value: JsonData,
}

impl FramePatch {
    /// Create a patch that sets `value` at `path`.
    pub fn set(path: JsonPath, value: JsonData) -> Self {
        Self { path, value }
    }

    /// Target location of the patch.
    pub fn path(&self) -> &JsonPath {
        &self.path
    }

    /// Value the patch sets.
    pub fn value(&self) -> &JsonData {
        &self.value
    }

    fn from_wire(item: JsonData) -> DomainResult<Self> {
        let mut fields = payload_fields(item, "Patch element", &["path", "operation", "value"])?;
        let Some(JsonData::String(path)) = fields.remove("path") else {
            return Err(invalid("Patch element must contain a string path"));
        };
        match fields.remove("operation") {
            Some(JsonData::String(op)) if op == "set" => {}
            _ => return Err(invalid("Patch element operation must be \"set\"")),
        }
        let value = fields
            .remove("value")
            .ok_or_else(|| invalid("Patch element must contain a value"))?;
        Ok(Self {
            path: JsonPath::new(path)?,
            value,
        })
    }
}

impl Serialize for FramePatch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("path", &self.path.to_string())?;
        map.serialize_entry("operation", "set")?;
        map.serialize_entry("value", &self.value)?;
        map.end()
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
    }

    #[test]
    fn test_complete_frame_creation() {
        let stream_id = StreamId::new();
        let frame = Frame::complete(stream_id, 10, Some("abc123".to_string()));

        assert_eq!(frame.frame_type(), &FrameType::Complete);
        assert_eq!(frame.priority(), Priority::CRITICAL);
        assert_eq!(frame.sequence(), 10);
    }

    #[test]
    fn test_frame_with_metadata() {
        let stream_id = StreamId::new();
        let skeleton_data = serde_json::json!({});
        let frame = Frame::skeleton(stream_id, 1, skeleton_data.into())
            .with_metadata("source".to_string(), "api".to_string())
            .with_metadata("version".to_string(), "1.0".to_string());

        assert_eq!(frame.metadata_value("source"), Some(&"api".to_string()));
        assert_eq!(frame.metadata_value("version"), Some(&"1.0".to_string()));
        assert_eq!(frame.metadata().len(), 2);
    }

    #[test]
    fn test_empty_patch_validation() {
        let stream_id = StreamId::new();
        let result = Frame::patch(stream_id, 1, Priority::MEDIUM, vec![]);

        assert!(result.is_err());
    }
}
