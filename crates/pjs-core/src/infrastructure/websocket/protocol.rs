//! Wire protocol shared by the WebSocket client and server.
//!
//! Holds the message types and the outbound write helper that both sides of a
//! connection use, so the wire format has a single definition regardless of
//! which WebSocket feature is enabled.

use crate::domain::{Priority, SessionId};
use futures::{Sink, SinkExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, time::Duration};

/// `Error` code the server sends when a session id is already live on the connection.
pub(crate) const CODE_CONFLICT: u16 = 409;

/// Default deadline for a single outbound WebSocket sink write.
///
/// Writing to the sink blocks until the peer's TCP receive buffer drains;
/// a peer that stops reading (dead connection, slow-loris) would otherwise
/// wedge the connection's task — and, on the server, the
/// `Arc<RateLimitGuard>` it holds — until the OS eventually times out the
/// socket. No existing timeout in `config::security::NetworkLimits` fits
/// this: `connection_timeout_secs` bounds establishing a connection, not a
/// steady-state write. 10s is long enough to absorb ordinary network
/// jitter while still freeing a stuck connection promptly.
///
/// This bounds a single `feed`+`flush`, not a minimum throughput: a
/// legitimate large frame (see `MAX_QUEUED_OUTGOING_BYTES` in `server.rs`,
/// up to 16 MiB) sent to a genuinely slow-but-honest client needs roughly
/// 13 Mbps to flush inside 10s, or it gets disconnected same as a stalled
/// peer would. This is an accepted, documented tradeoff rather than a
/// per-byte deadline: distinguishing "slow but making progress" from
/// "stalled" would need tracking partial-write progress, which
/// `Sink::send`'s `feed`+`flush` doesn't expose, and misclassifying a
/// stalled peer as "still making progress" is the failure mode this
/// timeout exists to close. Operators serving large frames to
/// bandwidth-constrained clients should raise the value passed to
/// [`send_with_write_timeout`] accordingly (the server threads its value
/// through `RateLimitConfig::write_timeout`).
///
/// Shares its 10s default with `RateLimitConfig::write_timeout`, but the
/// two are independent constants gated behind different features
/// (`websocket-client` vs. `websocket-server`) and can't easily share a single
/// definition — an intentional change to one's default should be mirrored in
/// the other unless a divergence is deliberate.
#[cfg(feature = "websocket-client")]
pub(crate) const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Writes `message` to `sink`, aborting the write if it does not complete
/// within `timeout` (see `WRITE_TIMEOUT` for the production default and
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
) -> Result<(), String>
where
    S: Sink<M> + Unpin,
    S::Error: std::fmt::Display,
{
    match tokio::time::timeout(timeout, sink.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(format!("send failed: {e}")),
        Err(_elapsed) => Err(format!("write stalled for {timeout:?}")),
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
pub enum WsMessage {
    /// Stream initialization request
    StreamInit {
        /// Identifier of the WebSocket session, chosen by the requesting peer.
        #[serde(with = "pjson_rs_domain::serde_support::session_id")]
        session_id: SessionId,
        /// Source JSON payload to be streamed.
        data: Value,
        /// Per-stream options controlling framing and compression.
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
            Self::FrameAck { .. } => "FrameAck",
            Self::StreamComplete { .. } => "StreamComplete",
            Self::StreamCancel { .. } => "StreamCancel",
            Self::Error { .. } => "Error",
            Self::Ping { .. } => "Ping",
            Self::Pong { .. } => "Pong",
        }
    }
}

/// Stream configuration options.
///
/// The server applies none of these fields: a `StreamInit` is accepted only
/// when its options equal [`StreamOptions::default`], and any other value is
/// answered with an `Error` message (code 400) and no stream. The fields stay
/// on the wire so existing peers, which always send `options`, keep working.
///
/// `priority_mapping` values are validated [`Priority`] values on the wire:
/// decoding rejects an entry that [`Priority::new`] refuses.
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::websocket::StreamOptions;
///
/// assert_eq!(StreamOptions::default(), StreamOptions::default());
/// let custom = StreamOptions { compression: false, ..StreamOptions::default() };
/// assert_ne!(custom, StreamOptions::default());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamOptions {
    /// Maximum frame size in bytes
    pub max_frame_size: usize,
    /// Client processing capability (frames per second)
    pub client_fps: Option<u32>,
    /// Enable compression
    pub compression: bool,
    /// Custom priority mapping
    #[serde(default, with = "option_priority_map")]
    pub priority_mapping: Option<HashMap<String, Priority>>,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            max_frame_size: 64 * 1024, // 64KB
            client_fps: None,          // Auto-detect
            compression: true,
            priority_mapping: None,
        }
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
    use serde_json::json;

    #[tokio::test(start_paused = true)]
    async fn test_send_with_write_timeout_times_out_on_stalled_sink() {
        let handle = tokio::spawn(async {
            let mut sink = futures::sink::unfold((), |_, _item: &str| {
                futures::future::pending::<Result<(), std::io::Error>>()
            });
            send_with_write_timeout(&mut sink, "hello", Duration::from_millis(200)).await
        });

        tokio::time::advance(Duration::from_millis(201)).await;

        let result = handle.await.expect("task panicked");
        assert!(
            result.is_err(),
            "a write that never completes must time out, not hang forever"
        );
    }

    #[cfg(feature = "websocket-client")]
    #[tokio::test]
    async fn test_send_with_write_timeout_succeeds_on_ready_sink() {
        let mut sink = futures::sink::drain();
        send_with_write_timeout(&mut sink, "hello", WRITE_TIMEOUT)
            .await
            .expect("a sink that accepts immediately must not be treated as stalled");
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
        assert!(matches!(
            decoded,
            WsMessage::Error {
                session_id: None,
                ..
            }
        ));
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
        assert!(matches!(decoded, WsMessage::StreamCancel { session_id } if session_id == id));
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
            "max_frame_size": 1, "client_fps": null, "compression": true,
            "priority_mapping": {"field": 0}
        });
        assert!(serde_json::from_value::<StreamOptions>(zero).is_err());

        let absent = json!({"max_frame_size": 1, "client_fps": null, "compression": true});
        assert_eq!(
            serde_json::from_value::<StreamOptions>(absent)
                .unwrap()
                .priority_mapping,
            None
        );
    }
}
