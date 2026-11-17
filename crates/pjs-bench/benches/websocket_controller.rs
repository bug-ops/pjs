//! Benchmarks for `AdaptiveStreamController` (#664).
//!
//! - `create_session`: cost of planning a document of 1 KiB to 1 MiB, the input
//!   for the `spawn_blocking` threshold of #658. Sizes the gate sends to the
//!   blocking pool (the 32 KiB document since #683, and 64 KiB and up) include
//!   the blocking-pool hop (6-10 us); their inline cost is the pre-#658 figure
//!   in spec 066 and spec 075 FR-008.
//! - `create_session_shapes`: the seven entry-dense shapes that calibrate the
//!   offload gate (#683, #686), each sized just under the pre-#683 gate cost of
//!   32768 byte-equivalents so that all of them plan inline in the baseline.
//! - `spawn_blocking_round_trip`: fixed cost of the blocking-pool hop.
//! - `handle_frame_ack`: throughput of a per-frame global-lock operation on
//!   500 connections from 1, 4, 8 and 16 concurrent tasks, the proxy for the
//!   per-frame `release_sent` write lock.

use std::{
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use criterion::{
    BenchmarkGroup, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
    measurement::WallTime,
};
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
/// Pre-#683 gate cost the calibration shapes are sized under.
const GATE_COST: usize = 32 * 1024;
/// How far below `GATE_COST` an exactly-sized shape may serialize.
const SHAPE_SIZE_WINDOW: usize = 64;

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

/// Times `create_session` of `data` for one benchmark case.
fn bench_plan_case(
    group: &mut BenchmarkGroup<'_, WallTime>,
    rt: &Runtime,
    id: BenchmarkId,
    data: &Value,
) {
    group.bench_with_input(id, data, |b, data| {
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

fn bench_create_session(c: &mut Criterion) {
    let mut group = c.benchmark_group("ws_controller_create_session");
    let rt = current_thread();
    for bytes in DOCUMENT_BYTES {
        group.throughput(Throughput::Bytes(bytes as u64));
        bench_plan_case(
            &mut group,
            &rt,
            BenchmarkId::from_parameter(bytes),
            &document(bytes),
        );
    }
    group.finish();
}

fn serialized_len(value: &Value) -> usize {
    serde_json::to_vec(value).expect("serializable").len()
}

/// Flat object of `n` entries `k{i:04}`, 7 bytes serialized as a key.
fn object_of(n: usize, value: impl Fn(usize) -> Value) -> Value {
    Value::Object((0..n).map(|i| (format!("k{i:04}"), value(i))).collect())
}

fn array_of(n: usize, value: &Value) -> Value {
    Value::Array(vec![value.clone(); n])
}

/// Largest `n` for which the flat object of `i % 1000` integers serializes
/// within the pre-#683 gate cost.
fn small_ints_count() -> usize {
    let mut size = 2;
    let mut n = 0;
    loop {
        let unit = 7 + 1 + (n % 1000).to_string().len() + usize::from(n > 0);
        if size + unit > GATE_COST {
            return n;
        }
        size += unit;
        n += 1;
    }
}

/// The calibration shapes; the flag says whether the serialized size is exact
/// (`true`) or only an upper bound because a float's digits are not counted
/// by the pre-#683 gate cost.
fn calibration_shapes() -> Vec<(&'static str, Value, bool)> {
    vec![
        ("short_strings", object_of(2730, |_| json!("v")), true),
        (
            "small_ints",
            object_of(small_ints_count(), |i| json!(i % 1000)),
            true,
        ),
        (
            "long_floats",
            object_of(2047, |_| json!(0.123_456_789_012_345)),
            false,
        ),
        ("short_floats", object_of(2047, |_| json!(0.5)), false),
        ("escape_strings", array_of(3640, &json!("\u{1}")), true),
        ("nested_arrays", array_of(8191, &json!([0])), true),
        ("small_objects", array_of(4095, &json!({"a": 0})), true),
    ]
}

fn bench_create_session_shapes(c: &mut Criterion) {
    let mut group = c.benchmark_group("ws_controller_create_session_shapes");
    let rt = current_thread();
    for (name, data, exact) in calibration_shapes() {
        let bytes = serialized_len(&data);
        if exact {
            assert!(
                (GATE_COST - SHAPE_SIZE_WINDOW..=GATE_COST).contains(&bytes),
                "{name}: {bytes} bytes outside the sizing window"
            );
        }
        group.throughput(Throughput::Bytes(bytes as u64));
        bench_plan_case(&mut group, &rt, BenchmarkId::new(name, bytes), &data);
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
    bench_create_session_shapes,
    bench_spawn_blocking,
    bench_handle_frame_ack
);
criterion_main!(benches);
