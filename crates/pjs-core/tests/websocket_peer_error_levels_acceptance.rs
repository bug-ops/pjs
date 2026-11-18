//! Acceptance tests for #767 (track D, coverage gap): the log level of a
//! peer-caused socket error follows its class. A connection reset (RST) is a
//! peer disconnect (`INFO`); a reset without a closing handshake and invalid
//! UTF-8 are protocol violations (`WARN`); neither is `ERROR`.
//!
//! The `Server` class (an I/O failure not caused by the peer, kept at
//! `ERROR`) cannot be provoked from a client; it needs an in-module unit test
//! of `SocketErrorClass::of`.
#![cfg(all(feature = "websocket-server", feature = "websocket-client"))]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::connect_async;

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

/// Server log lines of the websocket module, split by level.
struct Levels {
    info: Vec<String>,
    warn: Vec<String>,
    error: Vec<String>,
}

impl LogBuffer {
    fn levels(&self) -> Levels {
        let text = String::from_utf8_lossy(&self.0.lock().expect("log lock")).into_owned();
        let of = |level: &str| -> Vec<String> {
            text.lines()
                .filter(|line| {
                    line.contains(&format!(" {level} "))
                        && line.contains("pjson_rs::infrastructure::websocket::server")
                })
                .map(str::to_owned)
                .collect()
        };
        Levels {
            info: of("INFO"),
            warn: of("WARN"),
            error: of("ERROR"),
        }
    }
}

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
    tcp
}

#[tokio::test]
async fn a_connection_reset_is_logged_at_info_as_a_peer_disconnect() {
    let (logs, _guard) = capture_logs();
    let addr = spawn_server().await;
    let tcp = raw_upgrade(addr).await;

    // Linger of zero makes the drop send an RST instead of a FIN.
    #[allow(deprecated)]
    tcp.set_linger(Some(Duration::ZERO)).expect("linger");
    drop(tcp);
    tokio::time::sleep(Duration::from_millis(400)).await;

    let levels = logs.levels();
    assert_eq!(levels.error, Vec::<String>::new());
    assert_eq!(
        levels.warn,
        Vec::<String>::new(),
        "a reset is not a protocol violation"
    );
    assert!(
        levels
            .info
            .iter()
            .any(|line| line.to_lowercase().contains("reset")),
        "no INFO line reports the reset: {:?}",
        levels.info
    );
}

#[tokio::test]
async fn a_reset_without_a_closing_handshake_is_logged_at_warn() {
    let (logs, _guard) = capture_logs();
    let addr = spawn_server().await;
    let (ws, _) = connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("handshake");

    drop(ws);
    tokio::time::sleep(Duration::from_millis(400)).await;

    let levels = logs.levels();
    assert_eq!(levels.error, Vec::<String>::new());
    assert!(
        levels
            .warn
            .iter()
            .any(|line| line.contains("closing handshake")),
        "no WARN line reports the missing closing handshake: {:?}",
        levels.warn
    );
}

#[tokio::test]
async fn invalid_utf8_is_logged_at_warn() {
    let (logs, _guard) = capture_logs();
    let addr = spawn_server().await;
    let mut tcp = raw_upgrade(addr).await;
    let mask = [1u8, 2, 3, 4];
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

    let levels = logs.levels();
    assert_eq!(levels.error, Vec::<String>::new());
    assert!(
        levels
            .warn
            .iter()
            .any(|line| line.to_lowercase().contains("utf-8")),
        "no WARN line reports the invalid UTF-8: {:?}",
        levels.warn
    );
}
