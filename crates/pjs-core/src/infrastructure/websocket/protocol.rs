//! Wire protocol shared by the WebSocket client and server.
//!
//! Holds the message types and the outbound write helper that both sides of a
//! connection use, so the wire format has a single definition regardless of
//! which WebSocket feature is enabled.

use crate::domain::{JsonPath, PathSegment, Priority, SessionId};
#[cfg(any(feature = "websocket-client", test))]
use crate::error::PatchError;
#[cfg(feature = "websocket-server")]
use crate::error::PlanError;
use crate::error::{FrameKind, StreamOptionsError};
#[cfg(any(feature = "websocket-client", test))]
use crate::json_tree::{self, resolve_mut};
use futures::{Sink, SinkExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, num::NonZeroU32, time::Duration};

/// Maximum number of characters of peer-supplied text written to one log line.
pub(crate) const MAX_LOGGED_TEXT_CHARS: usize = 256;

/// Renders peer-supplied text for a log line: escaped, and cut to
/// [`MAX_LOGGED_TEXT_CHARS`] characters with the original length noted.
pub(crate) fn bounded_log_text(text: &str) -> String {
    match text.char_indices().nth(MAX_LOGGED_TEXT_CHARS) {
        None => format!("{text:?}"),
        Some((cut, _)) => format!("{:?}... ({} bytes total)", &text[..cut], text.len()),
    }
}

/// Every `Error` reply the server sends: a code and a fixed, non-echoing text.
///
/// The single definition of the reply table (spec 067), shared by the server
/// (replies to inbound messages), the controller (mid-stream drops) and the
/// client (which recognizes a reply by its exact code and text).
#[cfg(any(feature = "websocket-server", feature = "websocket-client"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerReply {
    /// Undecodable text frame.
    Malformed,
    /// `StreamInit` carrying the nil UUID.
    NilSessionId,
    /// `StreamInit` whose id is already live on the connection.
    DuplicateSessionId,
    /// `StreamInit` over the per-connection live-session cap.
    TooManySessions,
    /// One message exceeds the connection's outgoing byte budget.
    FrameExceedsBudget,
    /// The document's plan exceeds the plan limits (frames, bytes, path depth).
    PlanTooLarge,
    /// The connection already holds the most plan bytes it may.
    PlanBudgetExhausted,
    /// A planned message could not be serialized (a server fault, not the
    /// client's request).
    PlanUnserializable,
    /// One frame waited too long for room in the connection's channel.
    StreamStalled,
    /// The stream outlived its absolute deadline.
    StreamDeadlineExceeded,
}

#[cfg(any(feature = "websocket-server", feature = "websocket-client"))]
impl ServerReply {
    /// Every variant, for exact `(code, text)` matching.
    #[cfg(any(feature = "websocket-client", test))]
    const ALL: [Self; 10] = [
        Self::Malformed,
        Self::NilSessionId,
        Self::DuplicateSessionId,
        Self::TooManySessions,
        Self::FrameExceedsBudget,
        Self::PlanTooLarge,
        Self::PlanBudgetExhausted,
        Self::PlanUnserializable,
        Self::StreamStalled,
        Self::StreamDeadlineExceeded,
    ];

    /// The wire `code`.
    pub(crate) const fn code(self) -> u16 {
        match self {
            Self::Malformed | Self::NilSessionId => 400,
            Self::StreamStalled | Self::StreamDeadlineExceeded => 408,
            Self::DuplicateSessionId => 409,
            Self::FrameExceedsBudget | Self::PlanTooLarge => 413,
            Self::TooManySessions | Self::PlanBudgetExhausted => 429,
            Self::PlanUnserializable => 500,
        }
    }

    /// The fixed wire `error` text.
    pub(crate) const fn reason(self) -> &'static str {
        match self {
            Self::Malformed => "malformed message",
            Self::NilSessionId => "nil session id is not allowed",
            Self::DuplicateSessionId => "session id already in use",
            Self::TooManySessions => "too many live sessions on this connection",
            Self::FrameExceedsBudget => "frame exceeds the outgoing byte budget",
            Self::PlanTooLarge => "stream plan too large",
            Self::PlanBudgetExhausted => "connection plan budget exhausted",
            Self::PlanUnserializable => "stream plan could not be built",
            Self::StreamStalled => "stream stalled",
            Self::StreamDeadlineExceeded => "stream deadline exceeded",
        }
    }

    /// The reply a received `Error` message is, if its code and text both
    /// match one exactly.
    #[cfg(any(feature = "websocket-client", test))]
    pub(crate) fn from_wire(code: u16, reason: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reply| reply.code() == code && reply.reason() == reason)
    }
}

/// Reply for a rejected plan: the connection budget is transient (429), a
/// document over the limits is permanent for that document (413), and a plan
/// that cannot be serialized is a server fault (500).
#[cfg(feature = "websocket-server")]
impl From<PlanError> for ServerReply {
    fn from(error: PlanError) -> Self {
        match error {
            PlanError::ConnectionBudgetExhausted { .. } => Self::PlanBudgetExhausted,
            PlanError::UnitExceedsBudget { .. } => Self::FrameExceedsBudget,
            PlanError::TooManyFrames { .. }
            | PlanError::PlanBytesExceeded { .. }
            | PlanError::PathTooDeep { .. } => Self::PlanTooLarge,
            PlanError::Unserializable => Self::PlanUnserializable,
        }
    }
}

/// Writes `message` to `sink`, aborting the write if it does not complete
/// within `timeout` (see `DEFAULT_WRITE_TIMEOUT` for the production default and
/// the rationale/tradeoffs of a fixed per-write deadline).
///
/// Used at every outbound WebSocket write site (server and client). A
/// timeout is treated the same as a genuine send error — both mean the
/// caller should stop and close the connection. Taking `timeout` as a
/// parameter (rather than hardcoding the default) lets callers
/// configure it (see `RateLimitConfig::write_timeout` on the server side,
/// and `PjsWebSocketClient::with_write_timeout` on the client side) and
/// lets tests exercise a real stall deterministically with a short
/// deadline instead of waiting out the production value.
pub(crate) async fn send_with_write_timeout<S, M>(
    sink: &mut S,
    message: M,
    timeout: Duration,
) -> Result<(), WriteFailure<S::Error>>
where
    S: Sink<M> + Unpin,
{
    match tokio::time::timeout(timeout, sink.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(WriteFailure::Sink(e)),
        Err(_elapsed) => Err(WriteFailure::Stalled(timeout)),
    }
}

/// Why an outbound write did not complete.
#[derive(Debug)]
pub(crate) enum WriteFailure<E> {
    /// The sink reported an error.
    Sink(E),
    /// The write did not complete within the deadline.
    Stalled(Duration),
}

impl<E: std::fmt::Display> std::fmt::Display for WriteFailure<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sink(e) => write!(f, "send failed: {e}"),
            Self::Stalled(timeout) => write!(f, "write stalled for {timeout:?}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for WriteFailure<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sink(e) => Some(e),
            Self::Stalled(_) => None,
        }
    }
}

/// WebSocket message types for PJS streaming.
///
/// Session ids travel as UUID strings. A peer may send any textual UUID form,
/// but the server always echoes the canonical lowercase hyphenated form.
/// Priorities travel as `u8` and are validated on decode: a value that
/// [`Priority::new`] refuses (such as `0`) fails the whole message.
///
/// # Examples
///
/// ```
/// use pjson_rs::domain::{Priority, SessionId};
/// use pjson_rs::infrastructure::websocket::WsMessage;
/// use serde_json::json;
///
/// let frame = WsMessage::StreamFrame {
///     session_id: SessionId::new(),
///     frame_id: 0,
///     priority: Priority::HIGH,
///     payload: json!({"k": 1}),
///     is_complete: true,
/// };
/// let wire = serde_json::to_string(&frame).unwrap();
/// let decoded: WsMessage = serde_json::from_str(&wire).unwrap();
/// assert!(matches!(decoded, WsMessage::StreamFrame { priority, .. } if priority == Priority::HIGH));
///
/// let zero = r#"{"type":"StreamFrame","data":{"session_id":"00000000-0000-4000-8000-000000000001","frame_id":0,"priority":0,"payload":null,"is_complete":true}}"#;
/// assert!(serde_json::from_str::<WsMessage>(zero).is_err());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
#[non_exhaustive]
pub enum WsMessage {
    /// Stream initialization request
    StreamInit {
        /// Identifier of the WebSocket session, chosen by the requesting peer.
        #[serde(with = "pjson_rs_domain::serde_support::session_id")]
        session_id: SessionId,
        /// Source JSON payload to be streamed.
        data: Value,
        /// Per-stream options controlling framing and pacing.
        options: StreamOptions,
    },
    /// Stream frame with priority data
    StreamFrame {
        /// Identifier of the WebSocket session.
        #[serde(with = "pjson_rs_domain::serde_support::session_id")]
        session_id: SessionId,
        /// Monotonic frame index within the session.
        frame_id: u32,
        /// Priority assigned to this frame.
        #[serde(with = "pjson_rs_domain::serde_support::priority")]
        priority: Priority,
        /// Payload carried by the frame.
        payload: Value,
        /// Whether this frame completes the stream.
        is_complete: bool,
    },
    /// Stream frame carrying patch operations
    StreamPatch {
        /// Identifier of the WebSocket session.
        #[serde(with = "pjson_rs_domain::serde_support::session_id")]
        session_id: SessionId,
        /// Monotonic frame index within the session.
        frame_id: u32,
        /// Priority assigned to this frame.
        #[serde(with = "pjson_rs_domain::serde_support::priority")]
        priority: Priority,
        /// Patch operations carried by the frame.
        patches: Vec<WirePatch>,
        /// Whether this frame completes the stream.
        is_complete: bool,
    },
    /// Client acknowledgment of frame
    FrameAck {
        /// Identifier of the WebSocket session.
        #[serde(with = "pjson_rs_domain::serde_support::session_id")]
        session_id: SessionId,
        /// Index of the frame being acknowledged.
        frame_id: u32,
        /// Time the client took to process the frame, in milliseconds.
        processing_time_ms: u64,
    },
    /// Stream completion signal
    StreamComplete {
        /// Identifier of the WebSocket session.
        #[serde(with = "pjson_rs_domain::serde_support::session_id")]
        session_id: SessionId,
    },
    /// Client request to stop streaming a session it owns
    StreamCancel {
        /// Identifier of the WebSocket session to cancel.
        #[serde(with = "pjson_rs_domain::serde_support::session_id")]
        session_id: SessionId,
    },
    /// Error message
    Error {
        /// Identifier of the WebSocket session, if known.
        #[serde(default, with = "pjson_rs_domain::serde_support::option_session_id")]
        session_id: Option<SessionId>,
        /// Human-readable error description.
        error: String,
        /// Numeric error code.
        code: u16,
    },
    /// Heartbeat/ping message
    Ping {
        /// Wall-clock timestamp at which the ping was sent.
        timestamp: u64,
    },
    /// Heartbeat/pong response
    Pong {
        /// Wall-clock timestamp at which the pong was sent.
        timestamp: u64,
    },
}

impl WsMessage {
    /// Returns the wire name of this message's variant, with no payload.
    ///
    /// Safe to log in place of the message itself, which may carry
    /// client-controlled data.
    #[must_use]
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::StreamInit { .. } => "StreamInit",
            Self::StreamFrame { .. } => "StreamFrame",
            Self::StreamPatch { .. } => "StreamPatch",
            Self::FrameAck { .. } => "FrameAck",
            Self::StreamComplete { .. } => "StreamComplete",
            Self::StreamCancel { .. } => "StreamCancel",
            Self::Error { .. } => "Error",
            Self::Ping { .. } => "Ping",
            Self::Pong { .. } => "Pong",
        }
    }
}

/// Frame encoding a stream uses on the wire.
///
/// [`Framing::Merge`] is the original encoding: `StreamFrame` payloads that a
/// receiver shallow-merges into its state. [`Framing::Patch`] sends
/// [`WsMessage::StreamPatch`] frames whose [`WirePatch`] operations address any
/// location by path, so arbitrarily large or deep documents can be split.
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::websocket::Framing;
///
/// assert_eq!(Framing::default(), Framing::Merge);
/// assert_eq!(serde_json::to_string(&Framing::Patch).unwrap(), "\"patch\"");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Framing {
    /// Top-level key merge frames
    #[default]
    Merge,
    /// Patch operation frames
    Patch,
}

impl Framing {
    /// `skip_serializing_if` hook: merge framing is the wire default and is
    /// omitted, so a merge `StreamInit` carries no `framing` key.
    fn is_merge(&self) -> bool {
        matches!(self, Self::Merge)
    }
}

/// Longest [`JsonPath`] a patch may carry.
///
/// Receivers refuse longer paths so a hostile peer cannot grow their state's
/// nesting without bound (recursive drop would overflow the stack); the server
/// refuses to plan documents that need longer ones.
pub const MAX_PATCH_PATH_DEPTH: usize = 128;

/// Wire form of a [`JsonPath`] segment: a JSON string addresses an object
/// member and a JSON integer an array element, so every key (including `.`,
/// `[`, `]` and the empty string) is expressible and `"0"` stays distinct from
/// index `0`.
struct WireSegment<'a>(&'a PathSegment);

impl Serialize for WireSegment<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            PathSegment::Key(key) => serializer.serialize_str(key),
            PathSegment::Index(index) => serializer.serialize_u64(*index as u64),
        }
    }
}

struct OwnedWireSegment(PathSegment);

impl<'de> Deserialize<'de> for OwnedWireSegment {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SegmentVisitor;

        impl serde::de::Visitor<'_> for SegmentVisitor {
            type Value = OwnedWireSegment;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a string key or a non-negative integer index")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<OwnedWireSegment, E> {
                Ok(OwnedWireSegment(PathSegment::Key(v.to_owned())))
            }

            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<OwnedWireSegment, E> {
                Ok(OwnedWireSegment(PathSegment::Key(v)))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<OwnedWireSegment, E> {
                usize::try_from(v)
                    .map(|index| OwnedWireSegment(PathSegment::Index(index)))
                    .map_err(|_| E::custom("array index does not fit in usize"))
            }
        }

        deserializer.deserialize_any(SegmentVisitor)
    }
}

/// Serialize adapter writing a [`JsonPath`] as the wire array of segments.
struct ArrayPathRef<'a>(&'a JsonPath);

impl Serialize for ArrayPathRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.segments().iter().map(WireSegment))
    }
}

/// Deserialize adapter reading the wire array of segments into a [`JsonPath`].
struct ArrayPath(JsonPath);

impl<'de> Deserialize<'de> for ArrayPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let segments = Vec::<OwnedWireSegment>::deserialize(deserializer)?;
        Ok(Self(JsonPath::from_segments(
            segments.into_iter().map(|segment| segment.0),
        )))
    }
}

impl From<Framing> for FrameKind {
    fn from(framing: Framing) -> Self {
        match framing {
            Framing::Merge => Self::Merge,
            Framing::Patch => Self::Patch,
        }
    }
}

/// One patch operation of a [`WsMessage::StreamPatch`] frame.
///
/// The wire form is the 3-array `[op, path, argument]` with `op` one of
/// `"set"`, `"merge"`, `"append"`, `"append_str"`:
///
/// | op | argument | effect |
/// |----|----------|--------|
/// | `set` | any value | replaces the value at `path` (the document for `[]`); a parent object gets the key inserted, a parent array needs an existing index |
/// | `merge` | object | inserts or replaces each entry of the object at `path` (shallow) |
/// | `append` | array | appends the elements to the array at `path` |
/// | `append_str` | string | appends the text to the string at `path` |
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::websocket::WirePatch;
///
/// let patch: WirePatch = serde_json::from_str(r#"["append_str", ["s"], "tail"]"#).unwrap();
/// assert_eq!(serde_json::to_string(&patch).unwrap(), r#"["append_str",["s"],"tail"]"#);
/// assert!(serde_json::from_str::<WirePatch>(r#"["merge", [], [1]]"#).is_err());
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WirePatch {
    /// Replace the value at `path`
    Set {
        /// Target location
        path: JsonPath,
        /// New value
        value: Value,
    },
    /// Insert or replace object entries at `path`
    Merge {
        /// Target location
        path: JsonPath,
        /// Entries to insert
        entries: serde_json::Map<String, Value>,
    },
    /// Append array elements at `path`
    Append {
        /// Target location
        path: JsonPath,
        /// Elements to append
        values: Vec<Value>,
    },
    /// Append text to the string at `path`
    AppendStr {
        /// Target location
        path: JsonPath,
        /// Text to append
        text: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PatchOp {
    Set,
    Merge,
    Append,
    AppendStr,
}

impl Serialize for WirePatch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;

        let mut seq = serializer.serialize_seq(Some(3))?;
        match self {
            Self::Set { path, value } => {
                seq.serialize_element("set")?;
                seq.serialize_element(&ArrayPathRef(path))?;
                seq.serialize_element(value)?;
            }
            Self::Merge { path, entries } => {
                seq.serialize_element("merge")?;
                seq.serialize_element(&ArrayPathRef(path))?;
                seq.serialize_element(entries)?;
            }
            Self::Append { path, values } => {
                seq.serialize_element("append")?;
                seq.serialize_element(&ArrayPathRef(path))?;
                seq.serialize_element(values)?;
            }
            Self::AppendStr { path, text } => {
                seq.serialize_element("append_str")?;
                seq.serialize_element(&ArrayPathRef(path))?;
                seq.serialize_element(text)?;
            }
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for WirePatch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // A sequence visitor reads the op first and then decodes the argument
        // directly into its typed form; an internally tagged or untagged enum
        // would buffer every `Value` through serde's `Content`.
        struct PatchVisitor;

        impl<'de> serde::de::Visitor<'de> for PatchVisitor {
            type Value = WirePatch;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a 3-array [op, path, argument]")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<WirePatch, A::Error> {
                use serde::de::Error;

                fn missing<E: Error>(index: usize) -> E {
                    E::invalid_length(index, &"a 3-array [op, path, argument]")
                }

                let op: PatchOp = seq.next_element()?.ok_or_else(|| missing(0))?;
                let ArrayPath(path) = seq.next_element()?.ok_or_else(|| missing(1))?;
                let patch = match op {
                    PatchOp::Set => WirePatch::Set {
                        path,
                        value: seq.next_element()?.ok_or_else(|| missing(2))?,
                    },
                    PatchOp::Merge => WirePatch::Merge {
                        path,
                        entries: seq.next_element()?.ok_or_else(|| missing(2))?,
                    },
                    PatchOp::Append => WirePatch::Append {
                        path,
                        values: seq.next_element()?.ok_or_else(|| missing(2))?,
                    },
                    PatchOp::AppendStr => WirePatch::AppendStr {
                        path,
                        text: seq.next_element()?.ok_or_else(|| missing(2))?,
                    },
                };
                if seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(A::Error::invalid_length(
                        4,
                        &"a 3-array [op, path, argument]",
                    ));
                }
                Ok(patch)
            }
        }

        deserializer.deserialize_seq(PatchVisitor)
    }
}

/// Applies `patch` to `state`, moving its payload into the state.
///
/// `state` may already be partly modified when an error is returned; callers
/// drop the stream in that case.
///
/// # Errors
///
/// Returns [`PatchError::PathTooDeep`] for a path longer than
/// [`MAX_PATCH_PATH_DEPTH`], a resolution error ([`PatchError::KeyNotFound`],
/// [`PatchError::IndexOutOfBounds`], [`PatchError::ExpectedObject`],
/// [`PatchError::ExpectedArray`]) when the path does not resolve, and
/// [`PatchError::ExpectedObject`] / [`PatchError::ExpectedArray`] /
/// [`PatchError::ExpectedString`] when the target has the wrong kind for the op.
#[cfg(any(feature = "websocket-client", test))]
pub(crate) fn apply_patch(state: &mut Value, patch: WirePatch) -> Result<(), PatchError> {
    let (WirePatch::Set { path, .. }
    | WirePatch::Merge { path, .. }
    | WirePatch::Append { path, .. }
    | WirePatch::AppendStr { path, .. }) = &patch;
    let segments = path.segments();
    if segments.len() > MAX_PATCH_PATH_DEPTH {
        return Err(PatchError::PathTooDeep {
            depth: segments.len(),
            max: MAX_PATCH_PATH_DEPTH,
        });
    }

    match patch {
        WirePatch::Set { path, value } => json_tree::set(state, path.segments(), value)?,
        WirePatch::Merge { path, entries } => {
            let Value::Object(target) = resolve_mut(state, path.segments())? else {
                return Err(PatchError::ExpectedObject);
            };
            target.extend(entries);
        }
        WirePatch::Append { path, values } => {
            let Value::Array(target) = resolve_mut(state, path.segments())? else {
                return Err(PatchError::ExpectedArray);
            };
            target.extend(values);
        }
        WirePatch::AppendStr { path, text } => {
            let Value::String(target) = resolve_mut(state, path.segments())? else {
                return Err(PatchError::ExpectedString);
            };
            target.push_str(&text);
        }
    }
    Ok(())
}

/// Stream configuration options, carried by `StreamInit`.
///
/// `max_frame_size` must be at least 1024 and `client_fps` must not be zero;
/// the types enforce it at decode time, so a `StreamInit` carrying other values
/// is an undecodable message (answered `400 "malformed message"`, no stream).
///
/// `priority_mapping` values are validated [`Priority`] values on the wire:
/// decoding rejects an entry that [`Priority::new`] refuses. Top-level keys
/// with a mapping stream first (highest priority first); other keys, and a
/// non-object document, use [`Priority::HIGH`].
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::websocket::{Framing, MaxFrameSize, StreamOptions};
///
/// assert_eq!(StreamOptions::default().framing, Framing::Merge);
/// let mut split = StreamOptions::patch();
/// split.max_frame_size = MaxFrameSize::new(16 * 1024).unwrap();
/// assert_ne!(split, StreamOptions::default());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StreamOptions {
    /// Largest message the server sends, in bytes (at least 1024).
    ///
    /// The limit covers the whole message, envelope included. A piece that
    /// cannot be split further (a single top-level entry under
    /// [`Framing::Merge`]) may exceed it and then travels alone.
    pub max_frame_size: MaxFrameSize,
    /// Client processing capability in frames per second (never zero).
    ///
    /// Sets a floor of `1 / fps` seconds between frames while the plan has at
    /// most `30 * fps` frames; longer plans shrink the delay so that pacing
    /// adds at most 30 seconds in total.
    pub client_fps: Option<NonZeroU32>,
    /// Custom priority mapping
    #[serde(default, with = "option_priority_map")]
    pub priority_mapping: Option<HashMap<String, Priority>>,
    /// Frame encoding the stream uses; omitted on the wire when [`Framing::Merge`]
    #[serde(default, skip_serializing_if = "Framing::is_merge")]
    pub framing: Framing,
}

impl StreamOptions {
    /// Default options with [`Framing::Patch`], the encoding
    /// [`PjsWebSocketClient`](super::PjsWebSocketClient) reconstructs.
    pub fn patch() -> Self {
        Self {
            framing: Framing::Patch,
            ..Self::default()
        }
    }
}

/// Validated `max_frame_size`: the largest message the server sends, in bytes.
///
/// Decoding a value below [`MaxFrameSize::MIN`] fails, so an invalid size
/// cannot reach the planner.
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::websocket::MaxFrameSize;
///
/// assert_eq!(MaxFrameSize::new(1024).unwrap().get(), 1024);
/// assert!(MaxFrameSize::new(1023).is_err());
/// assert_eq!(MaxFrameSize::default().get(), 64 * 1024);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "usize", into = "usize")]
pub struct MaxFrameSize(usize);

impl MaxFrameSize {
    /// Smallest accepted size in bytes.
    pub const MIN: usize = 1024;

    /// Validates `bytes` against [`MaxFrameSize::MIN`].
    ///
    /// # Errors
    ///
    /// Returns [`StreamOptionsError::MaxFrameSizeTooSmall`] below the minimum.
    pub fn new(bytes: usize) -> Result<Self, StreamOptionsError> {
        if bytes < Self::MIN {
            return Err(StreamOptionsError::MaxFrameSizeTooSmall {
                requested: bytes,
                min: Self::MIN,
            });
        }
        Ok(Self(bytes))
    }

    /// The validated size in bytes.
    pub fn get(self) -> usize {
        self.0
    }
}

impl TryFrom<usize> for MaxFrameSize {
    type Error = StreamOptionsError;

    fn try_from(bytes: usize) -> Result<Self, Self::Error> {
        Self::new(bytes)
    }
}

impl From<MaxFrameSize> for usize {
    fn from(size: MaxFrameSize) -> Self {
        size.get()
    }
}

impl Default for MaxFrameSize {
    fn default() -> Self {
        Self(64 * 1024)
    }
}

/// `Option<HashMap<String, Priority>>` on top of the domain's validated
/// `priority_map` encoding.
mod option_priority_map {
    use crate::domain::Priority;
    use pjson_rs_domain::serde_support::priority_map;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    #[derive(Serialize)]
    struct Encoded<'a>(#[serde(with = "priority_map")] &'a HashMap<String, Priority>);

    #[derive(Deserialize)]
    struct Decoded(#[serde(with = "priority_map")] HashMap<String, Priority>);

    pub(super) fn serialize<S: Serializer>(
        map: &Option<HashMap<String, Priority>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        map.as_ref().map(Encoded).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<HashMap<String, Priority>>, D::Error> {
        Ok(Option::<Decoded>::deserialize(deserializer)?.map(|Decoded(map)| map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "websocket-client")]
    use crate::security::rate_limit::DEFAULT_WRITE_TIMEOUT;
    use core::assert_matches;
    use serde_json::json;

    #[test]
    fn test_server_reply_all_lists_every_variant_with_unique_wire_pairs() {
        // The exhaustive match makes a new variant a compile error here, so
        // `ALL` cannot silently miss it: the indices must cover `0..N`.
        let index = |reply: ServerReply| match reply {
            ServerReply::Malformed => 0,
            ServerReply::NilSessionId => 1,
            ServerReply::DuplicateSessionId => 2,
            ServerReply::TooManySessions => 3,
            ServerReply::FrameExceedsBudget => 4,
            ServerReply::PlanTooLarge => 5,
            ServerReply::PlanBudgetExhausted => 6,
            ServerReply::PlanUnserializable => 7,
            ServerReply::StreamStalled => 8,
            ServerReply::StreamDeadlineExceeded => 9,
        };
        let mut seen: Vec<usize> = ServerReply::ALL.into_iter().map(index).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..ServerReply::ALL.len()).collect::<Vec<_>>());
        assert_eq!(ServerReply::ALL.len(), 10);

        let mut pairs: Vec<(u16, &str)> = ServerReply::ALL
            .into_iter()
            .map(|reply| (reply.code(), reply.reason()))
            .collect();
        pairs.sort_unstable();
        pairs.dedup();
        assert_eq!(pairs.len(), ServerReply::ALL.len(), "wire pairs are unique");
        for reply in ServerReply::ALL {
            assert_eq!(
                ServerReply::from_wire(reply.code(), reply.reason()),
                Some(reply)
            );
        }
        assert_eq!(ServerReply::from_wire(409, "some other conflict"), None);
    }

    #[tokio::test(start_paused = true)]
    async fn test_send_with_write_timeout_times_out_on_stalled_sink() {
        let handle = tokio::spawn(async {
            let mut sink = futures::sink::unfold((), |_, _item: String| {
                futures::future::pending::<Result<(), std::io::Error>>()
            });
            send_with_write_timeout(&mut sink, "hello".to_owned(), Duration::from_millis(200))
                .await
                .is_err()
        });

        tokio::time::advance(Duration::from_millis(201)).await;

        let timed_out = handle.await.expect("task panicked");
        assert!(
            timed_out,
            "a write that never completes must time out, not hang forever"
        );
    }

    #[cfg(feature = "websocket-client")]
    #[tokio::test]
    async fn test_send_with_write_timeout_succeeds_on_ready_sink() {
        let mut sink = futures::sink::drain();
        send_with_write_timeout(&mut sink, "hello", DEFAULT_WRITE_TIMEOUT)
            .await
            .expect("a sink that accepts immediately must not be treated as stalled");
    }

    fn key(k: &str) -> PathSegment {
        PathSegment::Key(k.to_owned())
    }

    fn set(path: Vec<PathSegment>, value: Value) -> WirePatch {
        WirePatch::Set {
            path: JsonPath::from_segments(path),
            value,
        }
    }

    #[test]
    fn test_apply_set_root_replaces_state() {
        let mut state = json!({"old": 1});
        apply_patch(&mut state, set(vec![], json!([1]))).unwrap();
        assert_eq!(state, json!([1]));
    }

    #[test]
    fn test_apply_set_inserts_replaces_key_and_replaces_existing_index() {
        let mut state = json!({"a": [0, 1]});
        apply_patch(&mut state, set(vec![key("b")], json!(2))).unwrap();
        apply_patch(
            &mut state,
            set(vec![key("a"), PathSegment::Index(1)], json!("x")),
        )
        .unwrap();
        assert_eq!(state, json!({"a": [0, "x"], "b": 2}));
    }

    #[test]
    fn test_apply_set_index_out_of_bounds_is_rejected() {
        let mut state = json!([0]);
        let err = apply_patch(&mut state, set(vec![PathSegment::Index(1)], json!(1))).unwrap_err();
        assert_eq!(err, PatchError::IndexOutOfBounds { index: 1, len: 1 });
    }

    #[test]
    fn test_apply_set_wrong_parent_kind_is_rejected() {
        let mut state = json!({"a": 1});
        assert_eq!(
            apply_patch(&mut state, set(vec![PathSegment::Index(0)], json!(1))).unwrap_err(),
            PatchError::ExpectedArray
        );
        let mut state = json!([1]);
        assert_eq!(
            apply_patch(&mut state, set(vec![key("k")], json!(1))).unwrap_err(),
            PatchError::ExpectedObject
        );
    }

    #[test]
    fn test_apply_merge_extends_object_and_rejects_non_object() {
        let mut state = json!({"o": {"a": 1}, "n": 5});
        let merge = |path: &str, entries: Value| WirePatch::Merge {
            path: JsonPath::from_segments(vec![key(path)]),
            entries: entries.as_object().unwrap().clone(),
        };
        apply_patch(&mut state, merge("o", json!({"a": 2, "b": 3}))).unwrap();
        assert_eq!(state["o"], json!({"a": 2, "b": 3}));
        assert_eq!(
            apply_patch(&mut state, merge("n", json!({}))).unwrap_err(),
            PatchError::ExpectedObject
        );
    }

    #[test]
    fn test_apply_append_and_append_str_check_target_kind() {
        let mut state = json!({"l": [1], "s": "ab"});
        let at = |k: &str| JsonPath::from_segments(vec![key(k)]);
        apply_patch(
            &mut state,
            WirePatch::Append {
                path: at("l"),
                values: vec![json!(2), json!(null)],
            },
        )
        .unwrap();
        apply_patch(
            &mut state,
            WirePatch::AppendStr {
                path: at("s"),
                text: "cd".into(),
            },
        )
        .unwrap();
        assert_eq!(state, json!({"l": [1, 2, null], "s": "abcd"}));
        assert_eq!(
            apply_patch(
                &mut state,
                WirePatch::Append {
                    path: at("s"),
                    values: vec![]
                }
            )
            .unwrap_err(),
            PatchError::ExpectedArray
        );
        assert_eq!(
            apply_patch(
                &mut state,
                WirePatch::AppendStr {
                    path: at("l"),
                    text: String::new()
                }
            )
            .unwrap_err(),
            PatchError::ExpectedString
        );
    }

    #[test]
    fn test_apply_unresolvable_path_is_rejected() {
        let mut state = json!({"a": 1});
        assert_eq!(
            apply_patch(&mut state, set(vec![key("zz"), key("k")], json!(1))).unwrap_err(),
            PatchError::KeyNotFound("zz".into())
        );
        assert_eq!(
            apply_patch(&mut state, set(vec![key("a"), key("k")], json!(1))).unwrap_err(),
            PatchError::ExpectedObject
        );
    }

    #[test]
    fn test_apply_path_deeper_than_cap_is_rejected_before_resolving() {
        let mut state = json!({});
        let at_cap = vec![key("k"); MAX_PATCH_PATH_DEPTH];
        let too_deep = vec![key("k"); MAX_PATCH_PATH_DEPTH + 1];
        assert!(!matches!(
            apply_patch(&mut state, set(at_cap, json!(1))),
            Err(PatchError::PathTooDeep { .. })
        ));
        assert_eq!(
            apply_patch(&mut state, set(too_deep, json!(1))).unwrap_err(),
            PatchError::PathTooDeep {
                depth: MAX_PATCH_PATH_DEPTH + 1,
                max: MAX_PATCH_PATH_DEPTH
            }
        );
    }

    #[test]
    fn test_wire_patch_rejects_trailing_element_and_oversized_index() {
        assert!(serde_json::from_str::<WirePatch>(r#"["set", [], 1, 2]"#).is_err());
        assert!(
            serde_json::from_str::<WirePatch>(r#"["set", [18446744073709551616], 1]"#).is_err()
        );
    }

    #[test]
    fn test_stream_patch_kind_is_its_wire_tag() {
        let message = WsMessage::StreamPatch {
            session_id: SessionId::new(),
            frame_id: 0,
            priority: Priority::HIGH,
            patches: vec![],
            is_complete: true,
        };
        assert_eq!(
            serde_json::to_value(&message).unwrap()["type"],
            message.kind()
        );
    }

    #[test]
    fn test_kind_matches_wire_type_tag_for_every_variant() {
        let session_id = SessionId::new();
        let messages = [
            WsMessage::StreamInit {
                session_id,
                data: json!(null),
                options: StreamOptions::default(),
            },
            WsMessage::StreamFrame {
                session_id,
                frame_id: 0,
                priority: Priority::HIGH,
                payload: json!(null),
                is_complete: false,
            },
            WsMessage::FrameAck {
                session_id,
                frame_id: 0,
                processing_time_ms: 0,
            },
            WsMessage::StreamComplete { session_id },
            WsMessage::StreamCancel { session_id },
            WsMessage::Error {
                session_id: None,
                error: String::new(),
                code: 0,
            },
            WsMessage::Ping { timestamp: 0 },
            WsMessage::Pong { timestamp: 0 },
        ];
        for message in &messages {
            let wire = serde_json::to_value(message).unwrap();
            assert_eq!(wire["type"], message.kind());
        }
    }

    #[test]
    fn test_error_message_without_session_id_field_decodes_to_none() {
        let decoded: WsMessage =
            serde_json::from_str(r#"{"type":"Error","data":{"error":"x","code":1}}"#).unwrap();
        assert_matches!(
            decoded,
            WsMessage::Error {
                session_id: None,
                ..
            }
        );
    }

    #[test]
    fn test_ws_message_session_id_round_trips_as_canonical_uuid() {
        let id = SessionId::new();
        let message = WsMessage::FrameAck {
            session_id: id,
            frame_id: 3,
            processing_time_ms: 1,
        };
        let value = serde_json::to_value(&message).unwrap();
        assert_eq!(value["data"]["session_id"], id.as_uuid().to_string());
        assert!(serde_json::from_str::<WsMessage>(
            r#"{"type":"FrameAck","data":{"session_id":"not-a-uuid","frame_id":0,"processing_time_ms":0}}"#
        )
        .is_err());
    }

    #[test]
    fn test_stream_frame_priority_travels_as_u8_and_rejects_zero() {
        let id = SessionId::new();
        let message = WsMessage::StreamFrame {
            session_id: id,
            frame_id: 0,
            priority: Priority::HIGH,
            payload: json!(null),
            is_complete: true,
        };
        let value = serde_json::to_value(&message).unwrap();
        assert_eq!(value["data"]["priority"], 80);

        let mut zero = value;
        zero["data"]["priority"] = json!(0);
        assert!(serde_json::from_value::<WsMessage>(zero).is_err());
    }

    #[test]
    fn test_stream_complete_has_no_checksum_field() {
        let message = WsMessage::StreamComplete {
            session_id: SessionId::new(),
        };
        let value = serde_json::to_value(&message).unwrap();
        assert!(value["data"].get("checksum").is_none());
    }

    #[test]
    fn test_stream_cancel_round_trips() {
        let id = SessionId::new();
        let wire = serde_json::to_string(&WsMessage::StreamCancel { session_id: id }).unwrap();
        assert!(wire.contains(r#""type":"StreamCancel""#));
        let decoded: WsMessage = serde_json::from_str(&wire).unwrap();
        assert_matches!(decoded, WsMessage::StreamCancel { session_id } if session_id == id);
    }

    #[test]
    fn test_priority_mapping_round_trips_validates_and_defaults_to_none() {
        let options = StreamOptions {
            priority_mapping: Some(HashMap::from([("field".to_owned(), Priority::CRITICAL)])),
            ..StreamOptions::default()
        };
        let value = serde_json::to_value(&options).unwrap();
        assert_eq!(value["priority_mapping"]["field"], 100);
        assert_eq!(
            serde_json::from_value::<StreamOptions>(value).unwrap(),
            options
        );

        let zero = json!({
            "max_frame_size": 1024, "client_fps": null, "compression": true,
            "priority_mapping": {"field": 0}
        });
        assert!(serde_json::from_value::<StreamOptions>(zero).is_err());

        let absent = json!({"max_frame_size": 1024, "client_fps": null, "compression": true});
        assert_eq!(
            serde_json::from_value::<StreamOptions>(absent)
                .unwrap()
                .priority_mapping,
            None
        );
    }
}
