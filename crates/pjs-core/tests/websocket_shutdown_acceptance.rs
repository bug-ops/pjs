//! Acceptance tests for #590 (WebSocket side of graceful shutdown).
//!
//! Assumed API: `AxumWebSocketTransport::with_shutdown(ShutdownSignal) -> Self`, sharing one
//! `ShutdownSignal` with `serve_with_limits`. The delayed-callback hand-off race is a
//! developer-owned `#[cfg(test)]` unit test in `server.rs`.
#![cfg(all(feature = "http-server", feature = "websocket-server"))]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use pjson_rs::infrastructure::http::{
    ConnectionLimits, DrainTimeout, ShutdownConfig, ShutdownOutcome, ShutdownSignal,
    serve_with_limits,
};
use pjson_rs::infrastructure::websocket::{
    AxumWebSocketTransport, server::create_websocket_router,
};

struct Running {
    addr: SocketAddr,
    signal: ShutdownSignal,
    transport: Arc<AxumWebSocketTransport>,
    serve: JoinHandle<std::io::Result<ShutdownOutcome>>,
}

async fn start() -> Running {
    let signal = ShutdownSignal::new();
    let transport = Arc::new(AxumWebSocketTransport::new().with_shutdown(signal.clone()));
    let router = create_websocket_router().with_state(transport.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let config = ShutdownConfig {
        signal: signal.clone(),
        drain_timeout: DrainTimeout::try_from(Duration::from_secs(5)).expect("valid"),
    };
    let serve = tokio::spawn(serve_with_limits(
        listener,
        router,
        ConnectionLimits::default(),
        config,
    ));
    Running {
        addr,
        signal,
        transport,
        serve,
    }
}

#[tokio::test]
async fn test_shutdown_trigger_closes_open_websocket_with_going_away() {
    let server = start().await;
    let (mut ws, _) = connect_async(format!("ws://{}/ws", server.addr))
        .await
        .expect("handshake");

    server.signal.trigger();

    let close = timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(frame))) => break frame,
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => break None,
            }
        }
    })
    .await
    .expect("close frame arrives within the write timeout");
    let frame = close.expect("server sends a Close frame");
    assert_eq!(u16::from(frame.code), 1001);
}

#[tokio::test]
async fn test_shutdown_serve_returns_drained_and_leaves_no_connections() {
    let server = start().await;
    let (mut ws, _) = connect_async(format!("ws://{}/ws", server.addr))
        .await
        .expect("handshake");

    server.signal.trigger();
    while let Ok(Some(Ok(_))) = timeout(Duration::from_secs(5), ws.next()).await {}

    let outcome = timeout(Duration::from_secs(6), server.serve)
        .await
        .expect("serve returns")
        .expect("join")
        .expect("io ok");
    assert_eq!(outcome, ShutdownOutcome::Drained);
    assert_eq!(server.transport.active_connection_count().await, 0);
}

#[tokio::test]
async fn test_shutdown_client_socket_reaches_end_of_stream_after_serve_returns() {
    let server = start().await;
    let (mut ws, _) = connect_async(format!("ws://{}/ws", server.addr))
        .await
        .expect("handshake");

    server.signal.trigger();
    let _ = timeout(Duration::from_secs(6), server.serve).await;

    let ended = timeout(Duration::from_secs(1), async {
        loop {
            match ws.next().await {
                Some(Ok(_)) => continue,
                _ => break,
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "client socket must reach EOF within 1 s");
}
