//! Acceptance tests for #590 (graceful shutdown of `serve_with_limits`) and
//! #618 (`ConnectionLimits::max_header_bytes`).
//!
//! Assumed API (plan D1-D3, D10), all under `pjson_rs::infrastructure::http`:
//! `ShutdownSignal`, `ShutdownConfig { signal, drain_timeout }`, `DrainTimeout`,
//! `ShutdownOutcome`, `HeaderSizeLimit`, and
//! `serve_with_limits(listener, router, limits, ShutdownConfig) -> io::Result<ShutdownOutcome>`.
#![cfg(feature = "http-server")]

use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::{Router, body::Body, routing::get};
use bytes::Bytes;
use pjson_rs::infrastructure::http::{
    ConnectionLimits, DrainTimeout, HeaderSizeLimit, ShutdownConfig, ShutdownOutcome,
    ShutdownSignal, serve_with_limits,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::timeout,
};

fn drain(millis: u64) -> DrainTimeout {
    DrainTimeout::try_from(Duration::from_millis(millis)).expect("valid drain timeout")
}

fn router() -> Router {
    Router::new()
        .route("/", get(|| async { "ok" }))
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(400)).await;
                "finished"
            }),
        )
        .route(
            "/endless",
            get(|| async {
                let chunks = futures::stream::unfold((), |()| async {
                    Some((Ok::<_, Infallible>(Bytes::from(vec![b'x'; 64 * 1024])), ()))
                });
                Body::from_stream(chunks)
            }),
        )
}

struct Running {
    addr: SocketAddr,
    signal: ShutdownSignal,
    handle: JoinHandle<std::io::Result<ShutdownOutcome>>,
}

async fn start(router: Router, limits: ConnectionLimits, drain_timeout: DrainTimeout) -> Running {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let signal = ShutdownSignal::new();
    let config = ShutdownConfig {
        signal: signal.clone(),
        drain_timeout,
    };
    let handle = tokio::spawn(serve_with_limits(listener, router, limits, config));
    Running {
        addr,
        signal,
        handle,
    }
}

async fn request(addr: SocketAddr, raw: &str) -> String {
    let mut client = TcpStream::connect(addr).await.expect("connect");
    client.write_all(raw.as_bytes()).await.expect("write");
    let mut response = Vec::new();
    timeout(Duration::from_secs(5), client.read_to_end(&mut response))
        .await
        .expect("response in time")
        .expect("read");
    String::from_utf8_lossy(&response).into_owned()
}

fn get_close(path: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
}

#[test]
fn test_drain_timeout_try_from_bounds() {
    assert!(DrainTimeout::try_from(Duration::ZERO).is_err());
    assert!(DrainTimeout::try_from(Duration::from_millis(1)).is_ok());
    assert!(DrainTimeout::try_from(Duration::from_secs(600)).is_ok());
    assert!(DrainTimeout::try_from(Duration::from_secs(601)).is_err());
}

#[test]
fn test_drain_timeout_default_is_30_seconds() {
    assert_eq!(
        Duration::from(DrainTimeout::default()),
        Duration::from_secs(30)
    );
}

#[test]
fn test_shutdown_signal_trigger_is_idempotent_and_shared_by_clones() {
    let signal = ShutdownSignal::new();
    let clone = signal.clone();
    assert!(!signal.is_triggered());
    clone.trigger();
    clone.trigger();
    assert!(signal.is_triggered());
    assert!(clone.is_triggered());
}

#[tokio::test]
async fn test_shutdown_signal_triggered_future_completes_after_trigger() {
    let signal = ShutdownSignal::new();
    let waiter = signal.clone();
    let task = tokio::spawn(async move { waiter.triggered().await });
    signal.trigger();
    timeout(Duration::from_secs(1), task)
        .await
        .expect("triggered() resolves")
        .expect("task joins");
}

#[tokio::test]
async fn test_serve_with_limits_idle_server_drains_after_trigger() {
    let server = start(router(), ConnectionLimits::default(), drain(2_000)).await;
    assert!(request(server.addr, &get_close("/")).await.contains("200"));
    server.signal.trigger();
    let outcome = timeout(Duration::from_secs(3), server.handle)
        .await
        .expect("serve returns")
        .expect("join")
        .expect("io ok");
    assert_eq!(outcome, ShutdownOutcome::Drained);
}

#[tokio::test]
async fn test_serve_with_limits_in_flight_request_completes_before_drain() {
    let server = start(router(), ConnectionLimits::default(), drain(5_000)).await;
    let addr = server.addr;
    let client = tokio::spawn(async move { request(addr, &get_close("/slow")).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.signal.trigger();

    let response = client.await.expect("client joins");
    assert!(response.contains("200"), "{response}");
    assert!(response.ends_with("finished"), "{response}");
    let outcome = timeout(Duration::from_secs(3), server.handle)
        .await
        .expect("serve returns")
        .expect("join")
        .expect("io ok");
    assert_eq!(outcome, ShutdownOutcome::Drained);
}

#[tokio::test]
async fn test_serve_with_limits_stalled_reader_forces_shutdown_within_timeout() {
    let server = start(router(), ConnectionLimits::default(), drain(300)).await;
    let mut client = TcpStream::connect(server.addr).await.expect("connect");
    client
        .write_all(get_close("/endless").as_bytes())
        .await
        .expect("write");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let started = Instant::now();
    server.signal.trigger();
    let outcome = timeout(Duration::from_secs(3), server.handle)
        .await
        .expect("serve returns")
        .expect("join")
        .expect("io ok");
    assert!(
        matches!(outcome, ShutdownOutcome::Forced { .. }),
        "{outcome:?}"
    );
    assert!(started.elapsed() < Duration::from_millis(1_500));
    drop(client);
}

#[tokio::test]
async fn test_serve_with_limits_pre_triggered_signal_returns_without_serving() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let signal = ShutdownSignal::new();
    signal.trigger();
    let config = ShutdownConfig {
        signal,
        drain_timeout: drain(500),
    };
    let outcome = timeout(
        Duration::from_secs(2),
        serve_with_limits(listener, router(), ConnectionLimits::default(), config),
    )
    .await
    .expect("returns promptly")
    .expect("io ok");
    assert_eq!(outcome, ShutdownOutcome::Drained);
    assert!(TcpStream::connect(addr).await.is_err());
}

#[tokio::test]
async fn test_serve_with_limits_connection_after_trigger_not_served() {
    let server = start(router(), ConnectionLimits::default(), drain(500)).await;
    let addr = server.addr;
    server.signal.trigger();
    timeout(Duration::from_secs(2), server.handle)
        .await
        .expect("serve returns")
        .expect("join")
        .expect("io ok");

    let served = match TcpStream::connect(addr).await {
        Err(_) => false,
        Ok(mut client) => {
            let _ = client.write_all(get_close("/").as_bytes()).await;
            let mut buf = Vec::new();
            let _ = timeout(Duration::from_secs(1), client.read_to_end(&mut buf)).await;
            String::from_utf8_lossy(&buf).contains("200")
        }
    };
    assert!(!served);
}

#[test]
fn test_header_size_limit_new_bounds() {
    assert!(HeaderSizeLimit::new(8191).is_err());
    assert!(HeaderSizeLimit::new(8192).is_ok());
    assert!(HeaderSizeLimit::new(u32::MAX).is_ok());
}

#[test]
fn test_connection_limits_default_header_bytes_is_none() {
    assert!(ConnectionLimits::default().max_header_bytes.is_none());
}

fn request_with_header(len: usize) -> String {
    format!(
        "GET / HTTP/1.1\r\nHost: localhost\r\nX-Pad: {}\r\nConnection: close\r\n\r\n",
        "a".repeat(len)
    )
}

#[tokio::test]
async fn test_max_header_bytes_some_rejects_oversized_h1_header_with_431() {
    let mut limits = ConnectionLimits::default();
    limits.max_header_bytes = Some(HeaderSizeLimit::new(16 * 1024).expect("valid"));
    let server = start(router(), limits, drain(500)).await;

    let rejected = request(server.addr, &request_with_header(20_000)).await;
    assert!(rejected.starts_with("HTTP/1.1 431"), "{rejected}");
    let accepted = request(server.addr, &request_with_header(8_000)).await;
    assert!(accepted.starts_with("HTTP/1.1 200"), "{accepted}");
}

#[tokio::test]
async fn test_max_header_bytes_default_keeps_hyper_h1_limit() {
    let server = start(router(), ConnectionLimits::default(), drain(500)).await;
    let response = request(server.addr, &request_with_header(70_000)).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}
