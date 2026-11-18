//! Acceptance test for #745 (track D, coverage gap): control frames draw on
//! the same per-connection message budget as text frames. With the default
//! budget (burst of 5) three texts plus three pings exceed it, although
//! neither kind alone would.
#![cfg(all(feature = "websocket-server", feature = "websocket-client"))]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use pjson_rs::infrastructure::websocket::{
    AxumWebSocketTransport, server::create_websocket_router,
};

async fn spawn_server() -> SocketAddr {
    let app = create_websocket_router().with_state(Arc::new(AxumWebSocketTransport::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    addr
}

async fn close_code_after(messages: Vec<Message>, wait: Duration) -> Option<u16> {
    let addr = spawn_server().await;
    let (mut ws, _) = connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("handshake");
    for message in messages {
        if ws.send(message).await.is_err() {
            break;
        }
    }
    timeout(wait, async {
        while let Some(message) = ws.next().await {
            match message {
                Ok(Message::Close(frame)) => return frame.map(|f| u16::from(f.code)),
                Ok(_) => {}
                Err(_) => return None,
            }
        }
        None
    })
    .await
    .unwrap_or(None)
}

fn text() -> Message {
    Message::Text("{}".into())
}

fn ping() -> Message {
    Message::Ping(b"p".to_vec().into())
}

#[tokio::test]
async fn texts_and_pings_share_one_budget() {
    let mixed = vec![text(), text(), text(), ping(), ping(), ping()];

    assert_eq!(
        close_code_after(mixed, Duration::from_secs(5)).await,
        Some(1008)
    );
}

#[tokio::test]
async fn the_same_texts_or_pings_alone_stay_within_the_budget() {
    // Guards the mixed case: three of one kind are under the burst of five.
    // The server answers and keeps the connection open, so the drain times out.
    assert_eq!(
        close_code_after(vec![text(), text(), text()], Duration::from_secs(1)).await,
        None
    );
    assert_eq!(
        close_code_after(vec![ping(), ping(), ping()], Duration::from_secs(1)).await,
        None
    );
}
