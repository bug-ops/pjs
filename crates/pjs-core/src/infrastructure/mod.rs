//! Infrastructure layer - External concerns and adapters
//!
//! Implements infrastructure adapters for databases, HTTP servers,
//! message queues, WebSocket transport, and other external systems.

pub mod adapters;
pub mod bounded_channel;
#[cfg(feature = "http-server")]
pub mod http;
pub mod repositories;
pub mod schema_repository;
#[cfg(feature = "http-server")]
pub mod websocket;

pub use adapters::{
    GatInMemoryStreamRepository, GatInMemoryStreamStore, InMemoryEventPublisher,
    InMemoryFrameStore, InMemoryMetricsCollector, InMemoryStore, MAX_HEALTH_METRICS,
    MAX_PAGINATION_LIMIT, MAX_PAGINATION_OFFSET, MAX_RESULTS_LIMIT, MAX_SCAN_LIMIT,
    PerformanceMetrics, SessionMetrics, SessionStore, StoredEvent, StreamMetrics, StreamStore,
    TimestampedMetrics,
};
pub use bounded_channel::{
    ByteBoundedSender, Envelope, SendError, TrySendError, byte_bounded_channel,
};
#[cfg(feature = "http-server")]
pub use http::{
    BatchFrameStream, ConnectionLimits, CreateSessionRequest, CreateSessionResponse,
    HttpExtensionConfig, HttpServerConfig, PjsAppState, PjsError, PjsExtension, RateLimitConfig,
    RateLimitMiddleware, StartStreamRequest, StreamFormat, StreamParams, StreamTransportError,
    TrustedProxyConfig, create_pjs_router, create_pjs_router_with_auth,
    create_pjs_router_with_config, create_pjs_router_with_rate_limit,
    create_pjs_router_with_rate_limit_and_auth, create_pjs_router_with_rate_limit_and_config,
    create_streaming_response, create_streaming_response_with_content_type, serve_with_limits,
};
pub use schema_repository::SchemaRepository;
#[cfg(feature = "http-server")]
pub use websocket::{
    AdaptiveStreamController, AxumWebSocketTransport, ClientMetrics, SecureWebSocketHandler,
    StreamOptions, WebSocketStreamSession, WebSocketTransport, WsMessage, create_websocket_router,
};
#[cfg(all(feature = "http-server", feature = "websocket-client"))]
pub use websocket::{PjsWebSocketClient, StreamStats};
