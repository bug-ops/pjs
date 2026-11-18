//! Validation tests for #702: the server's fixed `Error` reply (code, text) pairs
//! stay unchanged on the wire for the replies reachable from a client message.
#![cfg(all(feature = "http-server", feature = "websocket-server"))]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use pjson_rs::infrastructure::websocket::{
    AxumWebSocketTransport, server::create_websocket_router,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

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

fn init(session: &str, data: Value) -> Message {
    Message::Text(
        json!({"type": "StreamInit", "data": {
            "session_id": session, "data": data,
            "options": {"max_frame_size": 65536, "client_fps": null, "priority_mapping": null}
        }})
        .to_string()
        .into(),
    )
}

/// Reads until an `Error` message arrives and returns `(code, text)`.
async fn next_error(ws: &mut Socket) -> (u64, String) {
    timeout(Duration::from_secs(5), async {
        loop {
            let Message::Text(text) = ws.next().await.expect("open").expect("ok") else {
                continue;
            };
            let value: Value = serde_json::from_str(&text).expect("json");
            if value["type"] == "Error" {
                return (
                    value["data"]["code"].as_u64().expect("code"),
                    value["data"]["error"].as_str().expect("text").to_owned(),
                );
            }
        }
    })
    .await
    .expect("an Error reply")
}

#[tokio::test]
async fn malformed_nil_and_duplicate_replies_keep_their_code_and_text() {
    let addr = spawn_server().await;
    let (mut ws, _) = connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("handshake");

    ws.send(Message::Text("{\"type\":\"nope\"}".into()))
        .await
        .expect("send");
    assert_eq!(
        next_error(&mut ws).await,
        (400, "malformed message".to_owned())
    );

    ws.send(init("00000000-0000-0000-0000-000000000000", json!({})))
        .await
        .expect("send");
    assert_eq!(
        next_error(&mut ws).await,
        (400, "nil session id is not allowed".to_owned())
    );

    let session = "00000000-0000-4000-8000-000000000042";
    let big = json!({"x": "y".repeat(200_000)});
    ws.send(init(session, big.clone())).await.expect("send");
    ws.send(init(session, big)).await.expect("send");
    assert_eq!(
        next_error(&mut ws).await,
        (409, "session id already in use".to_owned())
    );
}
