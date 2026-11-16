//! WebSocket transport layer for real-time PJS streaming
//!
//! Provides WebSocket-based streaming with progressive JSON delivery
//! and backpressure handling for optimal client performance.
//!
//! The wire protocol (`protocol`) is shared; the client (`websocket-client`)
//! and the server side (`websocket-server`) are separate feature-gated modules.

#[cfg(feature = "websocket-client")]
pub mod client;
#[cfg(all(test, feature = "websocket-client"))]
mod client_patch_acceptance;
#[cfg(feature = "websocket-server")]
pub mod controller;
#[cfg(all(
    any(feature = "websocket-client", feature = "websocket-server"),
    any(fuzzing, pjs_bench)
))]
#[doc(hidden)]
pub mod fuzzing;
#[cfg(feature = "websocket-server")]
mod planner;
#[cfg(all(test, feature = "websocket-server"))]
mod planner_exceeds_size_acceptance;
mod protocol;
#[cfg(feature = "websocket-server")]
pub mod security;
#[cfg(feature = "websocket-server")]
pub mod server;

#[cfg(feature = "websocket-client")]
pub use client::{
    ClosedSession, PjsWebSocketClient, SessionIdleTtl, StreamStats, WebSocketClientConfig,
};
/// The fuzzing entry points exist only under `--cfg fuzzing` or `--cfg pjs_bench`.
///
/// ```compile_fail,E0432
/// use pjson_rs::infrastructure::websocket::fuzzing::plan_messages;
/// ```
///
/// ```compile_fail,E0432
/// use pjson_rs::infrastructure::websocket::fuzzing::ClientPatchHarness;
/// ```
#[cfg(all(
    any(feature = "websocket-client", feature = "websocket-server"),
    not(any(fuzzing, pjs_bench))
))]
mod fuzzing_gate {}

#[cfg(feature = "websocket-server")]
pub use controller::{AdaptiveStreamController, ClientMetrics, ConnectionId, CreateSessionError};
#[cfg(feature = "websocket-client")]
pub(crate) use protocol::WRITE_TIMEOUT;
pub use protocol::{
    Framing, MAX_PATCH_PATH_DEPTH, StreamOptions, WirePatch, WirePath, WireSegment, WsMessage,
};
pub(crate) use protocol::{WriteFailure, send_with_write_timeout};
#[cfg(feature = "websocket-server")]
pub use security::SecureWebSocketHandler;
#[cfg(feature = "websocket-server")]
pub use server::{AxumWebSocketTransport, create_websocket_router};
