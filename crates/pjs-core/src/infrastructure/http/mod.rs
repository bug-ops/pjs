//! HTTP transport implementations

#[cfg(feature = "http-server")]
pub mod auth;
pub mod axum_adapter;
pub mod axum_extension;
pub mod handlers;
#[cfg(feature = "metrics")]
pub mod metrics;
pub mod middleware;
pub mod streaming;

pub use axum_adapter::{
    CreateSessionRequest, CreateSessionResponse, HttpServerConfig, PjsAppState, PjsError,
    StartStreamRequest, StreamParams, create_pjs_router, create_pjs_router_with_config,
    create_pjs_router_with_rate_limit, create_pjs_router_with_rate_limit_and_config,
};
#[cfg(feature = "http-server")]
pub use axum_adapter::{create_pjs_router_with_auth, create_pjs_router_with_rate_limit_and_auth};
pub use axum_extension::{HttpExtensionConfig, PjsExtension};
pub use middleware::{RateLimitConfig, RateLimitMiddleware, TrustedProxyConfig};
pub use streaming::{
    BatchFrameStream, StreamFormat, StreamTransportError, create_streaming_response,
    create_streaming_response_with_content_type,
};
