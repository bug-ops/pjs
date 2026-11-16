//! Benchmarks for `AdaptiveStreamController` (#664).
//!
//! - `create_session`: cost of planning a document of 1 KiB to 1 MiB, the input
//!   for the `spawn_blocking` threshold of #658. Sizes above the threshold (64 KiB
//!   and up) now include the blocking-pool hop (about 6 us); the inline cost of
//!   those sizes is the pre-#658 figure in spec 066.
//! - `spawn_blocking_round_trip`: fixed cost of the blocking-pool hop.
//! - `handle_frame_ack`: throughput of a per-frame global-lock operation on
//!   500 connections from 1, 4, 8 and 16 concurrent tasks, the proxy for the
//!   per-frame `release_sent` write lock.

use std::{
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use futures::future::join_all;
use pjson_rs::{
    domain::SessionId,
    infrastructure::websocket::{AdaptiveStreamController, ConnectionId, StreamOptions},
};
use serde_json::{Value, json};
use tokio::runtime::{Builder, Runtime};

const DOCUMENT_BYTES: [usize; 7] = [
    1024,
    4 * 1024,
    16 * 1024,
    32 * 1024,
    64 * 1024,
    256 * 1024,
    1024 * 1024,
];
const CONNECTIONS: usize = 500;
const TASKS: [usize; 4] = [1, 4, 8, 16];
const ACKS_PER_TASK: usize = 2_000;
const ENTRY_BYTES: usize = 64;

fn current_thread() -> Runtime {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn multi_thread() -> Runtime {
    Builder::new_multi_thread()
        .worker_threads(8)
        .enable_all()
        .build()
        .expect("runtime")
}

/// Object of string entries whose serialized size is close to `bytes`.
fn document(bytes: usize) -> Value {
    let text = "x".repeat(ENTRY_BYTES);
    let entries = (bytes / (ENTRY_BYTES + 16)).max(1);
    Value::Object(
        (0..entries)
            .map(|i| (format!("key{i:06}"), json!(text)))
            .collect(),
    )
}

fn bench_create_session(c: &mut Criterion) {
    let mut group = c.benchmark_group("ws_controller_create_session");
    let rt = current_thread();
    for bytes in DOCUMENT_BYTES {
        let data = document(bytes);
        group.throughput(Throughput::Bytes(bytes as u64));
        group.bench_with_input(BenchmarkId::from_parameter(bytes), &data, |b, data| {
            let controller = AdaptiveStreamController::new();
            let owner = ConnectionId::new();
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let id = SessionId::new();
                    let data = data.clone();
                    let started = Instant::now();
                    let result = rt.block_on(controller.create_session(
                        id,
                        owner,
                        data,
                        StreamOptions::default(),
                    ));
                    total += started.elapsed();
                    assert!(result.is_ok(), "{result:?}");
                    rt.block_on(controller.remove_session(owner, id));
                }
                total
            });
        });
    }
    group.finish();
}

fn bench_spawn_blocking(c: &mut Criterion) {
    let rt = multi_thread();
    c.bench_function("ws_controller_spawn_blocking_round_trip", |b| {
        b.iter(|| {
            rt.block_on(async {
                tokio::task::spawn_blocking(|| black_box(()))
                    .await
                    .expect("blocking task")
            })
        });
    });
}

async fn populated_controller() -> (
    Arc<AdaptiveStreamController>,
    Vec<(ConnectionId, SessionId)>,
) {
    let controller = Arc::new(AdaptiveStreamController::new());
    let mut sessions = Vec::with_capacity(CONNECTIONS);
    for _ in 0..CONNECTIONS {
        let owner = ConnectionId::new();
        let id = SessionId::new();
        controller
            .create_session(id, owner, json!({}), StreamOptions::default())
            .await
            .expect("session");
        sessions.push((owner, id));
    }
    (controller, sessions)
}

fn bench_handle_frame_ack(c: &mut Criterion) {
    let mut group = c.benchmark_group("ws_controller_handle_frame_ack");
    let rt = multi_thread();
    let (controller, sessions) = rt.block_on(populated_controller());
    let sessions = Arc::new(sessions);

    for tasks in TASKS {
        group.throughput(Throughput::Elements((tasks * ACKS_PER_TASK) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(tasks), &tasks, |b, &tasks| {
            b.iter(|| {
                rt.block_on(async {
                    let workers = (0..tasks).map(|task| {
                        let controller = Arc::clone(&controller);
                        let sessions = Arc::clone(&sessions);
                        tokio::spawn(async move {
                            for i in 0..ACKS_PER_TASK {
                                let (owner, id) = sessions[(task * 31 + i) % CONNECTIONS];
                                let result = controller.handle_frame_ack(owner, id, 0, 1).await;
                                assert!(result.is_ok(), "{result:?}");
                            }
                        })
                    });
                    for joined in join_all(workers).await {
                        joined.expect("ack task");
                    }
                })
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_create_session,
    bench_spawn_blocking,
    bench_handle_frame_ack
);
criterion_main!(benches);
