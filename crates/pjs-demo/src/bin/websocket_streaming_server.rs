//! Binary entry point for the WebSocket streaming demo server.
//!
//! The router, state, and handlers live in [`pjs_demo::servers::websocket_streaming`]
//! so integration tests can build and drive the same [`axum::Router`] directly.

use pjs_demo::servers::websocket_streaming::run;
use pjson_rs::ApplicationResult;

#[tokio::main]
async fn main() -> ApplicationResult<()> {
    tracing_subscriber::fmt::init();
    run().await
}
