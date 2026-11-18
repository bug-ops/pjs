//! WebSocket server fixture for the `pjs-js-client` cross-stack tests.
//!
//! Serves the stock `pjs-core` WebSocket router (default origin policy, no
//! `Origin` header accepted) on an ephemeral loopback port, prints
//! `PJS_WS_FIXTURE_LISTENING <addr>` once it is bound, and exits when stdin
//! reaches end of file so the test harness can stop it by closing the pipe.
//!
//! ```text
//! cargo build -p pjson-rs --example js_client_ws_fixture
//! PJS_WS_FIXTURE_BIN=target/debug/examples/js_client_ws_fixture \
//!     npx jest --config jest.cross-stack.config.js
//! ```

use std::io::Read;
use std::net::SocketAddr;
use std::sync::Arc;

use pjson_rs::infrastructure::websocket::{
    AxumWebSocketTransport, server::create_websocket_router,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // A plain thread keeps tokio's `io-std` feature out of the dependency set.
    std::thread::spawn(|| {
        std::io::stdin().lock().bytes().for_each(drop);
        std::process::exit(0);
    });

    let app = create_websocket_router().with_state(Arc::new(AxumWebSocketTransport::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    println!("PJS_WS_FIXTURE_LISTENING {}", listener.local_addr()?);

    // Without connect info every upgrade is answered with 500.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}
