//! WebSocket transport layer for real-time PJS streaming
//!
//! Provides WebSocket-based streaming with progressive JSON delivery
//! and backpressure handling for optimal client performance.
//!
//! The wire protocol (`protocol`) is shared; the client (`websocket-client`)
//! and the server side (`websocket-server`) are separate feature-gated modules.

#[cfg(feature = "websocket-client")]
pub mod client;
#[cfg(feature = "websocket-server")]
pub mod controller;
mod protocol;
#[cfg(feature = "websocket-server")]
pub mod security;
#[cfg(feature = "websocket-server")]
pub mod server;

#[cfg(feature = "websocket-client")]
pub use client::{
    ClosedSession, PjsWebSocketClient, SessionIdleTtl, StreamStats, WebSocketClientConfig,
};
#[cfg(feature = "websocket-server")]
pub use controller::{
    AdaptiveStreamController, ClientMetrics, ConnectionId, CreateSessionError, FrameEvent,
    WebSocketStreamSession,
};
#[cfg(feature = "websocket-client")]
pub(crate) use protocol::WRITE_TIMEOUT;
pub(crate) use protocol::send_with_write_timeout;
pub use protocol::{StreamOptions, WsMessage};
#[cfg(feature = "websocket-server")]
pub use security::SecureWebSocketHandler;
#[cfg(feature = "websocket-server")]
pub use server::{AxumWebSocketTransport, WebSocketTransport, create_websocket_router};
