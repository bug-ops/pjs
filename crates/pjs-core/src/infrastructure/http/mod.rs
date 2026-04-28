//! HTTP transport implementations

#[cfg(feature = "http-server")]
pub mod auth;
pub mod axum_adapter;
pub mod axum_extension;
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
pub use middleware::{RateLimitConfig, RateLimitMiddleware};
pub use streaming::{
    AdaptiveFrameStream, BatchFrameStream, PriorityFrameStream, StreamError, StreamFormat,
};
