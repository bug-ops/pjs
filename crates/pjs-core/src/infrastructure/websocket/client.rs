//! WebSocket client implementation for PJS streaming

#[cfg(feature = "websocket-client")]
use super::{Framing, StreamOptions, WriteFailure, WsMessage};
use crate::{
    Error as PjsError, Result as PjsResult,
    config::ConfigError,
    domain::{PathSegment, SessionId},
    error::{ErrorChain, FrameSequenceError, LimitKind},
    infrastructure::bounded_channel::{ByteBoundedSender, Envelope, byte_bounded_channel},
    json_tree::resolve,
};
use futures::{Sink, Stream, StreamExt};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::HashMap,
    mem::DropGuard,
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, mpsc};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Error as WsError, Message, protocol::WebSocketConfig},
};
use tracing::{debug, error, info, warn};

#[cfg(test)]
use super::protocol::MAX_LOGGED_TEXT_CHARS;
use super::protocol::{ServerReply, WirePatch, apply_patch, bounded_log_text};
use crate::security::rate_limit::{DEFAULT_MAX_INBOUND_FRAME_SIZE, DEFAULT_WRITE_TIMEOUT};
use crate::serialized_len::{json_len, json_str_len};
use url::Url;

/// Capacity of the client's outgoing message channel.
///
/// Bounds how many outgoing messages (stream requests, acks, pongs) can
/// queue while the send half of the connection catches up. This is a message-count bound only;
/// [`MAX_QUEUED_MESSAGE_BYTES`] additionally bounds cumulative queued
/// bytes.
const MESSAGE_QUEUE_CAPACITY: usize = 1000;

/// Cumulative byte budget for the client's outgoing message channel, on
/// top of [`MESSAGE_QUEUE_CAPACITY`]'s message-count bound.
///
/// Keeps worst-case queued memory a small, predictable constant regardless
/// of individual message size (e.g. a large `StreamInit` payload).
const MAX_QUEUED_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// Default cap on concurrently tracked client sessions.
const DEFAULT_MAX_SESSIONS: NonZeroUsize = NonZeroUsize::new(1024).unwrap();

/// Default idle time after which a session that stopped receiving data expires.
const DEFAULT_SESSION_IDLE_TTL: Duration = Duration::from_mins(5);

/// Default cap, in bytes, on the data reconstructed for a single session.
const DEFAULT_MAX_SESSION_BYTES: NonZeroUsize = NonZeroUsize::new(16 * 1024 * 1024).unwrap();

/// How long a client session may go without receiving data before it expires.
///
/// Always positive. Idle time is measured from the last frame applied to the
/// session, or from its creation if none arrived. Completed sessions are
/// frozen: late frames do not refresh them, so they expire after this time
/// too.
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::websocket::SessionIdleTtl;
/// use std::time::Duration;
///
/// assert!(SessionIdleTtl::new(Duration::from_secs(30)).is_ok());
/// assert!(SessionIdleTtl::new(Duration::ZERO).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionIdleTtl(Duration);

impl SessionIdleTtl {
    /// Creates an idle time-to-live.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MustBePositive`] for a zero duration.
    pub fn new(ttl: Duration) -> Result<Self, ConfigError> {
        if ttl.is_zero() {
            return Err(ConfigError::MustBePositive {
                section: "websocket.client",
                field: "session_idle_ttl",
            });
        }
        Ok(Self(ttl))
    }

    /// The time-to-live as a plain duration.
    pub fn as_duration(self) -> Duration {
        self.0
    }
}

impl Default for SessionIdleTtl {
    fn default() -> Self {
        Self(DEFAULT_SESSION_IDLE_TTL)
    }
}

/// Transport and state bounds applied to untrusted server input.
///
/// The message and frame caps default to the in-tree server's inbound cap
/// ([`RateLimitConfig::max_frame_size`](crate::security::RateLimitConfig::max_frame_size)),
/// because the server answers a `StreamInit` with the whole request data in a
/// single frame; a smaller client cap would sever the connection to it.
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::websocket::WebSocketClientConfig;
///
/// let mut config = WebSocketClientConfig::default();
/// config.max_message_size = 128 * 1024;
/// assert_eq!(config.max_frame_size, 1024 * 1024);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct WebSocketClientConfig {
    /// Maximum size in bytes of a complete incoming WebSocket message.
    pub max_message_size: usize,
    /// Maximum size in bytes of a single incoming WebSocket frame.
    pub max_frame_size: usize,
    /// Maximum number of sessions tracked at once.
    ///
    /// Together with [`max_session_bytes`](Self::max_session_bytes) this bounds
    /// client memory; the default is 1024 sessions of up to 16 MiB each.
    ///
    /// When the limit is reached, a new request first drops expired sessions,
    /// then evicts the incomplete session that has been idle the longest (it
    /// stops receiving frames). Completed sessions are never evicted, so a
    /// request fails with [`PjsError::SessionLimitReached`] when every
    /// tracked session is complete; release them with
    /// [`PjsWebSocketClient::close_session`].
    pub max_sessions: NonZeroUsize,
    /// Idle time after which a session expires and is treated as unknown.
    ///
    /// Memory held by an expired session is released at the next
    /// [`PjsWebSocketClient::request_stream`], at
    /// [`PjsWebSocketClient::close_session`], or when a message arrives for
    /// it; it is always bounded by `max_sessions` x `max_session_bytes`.
    pub session_idle_ttl: SessionIdleTtl,
    /// Maximum size in bytes of the data reconstructed for one session.
    ///
    /// The size is measured as serialized JSON, not as heap usage: a
    /// `serde_json::Value` made of many small numbers can occupy up to about
    /// 16 times more memory than its serialized form, so the worst-case
    /// footprint is `max_sessions` x `max_session_bytes` x ~16. A frame that
    /// would exceed the limit is rejected and its session is dropped.
    pub max_session_bytes: NonZeroUsize,
}

impl Default for WebSocketClientConfig {
    fn default() -> Self {
        Self {
            max_message_size: DEFAULT_MAX_INBOUND_FRAME_SIZE,
            max_frame_size: DEFAULT_MAX_INBOUND_FRAME_SIZE,
            max_sessions: DEFAULT_MAX_SESSIONS,
            session_idle_ttl: SessionIdleTtl::default(),
            max_session_bytes: DEFAULT_MAX_SESSION_BYTES,
        }
    }
}

impl WebSocketClientConfig {
    fn websocket_config(&self) -> WebSocketConfig {
        WebSocketConfig::default()
            .max_message_size(Some(self.max_message_size))
            .max_frame_size(Some(self.max_frame_size))
    }
}

/// WebSocket client for receiving PJS streams
pub struct PjsWebSocketClient {
    url: Url,
    config: WebSocketClientConfig,
    sessions: ClientSessions,
    message_tx: ByteBoundedSender<String>,
    message_rx: ReceiverSlot,
    write_timeout: Duration,
}

type ClientSessions = Arc<RwLock<HashMap<SessionId, ClientStreamSession>>>;

type ReceiverSlot = Arc<Mutex<Option<Outbox>>>;

/// Outgoing queue that outlives a connection.
///
/// `pending` is the message taken off the channel but not yet written
/// successfully; it stays here until its write completes, so a failed,
/// timed-out or cancelled write leaves it for the next connection.
struct Outbox {
    receiver: mpsc::Receiver<Envelope<String>>,
    pending: Option<Envelope<String>>,
}

impl Outbox {
    fn new(receiver: mpsc::Receiver<Envelope<String>>) -> Self {
        Self {
            receiver,
            pending: None,
        }
    }
}

/// Exclusive use of the outgoing message receiver for one connection.
///
/// Returns the receiver to its slot on drop, so queued messages survive the
/// connection and the next `connect` can send them.
struct ReceiverLease {
    slot: ReceiverSlot,
    outbox: Option<Outbox>,
}

impl ReceiverLease {
    fn acquire(slot: &ReceiverSlot) -> PjsResult<Self> {
        let outbox = slot.lock().take().ok_or(PjsError::AlreadyConnected)?;
        Ok(Self {
            slot: Arc::clone(slot),
            outbox: Some(outbox),
        })
    }

    fn outbox_mut(&mut self) -> PjsResult<&mut Outbox> {
        self.outbox.as_mut().ok_or(PjsError::ChannelClosed)
    }
}

impl Drop for ReceiverLease {
    fn drop(&mut self) {
        if let Some(outbox) = self.outbox.take() {
            *self.slot.lock() = Some(outbox);
        }
    }
}

/// Client-side stream session
#[derive(Debug)]
pub(super) struct ClientStreamSession {
    id: SessionId,
    created_at: Instant,
    /// Last time a frame was applied to the session; freezes on completion.
    last_activity: Instant,
    frames_received: usize,
    framing: Framing,
    next_frame_id: u32,
    total_processing_time: Duration,
    pub(super) reconstructed_data: Value,
    pub(super) reconstructed_bytes: usize,
    is_complete: bool,
}

impl ClientStreamSession {
    pub(super) fn new(id: SessionId, framing: Framing) -> Self {
        let now = Instant::now();
        Self {
            id,
            created_at: now,
            last_activity: now,
            frames_received: 0,
            framing,
            next_frame_id: 0,
            total_processing_time: Duration::ZERO,
            reconstructed_data: serde_json::json!({}),
            reconstructed_bytes: json_len(&serde_json::json!({})),
            is_complete: false,
        }
    }

    fn is_expired(&self, now: Instant, ttl: SessionIdleTtl) -> bool {
        now.saturating_duration_since(self.last_activity) > ttl.as_duration()
    }

    fn stats(&self) -> StreamStats {
        let total_frames = self.frames_received;
        let average_processing_time = u32::try_from(total_frames)
            .ok()
            .filter(|&n| n > 0)
            .map(|n| self.total_processing_time / n);

        StreamStats {
            session_id: self.id,
            total_frames,
            is_complete: self.is_complete,
            duration: self.created_at.elapsed(),
            average_processing_time,
        }
    }

    fn record_frame(&mut self, processing_time: Duration) {
        self.frames_received = self.frames_received.saturating_add(1);
        self.total_processing_time = self.total_processing_time.saturating_add(processing_time);
    }

    /// Accepts a data frame of `kind` with `frame_id` into the sequence: the kind
    /// must be the requested framing and the id the next one (from 0).
    ///
    /// # Errors
    ///
    /// [`PjsError::FrameSequence`] when either differs; nothing is advanced.
    fn accept_frame(&mut self, kind: Framing, frame_id: u32) -> PjsResult<()> {
        if kind != self.framing {
            return Err(FrameSequenceError::FramingMismatch {
                expected: self.framing.into(),
                received: kind.into(),
            }
            .into());
        }
        if frame_id != self.next_frame_id {
            return Err(FrameSequenceError::UnexpectedFrameId {
                expected: self.next_frame_id,
                received: frame_id,
            }
            .into());
        }
        self.next_frame_id = self.next_frame_id.saturating_add(1);
        Ok(())
    }

    /// Merges `payload` into the reconstructed data, refusing the frame
    /// (without mutating anything) if the result would exceed `max_bytes`.
    pub(super) fn apply_frame(&mut self, payload: Value, max_bytes: usize) -> PjsResult<()> {
        let projected = match (self.reconstructed_data.as_object(), payload.as_object()) {
            (Some(data_map), Some(payload_map)) => {
                merged_bytes(self.reconstructed_bytes, Some(data_map), payload_map)
            }
            _ => json_len(&payload),
        };
        if projected > max_bytes {
            return Err(PjsError::limit_exceeded(
                LimitKind::SessionBytes,
                projected,
                max_bytes,
            ));
        }

        match (self.reconstructed_data.as_object_mut(), payload) {
            (Some(data_map), Value::Object(payload_map)) => data_map.extend(payload_map),
            (_, payload) => self.reconstructed_data = payload,
        }
        self.reconstructed_bytes = projected;
        Ok(())
    }

    /// Applies `patches` in order, accounting each against `max_bytes` before it
    /// is applied. On error the state may be partly modified; the caller drops
    /// the session.
    pub(super) fn apply_patches(
        &mut self,
        patches: Vec<WirePatch>,
        max_bytes: usize,
    ) -> PjsResult<()> {
        for patch in patches {
            let projected = self.project_patch_bytes(&patch);
            if projected > max_bytes {
                return Err(PjsError::limit_exceeded(
                    LimitKind::SessionBytes,
                    projected,
                    max_bytes,
                ));
            }
            apply_patch(&mut self.reconstructed_data, patch)?;
            self.reconstructed_bytes = projected;
        }
        Ok(())
    }

    /// Upper bound on `reconstructed_bytes` after `patch` applies; an
    /// unresolvable path is counted as new content, `apply_patch` rejects it.
    fn project_patch_bytes(&self, patch: &WirePatch) -> usize {
        let bytes = self.reconstructed_bytes;
        let data = &self.reconstructed_data;
        match patch {
            WirePatch::Set { path, value } => match path.segments().split_last() {
                None => json_len(value),
                Some((PathSegment::Key(key), parent)) => {
                    let old = resolve(data, parent)
                        .and_then(Value::as_object)
                        .and_then(|map| map.get(key));
                    bytes
                        .saturating_add(entry_size(key, value))
                        .saturating_sub(old.map_or(0, |old| entry_size(key, old)))
                }
                Some((PathSegment::Index(_), _)) => {
                    let old = resolve(data, path.segments()).map_or(0, json_len);
                    bytes.saturating_add(json_len(value)).saturating_sub(old)
                }
            },
            WirePatch::Merge { path, entries } => merged_bytes(
                bytes,
                resolve(data, path.segments()).and_then(Value::as_object),
                entries,
            ),
            WirePatch::Append { values, .. } => values.iter().fold(bytes, |acc, value| {
                acc.saturating_add(json_len(value)).saturating_add(1)
            }),
            WirePatch::AppendStr { text, .. } => {
                bytes.saturating_add(json_str_len(text).saturating_sub(2))
            }
        }
    }
}

/// Bytes after inserting `entries` into `existing` (`None` when the target is
/// not an object): each entry adds its size and removes the one it replaces.
fn merged_bytes(
    base: usize,
    existing: Option<&serde_json::Map<String, Value>>,
    entries: &serde_json::Map<String, Value>,
) -> usize {
    let mut bytes = base;
    for (key, value) in entries {
        bytes = bytes.saturating_add(entry_size(key, value));
        if let Some(old) = existing.and_then(|map| map.get(key)) {
            bytes = bytes.saturating_sub(entry_size(key, old));
        }
    }
    bytes
}

/// Upper bound on the serialized size of one object entry: quoted key, colon,
/// value and a separating comma.
fn entry_size(key: &str, value: &Value) -> usize {
    json_str_len(key)
        .saturating_add(2)
        .saturating_add(json_len(value))
}

/// Removes the session from the map; used as a drop guard so a cancelled
/// `request_stream` cannot leak a slot.
fn remove_session_slot((sessions, id): (ClientSessions, SessionId)) {
    if let Ok(mut map) = sessions.try_write() {
        map.remove(&id);
    } else if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async move {
            sessions.write().await.remove(&id);
        });
    }
}

/// Returns the session unless it has expired; expired sessions read as unknown.
fn live_session<'a>(
    sessions: &'a HashMap<SessionId, ClientStreamSession>,
    id: &SessionId,
    ttl: SessionIdleTtl,
    now: Instant,
) -> Option<&'a ClientStreamSession> {
    sessions
        .get(id)
        .filter(|session| !session.is_expired(now, ttl))
}

/// Makes room for a new session.
///
/// When full, drops expired sessions; if the map is still full, evicts the incomplete
/// session that has been idle the longest. Completed sessions are never
/// evicted.
///
/// # Errors
///
/// Returns [`PjsError::SessionLimitReached`] when the map is full and every
/// session is complete.
fn reserve_session_slot(
    sessions: &mut HashMap<SessionId, ClientStreamSession>,
    max_sessions: NonZeroUsize,
    ttl: SessionIdleTtl,
    now: Instant,
) -> PjsResult<()> {
    if sessions.len() < max_sessions.get() {
        return Ok(());
    }
    sessions.retain(|_, session| !session.is_expired(now, ttl));
    while sessions.len() >= max_sessions.get() {
        let oldest_idle = sessions
            .iter()
            .filter(|(_, session)| !session.is_complete)
            .min_by_key(|(_, session)| session.last_activity)
            .map(|(id, _)| *id);
        let Some(id) = oldest_idle else {
            return Err(PjsError::SessionLimitReached {
                limit: max_sessions,
            });
        };
        sessions.remove(&id);
    }
    Ok(())
}

/// Writes queued outgoing messages to the socket until the queue closes.
///
/// Messages are already serialized when queued (see `request_stream` and
/// `handle_incoming_message`), so the byte-budget accounting there matches the
/// bytes held here. A message stays in `outbox.pending`, with its budget
/// charged, until its write succeeds: after an error or a cancellation it is
/// sent first on the next connection (at-least-once: a write cancelled just
/// after reaching the socket may be sent twice).
///
/// # Errors
///
/// Returns [`PjsError::Transport`] when a write fails and
/// [`PjsError::WriteTimeout`] when it times out.
async fn send_loop<W>(mut write: W, outbox: &mut Outbox, write_timeout: Duration) -> PjsResult<()>
where
    W: Sink<Message, Error = WsError> + Unpin,
{
    loop {
        if outbox.pending.is_none() {
            let Some(envelope) = outbox.receiver.recv().await else {
                return Ok(());
            };
            outbox.pending = Some(envelope);
        }
        let Some(envelope) = outbox.pending.as_ref() else {
            continue;
        };
        // The envelope stays in `outbox` (budget still charged) until the write
        // succeeds; a copy goes to the socket so a failure or cancellation
        // cannot lose the message.
        if let Err(e) = super::send_with_write_timeout(
            &mut write,
            Message::text(envelope.as_str()),
            write_timeout,
        )
        .await
        {
            error!(error = %e, "Failed to send message");
            return Err(match e {
                WriteFailure::Sink(source) => PjsError::Transport(Box::new(source)),
                WriteFailure::Stalled(timeout) => PjsError::WriteTimeout { timeout },
            });
        }
        outbox.pending = None;
    }
}

/// Renders a connection URL for logging without userinfo, query, or fragment.
fn redacted_url(url: &Url) -> String {
    let mut redacted = url.clone();
    // Both setters fail only for cannot-be-a-base URLs, which carry no userinfo.
    let _ = redacted.set_username("");
    let _ = redacted.set_password(None);
    redacted.set_query(None);
    redacted.set_fragment(None);
    redacted.into()
}

/// Reads server messages and applies them until the server closes the socket.
///
/// # Errors
///
/// Returns [`PjsError::Transport`] on a transport error.
async fn receive_loop<R>(
    mut read: R,
    sessions: ClientSessions,
    message_tx: ByteBoundedSender<String>,
    config: WebSocketClientConfig,
) -> PjsResult<()>
where
    R: Stream<Item = Result<Message, WsError>> + Unpin,
{
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => match serde_json::from_str::<WsMessage>(&text) {
                Ok(ws_message) => {
                    if let Err(e) = PjsWebSocketClient::handle_incoming_message(
                        sessions.clone(),
                        message_tx.clone(),
                        config,
                        ws_message,
                    )
                    .await
                    {
                        error!(error = %ErrorChain(&e), "Failed to handle incoming message");
                    }
                }
                Err(e) => {
                    warn!(
                        category = ?e.classify(),
                        line = e.line(),
                        column = e.column(),
                        bytes = text.len(),
                        "Failed to parse incoming message"
                    );
                }
            },
            Ok(Message::Binary(data)) => {
                debug!(bytes = data.len(), "Received binary data");
            }
            Ok(Message::Ping(_data)) => {
                debug!("Received ping, sending pong");
                // Pong is handled automatically by tungstenite
            }
            Ok(Message::Pong(_)) => {
                debug!("Received pong");
            }
            Ok(Message::Close(_)) => {
                info!("Server closed connection");
                break;
            }
            Ok(Message::Frame(_)) => {
                // Raw frame - usually handled internally by tungstenite
                debug!("Received raw frame");
            }
            Err(e) => {
                error!(error = %e, "WebSocket error");
                return Err(PjsError::Transport(Box::new(e)));
            }
        }
    }
    Ok(())
}

impl PjsWebSocketClient {
    /// Create new WebSocket client
    pub fn new(url: impl AsRef<str>) -> PjsResult<Self> {
        let url = Url::parse(url.as_ref()).map_err(|e| PjsError::InvalidUrl(Box::new(e)))?;

        let (message_tx, message_rx) =
            byte_bounded_channel(MESSAGE_QUEUE_CAPACITY, MAX_QUEUED_MESSAGE_BYTES);

        Ok(Self {
            url,
            config: WebSocketClientConfig::default(),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            message_tx,
            message_rx: Arc::new(Mutex::new(Some(Outbox::new(message_rx)))),
            write_timeout: DEFAULT_WRITE_TIMEOUT,
        })
    }

    /// Overrides the transport and state bounds applied to server input.
    ///
    /// Defaults to [`WebSocketClientConfig::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::websocket::{PjsWebSocketClient, WebSocketClientConfig};
    /// use std::num::NonZeroUsize;
    ///
    /// let mut config = WebSocketClientConfig::default();
    /// config.max_sessions = NonZeroUsize::new(16).unwrap();
    /// let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
    ///     .unwrap()
    ///     .with_config(config);
    /// ```
    #[must_use]
    pub fn with_config(mut self, config: WebSocketClientConfig) -> Self {
        self.config = config;
        self
    }

    /// Overrides the deadline for a single outbound WebSocket sink write,
    /// used by the send half of [`Self::connect`].
    ///
    /// Defaults to `DEFAULT_WRITE_TIMEOUT` (10s) — see
    /// its doc for the rationale and the tradeoff it implies for large
    /// frames sent to slow clients. Pair a shorter value with a
    /// resource-constrained deployment where freeing a wedged connection
    /// quickly matters more than absorbing network jitter (mirroring
    /// `RateLimitConfig::low_resource`'s tightened `write_timeout` on the
    /// server side); pair a longer value with a deployment that expects
    /// large payloads over slow or high-latency uplinks and would
    /// otherwise see legitimate writes misclassified as stalled.
    ///
    /// `write_timeout` is not validated: [`Duration::ZERO`] leaves at most
    /// one poll of the underlying write before it is treated as a timeout,
    /// and an arbitrarily large value (including [`Duration::MAX`]) is
    /// accepted as-is and does not panic, since `tokio::time::timeout`
    /// clamps internally.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::websocket::PjsWebSocketClient;
    /// use std::time::Duration;
    ///
    /// let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
    ///     .unwrap()
    ///     .with_write_timeout(Duration::from_secs(3));
    /// ```
    #[must_use]
    pub fn with_write_timeout(mut self, write_timeout: Duration) -> Self {
        self.write_timeout = write_timeout;
        self
    }

    /// Drives one connection to the server until it ends.
    ///
    /// The returned future completes only when the connection is over: the
    /// server closed the socket (`Ok(())`), or a read or write failed. It never
    /// returns once the handshake succeeds while the server stays up, so
    /// `connect` must run in its own task (the client is not `Clone`; share it
    /// through an [`Arc`]) while other tasks call
    /// [`request_stream`](Self::request_stream). Dropping the future closes the
    /// connection.
    ///
    /// Both halves run inside this future, so nothing outlives it. The client
    /// can connect again afterwards; messages queued in between (for example
    /// by `request_stream`) are sent on the next connection. Only one
    /// connection can be active at a time.
    ///
    /// # Errors
    ///
    /// Returns [`PjsError::AlreadyConnected`] if a connection is already
    /// active, [`PjsError::ConnectionFailed`] if the handshake fails,
    /// [`PjsError::WriteTimeout`] if a write times out, or
    /// [`PjsError::Transport`] if a write fails or the connection is aborted by
    /// a transport error (including a message or frame exceeding the
    /// configured [`WebSocketClientConfig`] caps).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use pjson_rs::infrastructure::websocket::{PjsWebSocketClient, StreamOptions};
    /// use serde_json::json;
    /// use std::sync::Arc;
    ///
    /// # async fn run() -> pjson_rs::Result<()> {
    /// let client = Arc::new(PjsWebSocketClient::new("ws://127.0.0.1:3001/ws")?);
    ///
    /// let connection = {
    ///     let client = Arc::clone(&client);
    ///     tokio::spawn(async move { client.connect().await })
    /// };
    ///
    /// let data = json!({"hello": "world"});
    /// let session = client.request_stream(data, StreamOptions::patch()).await?;
    /// # let _ = session;
    ///
    /// // Completes when the server closes the connection.
    /// connection.await.expect("connection task panicked")?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(&self) -> PjsResult<()> {
        let mut lease = ReceiverLease::acquire(&self.message_rx)?;

        info!(url = %redacted_url(&self.url), "Connecting to WebSocket server");

        let (ws_stream, _) = connect_async_with_config(
            self.url.as_str(),
            Some(self.config.websocket_config()),
            false,
        )
        .await
        .map_err(|e| PjsError::ConnectionFailed {
            url: redacted_url(&self.url),
            source: Box::new(e),
        })?;

        info!("WebSocket connection established");

        let (write, read) = ws_stream.split();

        let outcome = tokio::select! {
            result = send_loop(write, lease.outbox_mut()?, self.write_timeout) => {
                debug!("Send half finished");
                result
            }
            result = receive_loop(
                read,
                self.sessions.clone(),
                self.message_tx.clone(),
                self.config,
            ) => {
                debug!("Receive half finished");
                result
            }
        };

        info!("WebSocket connection closed");
        outcome
    }

    /// Request stream initialization and return the new session id.
    ///
    /// The id is chosen here and adopted by the server, so it is also the key
    /// of every later frame for this stream. When the session limit is
    /// reached, expired sessions are dropped and then the incomplete session
    /// idle the longest is evicted (see [`WebSocketClientConfig::max_sessions`]).
    ///
    /// `options` is sent verbatim. [`StreamOptions::patch`] requests
    /// [`super::Framing::Patch`], which this client reconstructs, so arbitrarily large
    /// documents stream in bounded frames; [`StreamOptions::default`] is
    /// [`super::Framing::Merge`], the encoding peers that predate patch framing
    /// understand.
    ///
    /// # Errors
    ///
    /// Returns [`PjsError::SessionLimitReached`] if every tracked session is
    /// complete, or a client error if the request cannot be serialized or the
    /// outgoing channel is closed.
    pub async fn request_stream(
        &self,
        data: Value,
        options: StreamOptions,
    ) -> PjsResult<SessionId> {
        let session_id = SessionId::new();
        let framing = options.framing;

        let message = WsMessage::StreamInit {
            session_id,
            data,
            options,
        };
        let json_str = serde_json::to_string(&message)?;
        let len = json_str.len();

        {
            let mut sessions = self.sessions.write().await;
            reserve_session_slot(
                &mut sessions,
                self.config.max_sessions,
                self.config.session_idle_ttl,
                Instant::now(),
            )?;
            sessions.insert(session_id, ClientStreamSession::new(session_id, framing));
        }
        let slot = DropGuard::new(
            (Arc::clone(&self.sessions), session_id),
            remove_session_slot,
        );

        self.message_tx
            .send(json_str, len)
            .await
            .map_err(|_| PjsError::ChannelClosed)?;
        DropGuard::dismiss(slot);

        info!(%session_id, "Requested stream initialization");
        Ok(session_id)
    }

    /// Applies `read` to the session unless it is unknown or expired.
    async fn read_live<T>(
        &self,
        session_id: SessionId,
        read: impl FnOnce(&ClientStreamSession) -> T,
    ) -> Option<T> {
        let sessions = self.sessions.read().await;
        live_session(
            &sessions,
            &session_id,
            self.config.session_idle_ttl,
            Instant::now(),
        )
        .map(read)
    }

    /// Get current reconstructed data for session.
    ///
    /// `None` for unknown and expired sessions.
    pub async fn get_current_data(&self, session_id: SessionId) -> PjsResult<Option<Value>> {
        Ok(self
            .read_live(session_id, |session| session.reconstructed_data.clone())
            .await)
    }

    /// Check if stream is complete.
    ///
    /// Returns `false` for unknown sessions and for expired ones, even if the
    /// session had completed before it expired, so a caller polling for
    /// completion should bound the wait. Read the data with
    /// [`Self::close_session`] before the idle time passes.
    pub async fn is_stream_complete(&self, session_id: SessionId) -> bool {
        self.read_live(session_id, |session| session.is_complete)
            .await
            .unwrap_or(false)
    }

    /// Get stream statistics; `None` for unknown and expired sessions.
    pub async fn get_stream_stats(&self, session_id: SessionId) -> Option<StreamStats> {
        self.read_live(session_id, ClientStreamSession::stats).await
    }

    /// Stops tracking a session and returns what it reconstructed.
    ///
    /// Frees the session's slot, including a completed one that the session
    /// limit never evicts. Returns `None` for unknown and expired sessions.
    ///
    /// For a session that is not complete, also queues a best-effort
    /// `StreamCancel` so the server stops streaming it (expired sessions
    /// included, since the server may still be sending). The cancel goes out on
    /// the current or next connection; it is dropped, with a warning, when the
    /// outgoing queue is full. Late frames for the id are ignored either way.
    pub async fn close_session(&self, session_id: SessionId) -> Option<ClosedSession> {
        self.close_session_at(session_id, Instant::now()).await
    }

    async fn close_session_at(&self, session_id: SessionId, now: Instant) -> Option<ClosedSession> {
        let session = self.sessions.write().await.remove(&session_id)?;
        if !session.is_complete {
            Self::try_send_control_message(
                &self.message_tx,
                &WsMessage::StreamCancel { session_id },
                "stream cancel",
            );
        }
        if session.is_expired(now, self.config.session_idle_ttl) {
            return None;
        }
        let stats = session.stats();
        Some(ClosedSession {
            data: session.reconstructed_data,
            stats,
        })
    }

    /// Best-effort control-message send: serializes `message` and
    /// `try_send`s it, logging (rather than propagating) both serialization
    /// and send failures. Used for acks/pongs sent from
    /// `handle_incoming_message`, which runs inline inside the read loop —
    /// awaiting a full channel there would stall draining the socket, so
    /// dropping is preferred over blocking (see call sites for the fuller
    /// rationale).
    fn try_send_control_message(
        message_tx: &ByteBoundedSender<String>,
        message: &WsMessage,
        kind: &str,
    ) {
        match serde_json::to_string(message) {
            Ok(json_str) => {
                let len = json_str.len();
                if let Err(e) = message_tx.try_send(json_str, len) {
                    warn!(kind, error = ?e, "Dropping message (channel full, byte budget exceeded, or closed)");
                }
            }
            Err(e) => warn!(kind, error = %e, "Failed to serialize message"),
        }
    }

    /// Removes `session_id` if it has expired, so it reads as unknown.
    fn drop_if_expired(
        sessions: &mut HashMap<SessionId, ClientStreamSession>,
        session_id: &SessionId,
        config: WebSocketClientConfig,
        now: Instant,
    ) {
        if sessions
            .get(session_id)
            .is_some_and(|session| session.is_expired(now, config.session_idle_ttl))
        {
            sessions.remove(session_id);
        }
    }

    /// Applies one data frame to its live session through `apply` and
    /// acknowledges it; a failed `apply` drops the session and returns the error.
    /// Frames for unknown, expired or completed sessions are ignored unacked.
    async fn apply_and_ack(
        sessions: &ClientSessions,
        message_tx: &ByteBoundedSender<String>,
        config: WebSocketClientConfig,
        (session_id, frame_id, is_complete): (SessionId, u32, bool),
        apply: impl FnOnce(&mut ClientStreamSession) -> PjsResult<()>,
    ) -> PjsResult<()> {
        debug!(frame_id, %session_id, "Received frame");

        let processing_start = Instant::now();

        let applied = {
            let mut sessions = sessions.write().await;
            Self::drop_if_expired(&mut sessions, &session_id, config, processing_start);
            match sessions.get_mut(&session_id) {
                Some(session) if !session.is_complete => {
                    if let Err(e) = apply(session) {
                        sessions.remove(&session_id);
                        return Err(e);
                    }
                    if is_complete {
                        session.is_complete = true;
                        info!(%session_id, "Stream completed");
                    }
                    session.last_activity = processing_start;
                    session.record_frame(processing_start.elapsed());
                    true
                }
                _ => false,
            }
        };
        if !applied {
            return Ok(());
        }

        let processing_time = processing_start.elapsed();

        let ack_message = WsMessage::FrameAck {
            session_id,
            frame_id,
            processing_time_ms: u64::try_from(processing_time.as_millis()).unwrap_or(u64::MAX),
        };

        // Best-effort: a dropped ack only costs the server its pacing
        // signal; stalling the whole read loop is not recoverable. See `try_send_control_message`'s doc.
        Self::try_send_control_message(message_tx, &ack_message, "frame acknowledgment");
        Ok(())
    }

    async fn handle_incoming_message(
        sessions: ClientSessions,
        message_tx: ByteBoundedSender<String>,
        config: WebSocketClientConfig,
        message: WsMessage,
    ) -> PjsResult<()> {
        match message {
            WsMessage::StreamFrame {
                session_id,
                frame_id,
                priority: _,
                payload,
                is_complete,
            } => {
                Self::apply_and_ack(
                    &sessions,
                    &message_tx,
                    config,
                    (session_id, frame_id, is_complete),
                    |session| {
                        session.accept_frame(Framing::Merge, frame_id)?;
                        session.apply_frame(payload, config.max_session_bytes.get())
                    },
                )
                .await?;
            }
            WsMessage::StreamPatch {
                session_id,
                frame_id,
                priority: _,
                patches,
                is_complete,
            } => {
                Self::apply_and_ack(
                    &sessions,
                    &message_tx,
                    config,
                    (session_id, frame_id, is_complete),
                    move |session| {
                        session.accept_frame(Framing::Patch, frame_id)?;
                        session.apply_patches(patches, config.max_session_bytes.get())
                    },
                )
                .await?;
            }
            WsMessage::StreamComplete { session_id } => {
                info!(%session_id, "Stream completed");

                let now = Instant::now();
                let mut sessions = sessions.write().await;
                Self::drop_if_expired(&mut sessions, &session_id, config, now);
                if let Some(session) = sessions.get_mut(&session_id)
                    && !session.is_complete
                {
                    session.is_complete = true;
                    session.last_activity = now;
                }
            }
            WsMessage::Error {
                session_id,
                error,
                code,
            } => {
                // A duplicate-id reply means the server still streams the existing
                // live session under this id, so the client keeps tracking it.
                // Only the exact code and text count: any other `409` is not
                // proof the session is alive.
                let is_duplicate =
                    ServerReply::from_wire(code, &error) == Some(ServerReply::DuplicateSessionId);
                let removed = match session_id {
                    Some(id) if !is_duplicate => sessions.write().await.remove(&id).is_some(),
                    _ => false,
                };
                error!(
                    ?session_id,
                    error = %bounded_log_text(&error),
                    code,
                    session_removed = removed,
                    "Received error from server"
                );
            }
            WsMessage::Ping { timestamp } => {
                debug!(timestamp, "Received ping");
                let pong = WsMessage::Pong { timestamp };
                // Same rationale as the ack above: best-effort, must not
                // stall the read loop by awaiting a full channel.
                Self::try_send_control_message(&message_tx, &pong, "pong");
            }
            WsMessage::Pong { timestamp } => {
                debug!(timestamp, "Received pong");
            }
            _ => {
                warn!(kind = message.kind(), "Unhandled message type");
            }
        }
        Ok(())
    }
}

/// Stream statistics
#[derive(Debug, Clone)]
pub struct StreamStats {
    /// Identifier of the streaming session.
    pub session_id: SessionId,
    /// Number of frames successfully applied to the session.
    ///
    /// A repeated `frame_id` counts again, and frames rejected (for example
    /// for exceeding the session data cap) are not counted.
    pub total_frames: usize,
    /// Whether the stream has been marked complete.
    pub is_complete: bool,
    /// Wall-clock duration since the session was created.
    pub duration: Duration,
    /// Average per-frame processing duration, if any frames have been applied.
    pub average_processing_time: Option<Duration>,
}

/// A session removed by [`PjsWebSocketClient::close_session`].
#[derive(Debug, Clone)]
pub struct ClosedSession {
    /// Data reconstructed from the frames applied to the session.
    pub data: Value,
    /// Statistics as of the moment the session was closed.
    pub stats: StreamStats,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Priority;
    use core::assert_matches;
    use serde_json::json;

    #[tokio::test]
    async fn test_client_creation() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        assert_eq!(client.url.as_str(), "ws://localhost:3001/ws");
        assert_eq!(client.write_timeout, DEFAULT_WRITE_TIMEOUT);
    }

    #[tokio::test]
    async fn test_with_write_timeout_overrides_default() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
            .unwrap()
            .with_write_timeout(Duration::from_secs(3));
        assert_eq!(client.write_timeout, Duration::from_secs(3));
    }

    #[tokio::test]
    async fn test_stream_session() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let data = json!({"test": "data"});

        let session_id = client
            .request_stream(data, StreamOptions::patch())
            .await
            .unwrap();
        let sessions = client.sessions.read().await;
        assert!(sessions.contains_key(&session_id));
    }

    #[tokio::test]
    async fn test_message_channel_is_bounded() {
        // Regression test for #314: the client's outgoing message channel
        // used to be unbounded, so a stalled `send_task` let it grow
        // without limit. It must now reject sends once
        // `MESSAGE_QUEUE_CAPACITY` is reached.
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let tx = client.message_tx.clone();
        let pong = "pong".to_string();

        for _ in 0..MESSAGE_QUEUE_CAPACITY {
            tx.try_send(pong.clone(), pong.len())
                .expect("channel should accept sends up to its capacity");
        }

        let result = tx.try_send(pong.clone(), pong.len());
        assert_matches!(
            result,
            Err(
                crate::infrastructure::bounded_channel::TrySendError::Channel(
                    mpsc::error::TrySendError::Full(_)
                )
            ),
            "channel must reject sends past capacity instead of growing unbounded"
        );
    }

    #[tokio::test]
    async fn test_message_channel_rejects_when_byte_budget_exceeded() {
        // Regression test for #349: a message-count bound alone doesn't
        // bound queued bytes. A single message larger than
        // `MAX_QUEUED_MESSAGE_BYTES` must be rejected even though the
        // channel is nowhere near its message-count capacity.
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let tx = client.message_tx.clone();
        let oversized = "x".repeat(MAX_QUEUED_MESSAGE_BYTES + 1);
        let len = oversized.len();

        assert_matches!(
            tx.try_send(oversized, len),
            Err(crate::infrastructure::bounded_channel::TrySendError::BudgetExceeded(_)),
            "a single over-budget message must be rejected"
        );
    }

    #[test]
    fn test_client_config_default_matches_server_inbound_cap() {
        let config = WebSocketClientConfig::default();
        let server_cap = crate::security::RateLimitConfig::default().max_frame_size;
        assert_eq!(config.max_message_size, server_cap);
        assert_eq!(config.max_frame_size, server_cap);
    }

    #[test]
    fn test_client_config_maps_to_transport_limits() {
        let config = WebSocketClientConfig {
            max_message_size: 100,
            max_frame_size: 50,
            ..WebSocketClientConfig::default()
        };
        let ws = config.websocket_config();
        assert_eq!(ws.max_message_size, Some(100));
        assert_eq!(ws.max_frame_size, Some(50));
    }

    #[tokio::test]
    async fn test_with_config_overrides_default() {
        let config = WebSocketClientConfig {
            max_sessions: NonZeroUsize::new(3).unwrap(),
            ..WebSocketClientConfig::default()
        };
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
            .unwrap()
            .with_config(config);
        assert_eq!(client.config, config);
    }

    fn session_idle_since(last_activity: Instant, is_complete: bool) -> ClientStreamSession {
        let mut session = ClientStreamSession::new(SessionId::new(), Framing::Merge);
        session.last_activity = last_activity;
        session.is_complete = is_complete;
        session
    }

    fn max(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).unwrap()
    }

    fn sessions_of(
        entries: impl IntoIterator<Item = ClientStreamSession>,
    ) -> HashMap<SessionId, ClientStreamSession> {
        entries
            .into_iter()
            .map(|session| (session.id, session))
            .collect()
    }

    #[test]
    fn test_session_idle_ttl_rejects_zero() {
        assert_matches!(
            SessionIdleTtl::new(Duration::ZERO),
            Err(ConfigError::MustBePositive { .. })
        );
        assert_eq!(
            SessionIdleTtl::new(Duration::from_secs(7))
                .unwrap()
                .as_duration(),
            Duration::from_secs(7)
        );
    }

    #[test]
    fn test_is_expired_uses_injected_now_and_strict_comparison() {
        let ttl = SessionIdleTtl::new(Duration::from_secs(10)).unwrap();
        let base = Instant::now();
        let session = session_idle_since(base, false);

        assert!(!session.is_expired(base, ttl));
        assert!(!session.is_expired(base + Duration::from_secs(10), ttl));
        assert!(session.is_expired(base + Duration::from_secs(11), ttl));
    }

    #[test]
    fn test_reserve_session_slot_evicts_incomplete_session_idle_longest() {
        let ttl = SessionIdleTtl::default();
        let base = Instant::now();
        let idle_longest = session_idle_since(base, false);
        let recent = session_idle_since(base + Duration::from_secs(10), false);
        let (idle_id, recent_id) = (idle_longest.id, recent.id);
        let mut sessions = sessions_of([idle_longest, recent]);

        reserve_session_slot(&mut sessions, max(2), ttl, base + Duration::from_secs(11)).unwrap();

        assert!(!sessions.contains_key(&idle_id));
        assert!(sessions.contains_key(&recent_id));
    }

    #[test]
    fn test_reserve_session_slot_never_evicts_completed_sessions() {
        let ttl = SessionIdleTtl::default();
        let base = Instant::now();
        let done = session_idle_since(base, true);
        let live = session_idle_since(base + Duration::from_secs(10), false);
        let (done_id, live_id) = (done.id, live.id);
        let mut sessions = sessions_of([done, live]);

        reserve_session_slot(&mut sessions, max(2), ttl, base + Duration::from_secs(11)).unwrap();

        assert!(sessions.contains_key(&done_id));
        assert!(!sessions.contains_key(&live_id));
    }

    #[test]
    fn test_reserve_session_slot_errors_when_every_session_is_complete() {
        let ttl = SessionIdleTtl::default();
        let base = Instant::now();
        let mut sessions = sessions_of([
            session_idle_since(base, true),
            session_idle_since(base, true),
        ]);

        let result = reserve_session_slot(&mut sessions, max(2), ttl, base);

        assert_matches!(
            result,
            Err(PjsError::SessionLimitReached { limit }) if limit == max(2)
        );
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_reserve_session_slot_drops_expired_sessions_first() {
        let ttl = SessionIdleTtl::new(Duration::from_secs(10)).unwrap();
        let base = Instant::now();
        let expired_done = session_idle_since(base, true);
        let fresh = session_idle_since(base + Duration::from_secs(20), false);
        let fresh_id = fresh.id;
        let mut sessions = sessions_of([expired_done, fresh]);

        reserve_session_slot(&mut sessions, max(2), ttl, base + Duration::from_secs(25)).unwrap();

        assert_eq!(sessions.len(), 1);
        assert!(sessions.contains_key(&fresh_id));
    }

    #[test]
    fn test_reserve_session_slot_is_noop_below_limit() {
        let base = Instant::now();
        let mut sessions = sessions_of([session_idle_since(base, false)]);

        reserve_session_slot(&mut sessions, max(2), SessionIdleTtl::default(), base).unwrap();

        assert_eq!(sessions.len(), 1);
    }

    #[tokio::test]
    async fn test_request_stream_at_capacity_evicts_idle_longest_instead_of_failing() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
            .unwrap()
            .with_config(WebSocketClientConfig {
                max_sessions: max(2),
                ..WebSocketClientConfig::default()
            });

        let first = client
            .request_stream(json!({}), StreamOptions::patch())
            .await
            .unwrap();
        let second = client
            .request_stream(json!({}), StreamOptions::patch())
            .await
            .unwrap();
        client
            .sessions
            .write()
            .await
            .get_mut(&second)
            .unwrap()
            .last_activity += Duration::from_secs(1);
        let third = client
            .request_stream(json!({}), StreamOptions::patch())
            .await
            .unwrap();

        let sessions = client.sessions.read().await;
        assert_eq!(sessions.len(), 2);
        assert!(!sessions.contains_key(&first));
        assert!(sessions.contains_key(&second));
        assert!(sessions.contains_key(&third));
    }

    #[tokio::test]
    async fn test_request_stream_failed_send_does_not_leak_session() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        drop(client.message_rx.lock().take());

        assert!(
            client
                .request_stream(json!({}), StreamOptions::patch())
                .await
                .is_err()
        );
        assert!(client.sessions.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_request_stream_cancelled_while_send_pending_does_not_leak_session() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let filler = "x".to_string();
        for _ in 0..MESSAGE_QUEUE_CAPACITY {
            client
                .message_tx
                .try_send(filler.clone(), filler.len())
                .expect("channel should accept sends up to its capacity");
        }

        let pending = tokio::time::timeout(
            Duration::from_millis(50),
            client.request_stream(json!({}), StreamOptions::patch()),
        )
        .await;

        assert!(pending.is_err(), "send must still be waiting for capacity");
        assert!(client.sessions.read().await.is_empty());
    }

    fn frame(session_id: SessionId, frame_id: u32, payload: Value) -> WsMessage {
        WsMessage::StreamFrame {
            session_id,
            frame_id,
            priority: Priority::LOW,
            payload,
            is_complete: false,
        }
    }

    async fn sessions_with(id: SessionId) -> ClientSessions {
        let sessions = Arc::new(RwLock::new(HashMap::new()));
        sessions
            .write()
            .await
            .insert(id, ClientStreamSession::new(id, Framing::Merge));
        sessions
    }

    fn control_channel() -> (ByteBoundedSender<String>, mpsc::Receiver<Envelope<String>>) {
        byte_bounded_channel(MESSAGE_QUEUE_CAPACITY, MAX_QUEUED_MESSAGE_BYTES)
    }

    async fn deliver(
        sessions: &ClientSessions,
        tx: &ByteBoundedSender<String>,
        config: WebSocketClientConfig,
        message: WsMessage,
    ) -> PjsResult<()> {
        PjsWebSocketClient::handle_incoming_message(sessions.clone(), tx.clone(), config, message)
            .await
    }

    fn complete(session_id: SessionId) -> WsMessage {
        WsMessage::StreamComplete { session_id }
    }

    #[tokio::test]
    async fn test_frame_state_is_bounded_by_counters() {
        let id = SessionId::new();
        let sessions = sessions_with(id).await;
        let (tx, _rx) = control_channel();

        for frame_id in 0..100u32 {
            deliver(
                &sessions,
                &tx,
                WebSocketClientConfig::default(),
                frame(id, frame_id, json!({"k": frame_id})),
            )
            .await
            .unwrap();
        }

        let session = &sessions.read().await[&id];
        assert_eq!(session.frames_received, 100);
    }

    #[tokio::test]
    async fn test_session_exceeding_byte_cap_is_rejected_and_dropped() {
        let id = SessionId::new();
        let sessions = sessions_with(id).await;
        let (tx, _rx) = control_channel();
        let config = WebSocketClientConfig {
            max_session_bytes: NonZeroUsize::new(64).unwrap(),
            ..WebSocketClientConfig::default()
        };

        deliver(&sessions, &tx, config, frame(id, 0, json!({"a": "small"})))
            .await
            .unwrap();
        assert!(sessions.read().await.contains_key(&id));

        let result = deliver(
            &sessions,
            &tx,
            config,
            frame(id, 1, json!({"b": "x".repeat(100)})),
        )
        .await;

        assert_matches!(
            result,
            Err(PjsError::LimitExceeded {
                kind: LimitKind::SessionBytes,
                ..
            })
        );
        assert!(!sessions.read().await.contains_key(&id));
    }

    #[test]
    fn test_apply_frame_overwriting_a_key_does_not_grow_byte_count() {
        let mut session = ClientStreamSession::new(SessionId::new(), Framing::Merge);
        session.apply_frame(json!({"k": "aaaa"}), 1024).unwrap();
        let after_first = session.reconstructed_bytes;

        session.apply_frame(json!({"k": "bbbb"}), 1024).unwrap();

        assert_eq!(session.reconstructed_bytes, after_first);
        assert!(session.reconstructed_bytes >= json_len(&session.reconstructed_data));
    }

    fn patch(value: Value) -> WirePatch {
        serde_json::from_value(value).expect("valid wire patch")
    }

    #[test]
    fn test_apply_patches_tracked_bytes_bound_the_real_size() {
        let mut session = ClientStreamSession::new(SessionId::new(), Framing::Merge);
        let patches = [
            json!(["merge", [], {"a": "xx", "list": [1], "n": {"k": 1}}]),
            json!(["append", ["list"], [2, 3]]),
            json!(["set", ["n", "k"], "longer value"]),
            json!(["set", ["list", 0], 1000]),
            json!(["merge", ["n"], {"k": 7, "z": null}]),
            json!(["append_str", ["a"], "\"quoted\""]),
        ]
        .map(patch)
        .to_vec();

        session.apply_patches(patches, 1 << 20).unwrap();

        assert_eq!(
            session.reconstructed_data,
            json!({"a": "xx\"quoted\"", "list": [1000, 2, 3], "n": {"k": 7, "z": null}})
        );
        assert!(session.reconstructed_bytes >= json_len(&session.reconstructed_data));
    }

    #[test]
    fn test_apply_patches_refuses_patch_that_would_exceed_cap_without_applying_it() {
        let mut session = ClientStreamSession::new(SessionId::new(), Framing::Merge);
        let small = patch(json!(["merge", [], {"k": "v"}]));
        let big = patch(json!(["append_str", ["k"], "y".repeat(100)]));
        session.apply_patches(vec![small], 64).unwrap();

        let result = session.apply_patches(vec![big], 64);

        assert_matches!(result, Err(PjsError::LimitExceeded { .. }), "{result:?}");
        assert_eq!(session.reconstructed_data, json!({"k": "v"}));
    }

    #[test]
    fn test_apply_frame_merges_objects() {
        let mut session = ClientStreamSession::new(SessionId::new(), Framing::Merge);
        session
            .apply_frame(json!({"existing": "value"}), 1024)
            .unwrap();
        session
            .apply_frame(json!({"new": "data", "existing": "updated"}), 1024)
            .unwrap();

        assert_eq!(session.reconstructed_data["existing"], "updated");
        assert_eq!(session.reconstructed_data["new"], "data");
    }

    #[tokio::test]
    async fn test_full_cap_of_completed_sessions_returns_typed_error_and_close_frees_a_slot() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
            .unwrap()
            .with_config(WebSocketClientConfig {
                max_sessions: max(2),
                ..WebSocketClientConfig::default()
            });
        let first = client
            .request_stream(json!({}), StreamOptions::patch())
            .await
            .unwrap();
        let second = client
            .request_stream(json!({}), StreamOptions::patch())
            .await
            .unwrap();
        let (tx, _rx) = control_channel();
        for id in [first, second] {
            deliver(&client.sessions, &tx, client.config, complete(id))
                .await
                .unwrap();
        }

        let refused = client
            .request_stream(json!({}), StreamOptions::patch())
            .await;
        assert_matches!(refused, Err(PjsError::SessionLimitReached { .. }));
        assert!(client.is_stream_complete(first).await);

        assert!(client.close_session(first).await.is_some());
        let fresh = client
            .request_stream(json!({}), StreamOptions::patch())
            .await
            .unwrap();
        let sessions = client.sessions.read().await;
        assert!(sessions.contains_key(&second));
        assert!(sessions.contains_key(&fresh));
    }

    #[tokio::test]
    async fn test_completed_session_ignores_late_frames_and_repeated_completion() {
        let id = SessionId::new();
        let sessions = sessions_with(id).await;
        let (tx, mut rx) = control_channel();
        let config = WebSocketClientConfig::default();
        deliver(&sessions, &tx, config, frame(id, 0, json!({"a": 1})))
            .await
            .unwrap();
        deliver(&sessions, &tx, config, complete(id)).await.unwrap();
        rx.try_recv().expect("the live frame is acknowledged");
        let (frozen_activity, frozen_frames) = {
            let guard = sessions.read().await;
            (guard[&id].last_activity, guard[&id].frames_received)
        };

        deliver(&sessions, &tx, config, frame(id, 1, json!({"late": true})))
            .await
            .unwrap();
        deliver(&sessions, &tx, config, complete(id)).await.unwrap();

        let guard = sessions.read().await;
        assert_eq!(guard[&id].last_activity, frozen_activity);
        assert_eq!(guard[&id].frames_received, frozen_frames);
        assert_eq!(guard[&id].reconstructed_data, json!({"a": 1}));
        assert!(rx.try_recv().is_err(), "a late frame must not be acked");
    }

    #[tokio::test]
    async fn test_frame_for_unknown_session_is_not_acked() {
        let sessions: ClientSessions = Arc::new(RwLock::new(HashMap::new()));
        let (tx, mut rx) = control_channel();

        deliver(
            &sessions,
            &tx,
            WebSocketClientConfig::default(),
            frame(SessionId::new(), 0, json!({})),
        )
        .await
        .unwrap();

        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_expired_session_reads_as_unknown_and_is_dropped_on_incoming_path() {
        let id = SessionId::new();
        let sessions = sessions_with(id).await;
        let config = WebSocketClientConfig::default();
        let ttl = config.session_idle_ttl;
        let later = Instant::now() + ttl.as_duration() + Duration::from_secs(1);

        {
            let guard = sessions.read().await;
            assert!(live_session(&guard, &id, ttl, Instant::now()).is_some());
            assert!(live_session(&guard, &id, ttl, later).is_none());
        }

        PjsWebSocketClient::drop_if_expired(&mut *sessions.write().await, &id, config, later);
        assert!(sessions.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_close_session_returns_data_and_stats_or_none() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let id = client
            .request_stream(json!({}), StreamOptions::default())
            .await
            .unwrap();
        let (tx, _rx) = control_channel();
        deliver(
            &client.sessions,
            &tx,
            client.config,
            frame(id, 0, json!({"a": 1})),
        )
        .await
        .unwrap();

        let closed = client.close_session(id).await.expect("known session");

        assert_eq!(closed.data, json!({"a": 1}));
        assert_eq!(closed.stats.session_id, id);
        assert_eq!(closed.stats.total_frames, 1);
        assert!(client.close_session(id).await.is_none());
        assert!(client.close_session(SessionId::new()).await.is_none());
    }

    async fn queued_messages(client: &PjsWebSocketClient) -> Vec<WsMessage> {
        let mut outbox = client.message_rx.lock().take().expect("outbox present");
        let mut messages = Vec::new();
        while let Some(envelope) = outbox
            .pending
            .take()
            .or_else(|| outbox.receiver.try_recv().ok())
        {
            messages.push(serde_json::from_str(&envelope).expect("valid message"));
        }
        *client.message_rx.lock() = Some(outbox);
        messages
    }

    #[tokio::test]
    async fn test_close_session_queues_stream_cancel_for_incomplete_session() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let id = client
            .request_stream(json!({}), StreamOptions::patch())
            .await
            .unwrap();

        client.close_session(id).await.expect("known session");

        let messages = queued_messages(&client).await;
        assert_matches!(messages[0], WsMessage::StreamInit { .. });
        assert_matches!(
            messages.as_slice(),
            [_, WsMessage::StreamCancel { session_id }] if *session_id == id
        );
    }

    #[tokio::test]
    async fn test_close_session_of_expired_session_still_queues_stream_cancel() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let id = client
            .request_stream(json!({}), StreamOptions::patch())
            .await
            .unwrap();
        let later =
            Instant::now() + client.config.session_idle_ttl.as_duration() + Duration::from_secs(1);

        assert!(client.close_session_at(id, later).await.is_none());

        assert_matches!(
            queued_messages(&client).await.as_slice(),
            [_, WsMessage::StreamCancel { session_id }] if *session_id == id
        );
    }

    #[tokio::test]
    async fn test_close_session_of_complete_session_sends_no_cancel() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let id = client
            .request_stream(json!({}), StreamOptions::patch())
            .await
            .unwrap();
        client
            .sessions
            .write()
            .await
            .get_mut(&id)
            .unwrap()
            .is_complete = true;

        client.close_session(id).await.expect("known session");

        assert_eq!(queued_messages(&client).await.len(), 1);
    }

    #[tokio::test]
    async fn test_close_session_of_unknown_session_sends_no_cancel() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();

        assert!(client.close_session(SessionId::new()).await.is_none());

        assert!(queued_messages(&client).await.is_empty());
    }

    #[tokio::test]
    async fn test_close_session_of_expired_session_is_none_and_frees_the_slot() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let id = client
            .request_stream(json!({}), StreamOptions::patch())
            .await
            .unwrap();
        let ttl = client.config.session_idle_ttl.as_duration();
        let later = Instant::now() + ttl + Duration::from_secs(1);

        assert!(client.close_session_at(id, later).await.is_none());
        assert!(client.sessions.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_server_error_with_session_id_removes_that_session() {
        let id = SessionId::new();
        let other = SessionId::new();
        let sessions = sessions_with(id).await;
        sessions
            .write()
            .await
            .insert(other, ClientStreamSession::new(other, Framing::Merge));
        let (tx, _rx) = control_channel();

        deliver(
            &sessions,
            &tx,
            WebSocketClientConfig::default(),
            WsMessage::Error {
                session_id: Some(id),
                error: "rejected".to_string(),
                code: 400,
            },
        )
        .await
        .unwrap();

        let guard = sessions.read().await;
        assert!(!guard.contains_key(&id));
        assert!(guard.contains_key(&other));
    }

    #[tokio::test]
    async fn test_server_conflict_error_keeps_the_live_session() {
        let id = SessionId::new();
        let sessions = sessions_with(id).await;
        let (tx, _rx) = control_channel();

        deliver(
            &sessions,
            &tx,
            WebSocketClientConfig::default(),
            WsMessage::Error {
                session_id: Some(id),
                error: ServerReply::DuplicateSessionId.reason().to_string(),
                code: ServerReply::DuplicateSessionId.code(),
            },
        )
        .await
        .unwrap();

        assert!(sessions.read().await.contains_key(&id));
    }

    #[tokio::test]
    async fn test_send_loop_reports_write_failure_as_connection_failed() {
        let (tx, rx) = byte_bounded_channel(4, 1024);
        let mut outbox = Outbox::new(rx);
        tx.send("hello".to_string(), 5).await.unwrap();
        let failing_sink = Box::pin(futures::sink::unfold((), |(), _message: Message| async {
            Err::<(), WsError>(WsError::ConnectionClosed)
        }));

        let result = send_loop(failing_sink, &mut outbox, Duration::from_secs(1)).await;

        assert_matches!(result, Err(PjsError::Transport(_)));
    }

    #[tokio::test]
    async fn test_send_loop_failed_write_keeps_message_for_next_connection() {
        let (tx, rx) = byte_bounded_channel(4, 1024);
        let mut outbox = Outbox::new(rx);
        tx.send("hello".to_string(), 5).await.unwrap();
        let failing_sink = Box::pin(futures::sink::unfold((), |(), _message: Message| async {
            Err::<(), WsError>(WsError::ConnectionClosed)
        }));
        assert!(
            send_loop(failing_sink, &mut outbox, Duration::from_secs(1))
                .await
                .is_err()
        );
        drop(tx);

        let written = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recording_sink = Box::pin(futures::sink::unfold(
            Arc::clone(&written),
            |written, message: Message| async move {
                written.lock().unwrap().push(message);
                Ok::<_, WsError>(written)
            },
        ));
        send_loop(recording_sink, &mut outbox, Duration::from_secs(1))
            .await
            .unwrap();

        assert_eq!(*written.lock().unwrap(), vec![Message::text("hello")]);
    }

    #[tokio::test]
    async fn test_send_loop_cancelled_write_keeps_message_for_next_connection() {
        let (tx, rx) = byte_bounded_channel(4, 1024);
        let mut outbox = Outbox::new(rx);
        tx.send("hello".to_string(), 5).await.unwrap();
        let stalled_sink = Box::pin(futures::sink::unfold((), |(), _message: Message| {
            std::future::pending::<Result<(), WsError>>()
        }));
        let cancelled = tokio::time::timeout(
            Duration::from_millis(50),
            send_loop(stalled_sink, &mut outbox, Duration::from_secs(60)),
        )
        .await;
        assert!(cancelled.is_err(), "send half is cancelled mid-write");

        assert_eq!(outbox.pending.as_deref().map(String::as_str), Some("hello"));
    }

    #[test]
    fn test_receiver_lease_is_exclusive_and_returns_receiver_on_drop() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();

        let lease = ReceiverLease::acquire(&client.message_rx).unwrap();
        assert_matches!(
            ReceiverLease::acquire(&client.message_rx).err(),
            Some(PjsError::AlreadyConnected)
        );
        drop(lease);
        assert!(ReceiverLease::acquire(&client.message_rx).is_ok());
    }

    #[derive(Clone, Default)]
    struct CapturedLogs(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for CapturedLogs {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLogs {
        type Writer = CapturedLogs;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[tokio::test]
    async fn test_server_supplied_strings_cannot_forge_log_lines() {
        let logs = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(logs.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let hostile = "\n\r\x1b[31m".to_string();
        let id = SessionId::new();
        let sessions = sessions_with(id).await;
        let (tx, _rx) = control_channel();
        let config = WebSocketClientConfig::default();

        for message in [
            WsMessage::StreamComplete { session_id: id },
            WsMessage::Error {
                session_id: Some(id),
                error: hostile.clone(),
                code: 500,
            },
            frame(id, 0, json!({})),
        ] {
            deliver(&sessions, &tx, config, message).await.unwrap();
        }

        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(
            !output.contains('\r'),
            "raw CR leaked into logs: {output:?}"
        );
        assert!(
            !output.contains('\x1b'),
            "raw ESC leaked into logs: {output:?}"
        );
        assert_eq!(
            output.lines().count(),
            3,
            "a hostile string started its own log line: {output:?}"
        );
    }

    #[test]
    fn test_bounded_log_text_truncates_and_escapes() {
        let long = "a".repeat(MAX_LOGGED_TEXT_CHARS * 100);
        let out = bounded_log_text(&long);
        assert!(
            out.len() < MAX_LOGGED_TEXT_CHARS + 64,
            "unbounded: {}",
            out.len()
        );
        assert!(out.contains("bytes total"));
        assert_eq!(bounded_log_text("a\nb"), "\"a\\nb\"");
        let multibyte = "\u{44f}".repeat(MAX_LOGGED_TEXT_CHARS + 10);
        assert!(bounded_log_text(&multibyte).contains("bytes total"));
    }

    #[test]
    fn test_redacted_url_strips_userinfo_query_and_fragment() {
        let url = Url::parse("wss://user:s3cret@example.com:9000/ws?token=abc#frag").unwrap();
        assert_eq!(redacted_url(&url), "wss://example.com:9000/ws");
    }

    #[tokio::test]
    async fn test_server_controlled_content_is_bounded_in_logs() {
        let logs = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(logs.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let id = SessionId::new();
        let sessions = sessions_with(id).await;
        let (tx, _rx) = control_channel();
        let config = WebSocketClientConfig::default();
        let marker = "X".repeat(100_000);

        deliver(
            &sessions,
            &tx,
            config,
            WsMessage::Error {
                session_id: Some(id),
                error: marker.clone(),
                code: 500,
            },
        )
        .await
        .unwrap();
        deliver(
            &sessions,
            &tx,
            config,
            WsMessage::StreamInit {
                session_id: id,
                data: json!({ "blob": marker }),
                options: StreamOptions::default(),
            },
        )
        .await
        .unwrap();
        let bad = format!(r#"{{"type":"{marker}"}}"#);
        let stream = futures::stream::iter([Ok::<_, WsError>(Message::text(bad))]);
        receive_loop(stream, sessions, tx, config).await.unwrap();

        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("StreamInit"), "{output}");
        assert!(output.contains("category="), "{output}");
        assert!(
            output.len() < 4096,
            "log output unbounded: {}",
            output.len()
        );
    }
}
