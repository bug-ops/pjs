//! Acceptance tests for #745 (track D): protocol-level `Ping` and `Pong`
//! frames are charged to the per-connection message budget, so a control-frame
//! burst closes the connection with 1008 like a text burst does.
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

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const BURST: usize = 60;

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

async fn connect(addr: SocketAddr) -> Socket {
    connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("handshake")
        .0
}

/// Sends `BURST` control frames back to back, ignoring send errors after the
/// server closed, then drains until the connection ends.
/// Returns the close code (if any) and the number of pongs received.
async fn burst_and_drain(ws: &mut Socket, frame: impl Fn() -> Message) -> (Option<u16>, usize) {
    for _ in 0..BURST {
        if ws.send(frame()).await.is_err() {
            break;
        }
    }
    let mut pongs = 0;
    let mut close_code = None;
    let drained = timeout(Duration::from_secs(5), async {
        while let Some(message) = ws.next().await {
            match message {
                Ok(Message::Pong(_)) => pongs += 1,
                Ok(Message::Close(frame)) => {
                    close_code = frame.map(|f| u16::from(f.code));
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    })
    .await;
    assert!(drained.is_ok(), "the server must end the connection");
    (close_code, pongs)
}

#[tokio::test]
async fn a_burst_of_pings_closes_the_connection_with_policy_violation() {
    let addr = spawn_server().await;
    let mut ws = connect(addr).await;

    let (close_code, pongs) =
        burst_and_drain(&mut ws, || Message::Ping(b"p".to_vec().into())).await;

    assert_eq!(close_code, Some(1008));
    assert!(pongs < BURST, "all {pongs} pings were answered");
}

#[tokio::test]
async fn a_burst_of_unsolicited_pongs_closes_the_connection_with_policy_violation() {
    let addr = spawn_server().await;
    let mut ws = connect(addr).await;

    let (close_code, _) = burst_and_drain(&mut ws, || Message::Pong(b"p".to_vec().into())).await;

    assert_eq!(close_code, Some(1008));
}

#[tokio::test]
async fn pings_within_the_burst_allowance_are_still_answered() {
    let addr = spawn_server().await;
    let mut ws = connect(addr).await;

    for index in 0..3u8 {
        ws.send(Message::Ping(vec![index].into()))
            .await
            .expect("send ping");
        let reply = timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("a reply")
            .expect("open")
            .expect("ok");
        assert_eq!(reply, Message::Pong(vec![index].into()));
    }
}
