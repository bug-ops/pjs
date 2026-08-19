//! Integration tests for connection-level protection provided by
//! `infrastructure::http::serve_with_limits` (#523).
//!
//! These tests drive real TCP sockets against a real `serve_with_limits`
//! listener rather than exercising the router through `tower::ServiceExt`,
//! because the behavior under test — header-read timeouts, whole-connection
//! deadlines, and accept-loop backpressure — only exists at the
//! connection/socket level, not the request/response level `oneshot` tests
//! cover.
#![cfg(feature = "http-server")]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::{Router, extract::ConnectInfo, routing::get};
use pjson_rs::infrastructure::http::{ConnectionLimits, serve_with_limits};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

fn ok_router() -> Router {
    Router::new().route("/", get(|| async { "ok" }))
}

/// Binds `router` behind `serve_with_limits` on a random localhost port and
/// spawns the accept loop, returning the bound address.
async fn spawn_server(router: Router, limits: ConnectionLimits) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind random port");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = serve_with_limits(listener, router, limits).await;
    });
    addr
}

/// A connection that sends a partial request line and then goes silent is
/// closed once `header_read_timeout` elapses.
///
/// This is the regression test for hyper's `Time::check` panic: calling
/// `.header_read_timeout(d)` on an `http1` builder without first calling
/// `.timer(TokioTimer::new())` panics the instant a connection starts being
/// served, closing it near-instantly instead of after the configured
/// duration. Asserting the close takes at least close to the configured
/// timeout (not near-zero) proves the timer really is wired up.
#[tokio::test]
async fn header_read_timeout_closes_stalled_connection() {
    let mut limits = ConnectionLimits::default();
    limits.header_read_timeout = Some(Duration::from_millis(150));
    limits.max_connection_duration = None;
    let addr = spawn_server(ok_router(), limits).await;

    let mut client = TcpStream::connect(addr).await.expect("connect");
    client
        .write_all(b"GET / HTTP/1.1\r\n")
        .await
        .expect("write partial request line");

    let start = Instant::now();
    let mut buf = [0u8; 16];
    let read = timeout(Duration::from_secs(3), client.read(&mut buf))
        .await
        .expect("server never closed the stalled connection");
    let elapsed = start.elapsed();

    assert_eq!(
        read.expect("read error"),
        0,
        "expected EOF once the header-read timeout closes the connection"
    );
    assert!(
        elapsed >= Duration::from_millis(80),
        "connection closed almost instantly ({elapsed:?}); this points at a panic \
         during connection setup (missing timer) rather than a real timeout firing"
    );
}

/// `serve_with_limits` makes `ConnectInfo<SocketAddr>` available to
/// handlers and middleware, matching what
/// `axum::serve(listener, router.into_make_service_with_connect_info())`
/// would provide.
///
/// This is a regression test: an earlier version of `serve_with_limits`
/// discarded the peer address from `accept()`, which 500'd every
/// `ConnectInfo`-based extractor — including this crate's own WebSocket
/// upgrade handler — and silently collapsed the per-IP rate limiter onto a
/// single shared bucket.
#[tokio::test]
async fn connect_info_is_available_to_handlers() {
    let router = Router::new().route(
        "/whoami",
        get(|ConnectInfo(addr): ConnectInfo<SocketAddr>| async move { addr.to_string() }),
    );
    let addr = spawn_server(router, ConnectionLimits::default()).await;

    let mut client = TcpStream::connect(addr).await.expect("connect");
    let client_local_addr = client.local_addr().expect("client local_addr");
    client
        .write_all(b"GET /whoami HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write request");

    let mut response = Vec::new();
    timeout(Duration::from_secs(3), client.read_to_end(&mut response))
        .await
        .expect("timed out waiting for response")
        .expect("read error");

    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "ConnectInfo extractor failed (expected 200, e.g. \"missing request extension\" \
         if the peer address was never injected): {response}"
    );
    assert!(
        response.ends_with(&client_local_addr.to_string()),
        "handler did not see the real peer address: {response}"
    );
}

/// A connection that completes a normal request/response cycle through
/// `serve_with_limits` is unaffected by the connection-level limits.
#[tokio::test]
async fn normal_request_completes_successfully() {
    let addr = spawn_server(ok_router(), ConnectionLimits::default()).await;

    let mut client = TcpStream::connect(addr).await.expect("connect");
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write request");

    let mut response = Vec::new();
    timeout(Duration::from_secs(3), client.read_to_end(&mut response))
        .await
        .expect("timed out waiting for response")
        .expect("read error");

    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "unexpected response: {response}"
    );
    assert!(response.ends_with("ok"), "unexpected body: {response}");
}

/// A connection whose client never sends a full request (and never closes
/// the socket) is force-closed once `max_connection_duration` elapses —
/// proving `serve_with_limits` closes the gap #523 identified: a stalled
/// client is no longer able to hold a connection open indefinitely.
#[tokio::test]
async fn max_connection_duration_closes_idle_connection() {
    let mut limits = ConnectionLimits::default();
    limits.header_read_timeout = None;
    limits.max_connection_duration = Some(Duration::from_millis(200));
    let addr = spawn_server(ok_router(), limits).await;

    let mut client = TcpStream::connect(addr).await.expect("connect");

    let start = Instant::now();
    let mut buf = [0u8; 16];
    let read = timeout(Duration::from_secs(3), client.read(&mut buf))
        .await
        .expect("server never closed the idle connection");
    let elapsed = start.elapsed();

    assert_eq!(
        read.expect("read error"),
        0,
        "expected EOF once max_connection_duration closes the connection"
    );
    assert!(
        elapsed >= Duration::from_millis(150),
        "connection closed almost instantly ({elapsed:?}); expected it to survive \
         close to the configured max_connection_duration"
    );
}

/// A client that completes its request but then stops reading a large
/// response body is force-closed once `max_connection_duration` elapses.
///
/// This is the literal #523 threat, distinct from the other
/// `max_connection_duration` test above (which covers a client that never
/// finishes its *request*): `ResponseBodyTimeoutLayer` in
/// `apply_common_layers` cannot detect this case because `TimeoutBody`'s
/// deadline only resets when the body is *polled*, and hyper stops polling
/// a response body once its outbound socket buffer fills waiting on the
/// client to read — a client that stops reading entirely is never caught by
/// that layer. Only a connection-level deadline, as enforced here, closes
/// the gap.
#[tokio::test]
async fn max_connection_duration_closes_connection_with_stalled_body_reader() {
    let mut limits = ConnectionLimits::default();
    limits.header_read_timeout = None;
    limits.max_connection_duration = Some(Duration::from_millis(400));
    // Large enough that it cannot fit into the client's OS-level receive
    // buffer plus the server's send buffer, so hyper's writer genuinely
    // stalls in `poll_flush` waiting on socket writability rather than the
    // whole body slipping through before the deadline fires.
    let big_body_router =
        Router::new().route("/big", get(|| async { vec![0u8; 64 * 1024 * 1024] }));
    let addr = spawn_server(big_body_router, limits).await;

    let mut client = TcpStream::connect(addr).await.expect("connect");
    client
        .write_all(b"GET /big HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write request");

    // Never read the response at all until well past the configured
    // deadline: the request is complete (so `ResponseBodyTimeoutLayer`'s
    // idle-producer case does not apply either — the producer here is an
    // already-materialized `Vec`, never idle), but the client refuses to
    // drain the socket, so hyper's writer stalls in `poll_flush` waiting on
    // socket writability.
    tokio::time::sleep(Duration::from_millis(700)).await;

    // Drain whatever was already buffered before the stall, then expect the
    // connection to end (EOF or a reset) well short of the full body —
    // proving `max_connection_duration` force-closed it rather than the
    // transfer completing normally.
    let start = Instant::now();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total_bytes = 0usize;
    loop {
        match timeout(Duration::from_secs(3), client.read(&mut buf)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => total_bytes += n,
            Ok(Err(_)) => break, // connection reset counts as closed too
            Err(_) => panic!(
                "server never finished closing the stalled-body-reader connection \
                 ({total_bytes} bytes drained so far)"
            ),
        }
    }
    let drain_elapsed = start.elapsed();

    assert!(
        total_bytes < 64 * 1024 * 1024,
        "received the full 64MB body ({total_bytes} bytes); expected \
         max_connection_duration to force-close the connection before the stalled \
         writer could finish, once the client resumed reading"
    );
    assert!(
        drain_elapsed < Duration::from_secs(3),
        "draining the buffered bytes and observing closure took too long ({drain_elapsed:?})"
    );
}

/// A connection that sends **zero bytes** (never even a partial request
/// line) is closed once `header_read_timeout` elapses, not left to linger
/// until `max_connection_duration`.
///
/// This is the headline finding from the #525/#526 security audit:
/// hyper-util's `auto::Builder` runs an unguarded `ReadVersion` preface-sniff
/// step before any protocol builder engages, and `header_read_timeout` only
/// arms once >=1 byte classifies the connection as H1/H2 — so before the
/// accept-level preface-read gate, a client sending nothing at all was
/// bounded solely by `max_connection_duration` (300s default), making it far
/// cheaper to hold open than `header_read_timeout_closes_stalled_connection`
/// above (which sends a partial request line and thus already exercises the
/// gated-on-first-byte h1 timeout).
#[tokio::test]
async fn zero_byte_connection_closed_by_header_read_timeout_not_max_duration() {
    let mut limits = ConnectionLimits::default();
    limits.header_read_timeout = Some(Duration::from_millis(150));
    limits.max_connection_duration = Some(Duration::from_secs(5));
    let addr = spawn_server(ok_router(), limits).await;

    let mut client = TcpStream::connect(addr).await.expect("connect");
    // Deliberately send nothing.

    let start = Instant::now();
    let mut buf = [0u8; 16];
    let read = timeout(Duration::from_secs(2), client.read(&mut buf))
        .await
        .expect(
            "connection sending zero bytes was not closed within 2s (well short of the 5s \
         max_connection_duration) -- the ReadVersion preface-sniff hole looks unguarded",
        );
    let elapsed = start.elapsed();

    assert_eq!(
        read.expect("read error"),
        0,
        "expected EOF once the preface-read gate closes the zero-byte connection"
    );
    assert!(
        elapsed >= Duration::from_millis(80) && elapsed < Duration::from_secs(2),
        "connection closed after {elapsed:?}; expected closure near header_read_timeout \
         (150ms), not near-instant (missing timer) or near max_connection_duration (5s)"
    );
}

/// A source IP that opens more connections than `max_connections_per_ip`
/// has the excess rejected at accept level — before any task is spawned or
/// byte is read — while connections within the cap are served normally.
#[tokio::test]
async fn per_ip_cap_rejects_connections_beyond_limit() {
    let mut limits = ConnectionLimits::default();
    limits.header_read_timeout = None;
    limits.max_connection_duration = None;
    limits.max_connections_per_ip = Some(2);
    let addr = spawn_server(ok_router(), limits).await;

    // Open (and hold open, never completing a request) `max_connections_per_ip`
    // connections from the same source IP (127.0.0.1), each occupying a
    // per-IP slot until dropped at the end of the test.
    let mut held = Vec::new();
    for _ in 0..2 {
        held.push(TcpStream::connect(addr).await.expect("connect within cap"));
        tokio::time::sleep(Duration::from_millis(30)).await;
    }

    // A third connection from the same IP exceeds the cap and must be
    // rejected immediately, without ever reaching the router.
    let mut excess = TcpStream::connect(addr).await.expect("connect excess");
    let mut buf = [0u8; 16];
    let read = timeout(Duration::from_secs(1), excess.read(&mut buf))
        .await
        .expect("excess connection was never closed");
    assert_eq!(
        read.expect("read error"),
        0,
        "expected EOF: a connection beyond max_connections_per_ip should be rejected"
    );

    drop(held);
}

/// Rejecting a connection for exceeding `max_connections_per_ip` releases
/// the *global* `max_connections` semaphore permit it had acquired before
/// the per-IP check ran — the permit must not leak.
///
/// Two genuinely different source IPs aren't reachable from a single-host
/// test harness without OS-level loopback aliasing, so this proves permit
/// release indirectly: with `max_connections: 2` and `max_connections_per_ip:
/// 1`, connection A holds the one per-IP slot (and one of the two global
/// permits) open. Connection B, same IP, exceeds the per-IP cap and is
/// rejected. If B's global permit leaked instead of being released by the
/// accept loop's `continue`, both global permits would now be permanently
/// consumed (one held by A, one leaked by B), and the accept loop would
/// never call `accept()` again — connection C would sit un-accepted (no read
/// event at all) rather than receiving its own prompt EOF from the per-IP
/// check.
#[tokio::test]
async fn per_ip_rejection_releases_the_global_connection_permit() {
    let mut limits = ConnectionLimits::default();
    limits.header_read_timeout = None;
    limits.max_connection_duration = None;
    limits.max_connections = 2;
    limits.max_connections_per_ip = Some(1);
    let addr = spawn_server(ok_router(), limits).await;

    let _client_a = TcpStream::connect(addr).await.expect("connect A");
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client_b = TcpStream::connect(addr).await.expect("connect B");
    let mut buf = [0u8; 16];
    let read_b = timeout(Duration::from_secs(1), client_b.read(&mut buf))
        .await
        .expect("connection B was never closed");
    assert_eq!(
        read_b.expect("read error"),
        0,
        "expected EOF: B exceeds the per-IP cap"
    );

    let mut client_c = TcpStream::connect(addr).await.expect("connect C");
    let read_c = timeout(Duration::from_millis(500), client_c.read(&mut buf))
        .await
        .expect(
            "connection C was never accepted -- this indicates B's global semaphore permit \
         leaked instead of being released on rejection",
        );
    assert_eq!(
        read_c.expect("read error"),
        0,
        "expected EOF: C also exceeds the per-IP cap, but must still be accepted promptly"
    );
}

/// With `max_connections: 1`, a second connection is left unserved — its
/// bytes sit unread on the socket — until the first connection's permit is
/// released, proving the accept loop applies real backpressure rather than
/// accepting unboundedly.
#[tokio::test]
async fn max_connections_backpressures_additional_connections() {
    let mut limits = ConnectionLimits::default();
    limits.header_read_timeout = None;
    limits.max_connection_duration = None;
    limits.max_connections = 1;
    let addr = spawn_server(ok_router(), limits).await;

    // Client A occupies the single permit by connecting and never
    // completing its request.
    let client_a = TcpStream::connect(addr).await.expect("connect A");
    // Give the accept loop a moment to actually accept A and start serving
    // it (and thus hold the permit) before B connects.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Client B connects (the kernel backlog accepts the handshake even
    // though the server hasn't called `accept()` for it yet) and sends a
    // complete request.
    let mut client_b = TcpStream::connect(addr).await.expect("connect B");
    client_b
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write request B");

    // B must not be served yet: the single permit is held by A.
    let mut buf = [0u8; 16];
    let still_pending = timeout(Duration::from_millis(300), client_b.read(&mut buf)).await;
    assert!(
        still_pending.is_err(),
        "client B was served before the single connection permit was released"
    );

    // Releasing A's connection frees the permit, letting B's already-queued
    // connection be accepted and served.
    drop(client_a);

    let mut response = Vec::new();
    timeout(Duration::from_secs(3), client_b.read_to_end(&mut response))
        .await
        .expect("client B was never served after the permit was released")
        .expect("read error");

    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "unexpected response for client B: {response}"
    );
}
