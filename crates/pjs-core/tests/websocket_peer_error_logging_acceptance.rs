//! Acceptance tests for #767 (track D): errors the peer causes (an abrupt
//! disconnect without a closing handshake, invalid UTF-8 in a text frame) are
//! not logged at `ERROR`; that level is kept for server-side faults.
#![cfg(all(feature = "websocket-server", feature = "websocket-client"))]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use pjson_rs::infrastructure::websocket::{
    AxumWebSocketTransport, server::create_websocket_router,
};

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl LogBuffer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("log lock")).into_owned()
    }
}

/// Captures every level on the current thread; `#[tokio::test]` runs the
/// spawned server on that same thread.
fn capture_logs() -> (LogBuffer, tracing::subscriber::DefaultGuard) {
    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    (buffer, tracing::subscriber::set_default(subscriber))
}

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

fn error_lines(logs: &LogBuffer) -> Vec<String> {
    logs.text()
        .lines()
        .filter(|line| line.contains(" ERROR "))
        .map(str::to_owned)
        .collect()
}

/// Upgrades a raw TCP connection by hand so the test controls the frame bytes.
async fn raw_upgrade(addr: SocketAddr) -> TcpStream {
    let mut tcp = TcpStream::connect(addr).await.expect("connect");
    tcp.write_all(
        b"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
          Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
    )
    .await
    .expect("send upgrade");
    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        tcp.read_exact(&mut byte).await.expect("upgrade response");
        response.push(byte[0]);
    }
    assert!(
        String::from_utf8_lossy(&response).starts_with("HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&response)
    );
    tcp
}

#[tokio::test]
async fn an_abrupt_disconnect_without_a_closing_handshake_is_not_an_error_line() {
    let (logs, _guard) = capture_logs();
    let addr = spawn_server().await;
    let (ws, _) = connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("handshake");

    drop(ws);
    tokio::time::sleep(Duration::from_millis(400)).await;

    assert_eq!(error_lines(&logs), Vec::<String>::new());
}

#[tokio::test]
async fn invalid_utf8_in_a_text_frame_is_not_an_error_line() {
    let (logs, _guard) = capture_logs();
    let addr = spawn_server().await;
    let mut tcp = raw_upgrade(addr).await;
    let mask = [1u8, 2, 3, 4];
    // FIN + text opcode, masked, 2-byte payload `ff fe` (invalid UTF-8).
    tcp.write_all(&[
        0x81,
        0x82,
        mask[0],
        mask[1],
        mask[2],
        mask[3],
        0xff ^ mask[0],
        0xfe ^ mask[1],
    ])
    .await
    .expect("send frame");

    let mut sink = Vec::new();
    let _ = tokio::time::timeout(Duration::from_millis(500), tcp.read_to_end(&mut sink)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(error_lines(&logs), Vec::<String>::new());
}

#[tokio::test]
async fn a_clean_close_is_not_an_error_line() {
    let (logs, _guard) = capture_logs();
    let addr = spawn_server().await;
    let (mut ws, _) = connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("handshake");

    futures::SinkExt::send(&mut ws, Message::Close(None))
        .await
        .expect("send close");
    let _ = tokio::time::timeout(Duration::from_secs(2), ws.next()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(error_lines(&logs), Vec::<String>::new());
}
