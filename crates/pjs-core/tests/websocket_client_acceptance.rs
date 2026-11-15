//! Acceptance tests for #597: `PjsWebSocketClient::connect` lifecycle.
#![cfg(feature = "websocket-client")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{accept_async, tungstenite::Message};

use pjson_rs::Error as PjsError;
use pjson_rs::infrastructure::websocket::PjsWebSocketClient;

/// Accepts `connections` sequential WebSocket clients; each is closed by the server after
/// `hold` (a zero `hold` closes immediately).
async fn spawn_closing_server(connections: usize, hold: Duration) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        for _ in 0..connections {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let Ok(mut ws) = accept_async(stream).await else {
                continue;
            };
            tokio::time::sleep(hold).await;
            let _ = ws.send(Message::Close(None)).await;
            while let Some(Ok(_)) = ws.next().await {}
        }
    });
    addr
}

fn client(addr: SocketAddr) -> Arc<PjsWebSocketClient> {
    Arc::new(PjsWebSocketClient::new(format!("ws://{addr}")).expect("valid url"))
}

#[tokio::test]
async fn test_connect_returns_ok_when_server_closes() {
    let client = client(spawn_closing_server(1, Duration::ZERO).await);
    let result = timeout(Duration::from_secs(5), client.connect())
        .await
        .expect("connect returns once the connection ends");
    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test]
async fn test_connect_can_be_called_again_after_connection_ended() {
    let client = client(spawn_closing_server(2, Duration::ZERO).await);
    timeout(Duration::from_secs(5), client.connect())
        .await
        .expect("first connect returns")
        .expect("first connect ok");
    let second = timeout(Duration::from_secs(5), client.connect())
        .await
        .expect("second connect returns");
    assert!(second.is_ok(), "reconnect must succeed: {second:?}");
}

#[tokio::test]
async fn test_connect_second_concurrent_call_fails_with_client_error() {
    let client = client(spawn_closing_server(1, Duration::from_millis(500)).await);
    let first = {
        let client = Arc::clone(&client);
        tokio::spawn(async move { client.connect().await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;

    let second = timeout(Duration::from_secs(3), client.connect())
        .await
        .expect("second concurrent connect must fail fast, not block");
    assert!(
        matches!(second, Err(PjsError::AlreadyConnected)),
        "{second:?}"
    );
    first.abort();
}

#[tokio::test]
async fn test_connect_unreachable_server_fails_with_connection_failed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);
    let result = client(addr).connect().await;
    assert!(
        matches!(result, Err(PjsError::ConnectionFailed { .. })),
        "{result:?}"
    );
}
