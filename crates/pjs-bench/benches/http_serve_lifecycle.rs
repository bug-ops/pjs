//! Benchmarks for the `serve_with_limits` connection lifecycle (#630).
//!
//! Covers the accept loop and per-connection setup added by graceful shutdown
//! (`spawn_tracked` tracker, shutdown `select!` arms), which no other bench
//! exercises:
//!
//! - `request_roundtrip`: connect, one `GET`, read to EOF against a running,
//!   never-triggered server (accept + connection setup + teardown).
//! - `serve_pre_triggered`: bind and `serve_with_limits` with an already
//!   triggered signal (loop entry and drain bookkeeping, no connections).
//! - `trigger_with_idle_connections`: trigger shutdown while N connections sit
//!   before their first byte, then await the drain (shutdown arm on every
//!   connection task).

use std::{net::SocketAddr, time::Duration};

use axum::{Router, routing::get};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use pjson_rs::infrastructure::http::{
    ConnectionLimits, DrainTimeout, ShutdownConfig, ShutdownSignal, serve_with_limits,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    runtime::{Builder, Runtime},
};

const IDLE_CONNECTIONS: [usize; 2] = [1, 16];
const REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: bench\r\nConnection: close\r\n\r\n";

fn runtime() -> Runtime {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn router() -> Router {
    Router::new().route("/", get(|| async { "ok" }))
}

fn shutdown_config(signal: &ShutdownSignal) -> ShutdownConfig {
    ShutdownConfig {
        signal: signal.clone(),
        drain_timeout: DrainTimeout::try_from(Duration::from_secs(5)).expect("valid timeout"),
    }
}

async fn bind() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    (listener, addr)
}

async fn roundtrip(addr: SocketAddr) {
    let mut client = TcpStream::connect(addr).await.expect("connect");
    client.write_all(REQUEST).await.expect("write");
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.expect("read");
    assert!(response.starts_with(b"HTTP/1.1 200"));
}

fn bench_request_roundtrip(c: &mut Criterion) {
    let rt = runtime();
    let signal = ShutdownSignal::new();
    let (listener, addr) = rt.block_on(bind());
    let server = rt.spawn(serve_with_limits(
        listener,
        router(),
        ConnectionLimits::default(),
        shutdown_config(&signal),
    ));

    c.bench_function("http_serve_lifecycle_request_roundtrip", |b| {
        b.iter(|| rt.block_on(roundtrip(addr)));
    });

    signal.trigger();
    rt.block_on(server).expect("join").expect("serve");
}

fn bench_serve_pre_triggered(c: &mut Criterion) {
    let rt = runtime();
    c.bench_function("http_serve_lifecycle_serve_pre_triggered", |b| {
        b.iter(|| {
            rt.block_on(async {
                let signal = ShutdownSignal::new();
                signal.trigger();
                let (listener, _addr) = bind().await;
                serve_with_limits(
                    listener,
                    router(),
                    ConnectionLimits::default(),
                    shutdown_config(&signal),
                )
                .await
                .expect("serve")
            })
        });
    });
}

fn bench_trigger_with_idle_connections(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("http_serve_lifecycle_trigger_with_idle_connections");
    for connections in IDLE_CONNECTIONS {
        group.bench_with_input(
            BenchmarkId::from_parameter(connections),
            &connections,
            |b, &connections| {
                b.iter(|| {
                    rt.block_on(async {
                        let signal = ShutdownSignal::new();
                        let (listener, addr) = bind().await;
                        let server = tokio::spawn(serve_with_limits(
                            listener,
                            router(),
                            ConnectionLimits::default(),
                            shutdown_config(&signal),
                        ));
                        let mut clients = Vec::with_capacity(connections);
                        for _ in 0..connections {
                            clients.push(TcpStream::connect(addr).await.expect("connect"));
                        }
                        tokio::task::yield_now().await;
                        signal.trigger();
                        server.await.expect("join").expect("serve")
                    })
                });
            },
        );
    }
    group.finish();
}

fn configured() -> Criterion {
    Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(3))
}

criterion_group! {
    name = benches;
    config = configured();
    targets = bench_request_roundtrip, bench_serve_pre_triggered, bench_trigger_with_idle_connections
}
criterion_main!(benches);
