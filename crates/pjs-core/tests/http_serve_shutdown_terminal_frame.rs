//! Tests for #625: `serve_with_limits` appends a terminal `shutdown` frame to
//! SSE and NDJSON streams still open at the end of the drain, and leaves
//! streams that finish on their own, compressed bodies and other content
//! types untouched.
#![cfg(feature = "http-server")]

use std::{convert::Infallible, net::SocketAddr, time::Duration};

use axum::{Router, body::Body, http::header, response::Response, routing::get};
use bytes::Bytes;
use pjson_rs::infrastructure::http::{
    ConnectionLimits, DrainTimeout, ShutdownConfig, ShutdownOutcome, ShutdownSignal,
    serve_with_limits,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::timeout,
};

const NDJSON_MARKER: &str = "{\"event\":\"shutdown\"}\n";
const DRAIN: Duration = Duration::from_millis(600);
const CHUNKED_END: &str = "\r\n0\r\n\r\n";
const SSE_MARKER: &str = "event: shutdown\ndata: {\"event\":\"shutdown\"}\n\n";

fn endless(content_type: &'static str, line: &'static str) -> Response {
    let chunks = futures::stream::unfold((), move |()| async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Some((Ok::<_, Infallible>(Bytes::from_static(line.as_bytes())), ()))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from_stream(chunks))
        .expect("valid response")
}

const FINITE_FRAMES: usize = 5;

fn always_ready() -> Response {
    let chunks =
        futures::stream::repeat_with(|| Ok::<_, Infallible>(Bytes::from_static(b"{\"n\":1}\n")));
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from_stream(chunks))
        .expect("valid response")
}

fn finite() -> Response {
    let chunks = futures::stream::unfold(0usize, |sent| async move {
        if sent == FINITE_FRAMES {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        let line = Bytes::from(format!("{{\"n\":{sent}}}\n"));
        Some((Ok::<_, Infallible>(line), sent + 1))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from_stream(chunks))
        .expect("valid response")
}

fn router() -> Router {
    Router::new()
        .route(
            "/ndjson",
            get(|| async { endless("application/x-ndjson", "{\"n\":1}\n") }),
        )
        .route(
            "/sse",
            get(|| async { endless("text/event-stream", "data: {\"n\":1}\n\n") }),
        )
        .route("/plain", get(|| async { endless("text/plain", "x") }))
        .route("/ready", get(|| async { always_ready() }))
        .route("/finite", get(|| async { finite() }))
        .route(
            "/encoded",
            get(|| async {
                let mut response = endless("application/x-ndjson", "{\"n\":1}\n");
                response
                    .headers_mut()
                    .insert(header::CONTENT_ENCODING, "gzip".parse().expect("header"));
                response
            }),
        )
}

struct Running {
    addr: SocketAddr,
    signal: ShutdownSignal,
    handle: JoinHandle<std::io::Result<ShutdownOutcome>>,
}

async fn start() -> Running {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let signal = ShutdownSignal::new();
    let config = ShutdownConfig {
        signal: signal.clone(),
        drain_timeout: DrainTimeout::try_from(DRAIN).expect("valid timeout"),
    };
    let handle = tokio::spawn(serve_with_limits(
        listener,
        router(),
        ConnectionLimits::default(),
        config,
    ));
    Running {
        addr,
        signal,
        handle,
    }
}

async fn open_stream(addr: SocketAddr, path: &str) -> TcpStream {
    let mut client = TcpStream::connect(addr).await.expect("connect");
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
    client.write_all(request.as_bytes()).await.expect("write");
    let mut seen = Vec::new();
    let mut buf = [0u8; 256];
    while !String::from_utf8_lossy(&seen).contains("\r\n\r\n") {
        let read = timeout(Duration::from_secs(5), client.read(&mut buf))
            .await
            .expect("headers in time")
            .expect("read");
        assert_ne!(read, 0, "connection closed before response headers");
        seen.extend_from_slice(&buf[..read]);
    }
    client
}

/// Reads until EOF or a connection error: a body aborted by the drain may end
/// with a reset instead of a clean close.
async fn read_until_closed(client: &mut TcpStream) -> String {
    let mut body = Vec::new();
    let mut buf = [0u8; 1024];
    timeout(Duration::from_secs(5), async {
        while let Ok(read) = client.read(&mut buf).await {
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read]);
        }
    })
    .await
    .expect("connection closes within the drain timeout");
    String::from_utf8_lossy(&body).into_owned()
}

async fn shutdown_and_collect(
    running: Running,
    mut client: TcpStream,
) -> (String, ShutdownOutcome) {
    running.signal.trigger();
    let body = read_until_closed(&mut client).await;
    let outcome = timeout(Duration::from_secs(5), running.handle)
        .await
        .expect("serve returns")
        .expect("join")
        .expect("serve ok");
    (body, outcome)
}

fn assert_marker_once_and_last(body: &str, marker: &str) {
    assert_eq!(body.matches(marker).count(), 1, "body: {body:?}");
    let payload = body.strip_suffix(CHUNKED_END).unwrap_or(body);
    assert!(
        payload.ends_with(marker),
        "marker must be last, body: {body:?}"
    );
}

#[tokio::test]
async fn ndjson_stream_open_at_drain_deadline_ends_with_one_shutdown_line() {
    let running = start().await;
    let client = open_stream(running.addr, "/ndjson").await;
    let (body, _outcome) = shutdown_and_collect(running, client).await;
    assert_marker_once_and_last(&body, NDJSON_MARKER);
}

/// A body that is ready on every poll must not starve the terminal-frame
/// deadline. The marker is the last payload, so the tail is kept while the
/// bulk is discarded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn always_ready_body_still_gets_terminal_frame() {
    let running = start().await;
    let mut client = open_stream(running.addr, "/ready").await;
    running.signal.trigger();
    let mut tail = String::new();
    let mut buf = vec![0u8; 64 * 1024];
    timeout(Duration::from_secs(5), async {
        while let Ok(read) = client.read(&mut buf).await {
            if read == 0 {
                break;
            }
            tail.push_str(&String::from_utf8_lossy(&buf[..read]));
            let keep_from = tail.len().saturating_sub(64);
            let keep_from = (keep_from..=tail.len())
                .find(|&i| tail.is_char_boundary(i))
                .unwrap_or(tail.len());
            tail.drain(..keep_from);
        }
    })
    .await
    .expect("connection closes within the drain timeout");
    let payload = tail.strip_suffix(CHUNKED_END).unwrap_or(&tail);
    assert!(payload.ends_with(NDJSON_MARKER), "tail: {tail:?}");
}

#[tokio::test]
async fn sse_stream_open_at_drain_deadline_ends_with_one_shutdown_event() {
    let running = start().await;
    let client = open_stream(running.addr, "/sse").await;
    let (body, _outcome) = shutdown_and_collect(running, client).await;
    assert_marker_once_and_last(&body, SSE_MARKER);
}

#[tokio::test]
async fn finite_stream_completes_during_drain_without_terminal_frame() {
    let running = start().await;
    let client = open_stream(running.addr, "/finite").await;
    let (body, outcome) = shutdown_and_collect(running, client).await;
    for n in 0..FINITE_FRAMES {
        assert!(body.contains(&format!("{{\"n\":{n}}}\n")), "body: {body:?}");
    }
    assert!(!body.contains("shutdown"), "body: {body:?}");
    assert_eq!(outcome, ShutdownOutcome::Drained);
}

#[tokio::test]
async fn content_encoded_response_is_left_untouched() {
    let running = start().await;
    let mut client = open_stream(running.addr, "/encoded").await;
    running.signal.trigger();
    let body = read_until_closed(&mut client).await;
    assert!(!body.contains("shutdown"), "body: {body:?}");
}

#[tokio::test]
async fn other_content_types_get_no_terminal_frame() {
    let running = start().await;
    let mut client = open_stream(running.addr, "/plain").await;
    running.signal.trigger();
    let body = read_until_closed(&mut client).await;
    assert!(!body.contains("shutdown"), "body: {body:?}");
}
