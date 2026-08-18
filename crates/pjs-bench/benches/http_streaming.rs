//! HTTP streaming benchmarks for `infrastructure::http::streaming`.
//!
//! # Answer to #514
//!
//! sonic_rs is consistently faster than serde_json at the serialization
//! primitive #510 swapped (~1.4x-1.6x for one call per frame, ~1.7x-1.8x for
//! one call over a whole batch — see `http_streaming_serialization_*`
//! below), but that primitive is only ~18-19% of the actual production
//! route's per-request time (`http_streaming_batch_frame_stream_e2e`,
//! `json_batch_1`) — the rest is `frame_to_value`'s serde_json-based prep
//! (unchanged by #510) plus stream/async overhead. Substituting the
//! serde_json arm's cost back into the end-to-end total puts sonic_rs's
//! actual contribution on the production route at roughly **9-11%**,
//! consistently across `frame_count` in `[10, 100, 1000]` — not the
//! 1.6x-6.7x (single-frame) / 1.07x-4x (batch) #510 originally claimed for
//! the serializer call in isolation. Both figures are real and both
//! matter: the serializer-primitive ratio is what #510 measured, the
//! end-to-end percentage is what production users experience; see the
//! `[Unreleased]` CHANGELOG entry for this PR for the specific numbers from
//! the reference run. These percentages are from one reference run on this
//! machine and should be read as order-of-magnitude, not exact — see the
//! stability note below.
//!
//! # Structure
//!
//! Two independent benchmark groups answer two different questions; do not
//! conflate their numbers.
//!
//! - `http_streaming_serialization_*` isolates the primitive #510 actually
//!   changed (`sonic_rs::to_vec` replacing `serde_json::to_vec`) from
//!   everything else in the pipeline, and is the arm that can verify or
//!   correct #510's claim per #514. It compares both call shapes production
//!   code exercises: `format_batch_owned`'s `Json`/`NdJson`/`ServerSentEvents`
//!   branches call the serializer once *per frame* regardless of
//!   `batch_size` (`crates/pjs-core/src/infrastructure/http/streaming.rs:172-188`),
//!   while its `Binary` branch calls it once over the *whole batch*
//!   (streaming.rs:189) — "many small calls" vs "one big call" below mirror
//!   exactly those two shapes. Note that the live route normalizes `Binary`
//!   away to `Json` (`handlers/streams.rs`), so `one_big_call` adjudicates
//!   #510's original batch-regime claim but is not itself a route-reachable
//!   call shape — `many_small_calls` is the one that matches what the route
//!   actually calls. Values are built once outside the timed closure so
//!   `frame_to_value`'s own `serde_json`-based prep cost (a `format!`, an
//!   RFC3339 allocation, a `Map` build — all unchanged by #510) does not
//!   dilute the serializer comparison.
//! - `http_streaming_batch_frame_stream_e2e` measures the actual production
//!   call chain (`BatchFrameStream::into_stream()`, matching the route at
//!   `infrastructure::http::handlers::streams.rs:294`) as a regression
//!   baseline, not a serializer micro-comparison. Its `batch_size` axis
//!   changes stream-yield/allocation amortization only — for `Json`/
//!   `ServerSentEvents` it does *not* change sonic_rs's per-call payload
//!   size (see above), so it cannot and does not speak to #510's claim on
//!   its own. This group carries a higher `noise_threshold` than criterion's
//!   0.01 default (see `bench_batch_frame_stream_e2e`) because its
//!   per-iteration `Vec<Frame>` clone is not free at `frame_count = 1000`;
//!   the `http_streaming_serialization_*` groups are the better choice for
//!   `>10%` P1 gating under `.claude/rules/continuous-improvement.md`, except
//!   at their own fastest, few-µs `frame_count = 10` cells — see Stability
//!   below.
//!
//! # Stability
//!
//! This machine is shared across concurrent sessions/worktrees, and observed
//! run-to-run drift varies with that load: a quiet run shows single-digit-%
//! drift almost everywhere, but a contended run has shown drift past 30% on
//! individual cells (most often the fastest, few-µs `frame_count = 10`
//! serialization cells, which are inherently more sensitive to system
//! jitter at that timescale regardless of group). Treat the ratio and
//! percentage figures above as order-of-magnitude and re-run locally before
//! relying on a specific number; they are not a guarantee of a fixed
//! stability percentage under load.
//!
//! `NdJson` is skipped everywhere: `format_batch_owned` produces byte-identical
//! output to `Json`, differing only in `Content-Type` (streaming.rs:162-163,
//! `BatchFrameStream::content_type`), so it would only duplicate the `Json`
//! numbers. `create_streaming_response`'s `axum::body::Body::from_stream`
//! wrapping and hyper's HTTP framing are out of scope for both groups — that
//! layer adds transport-only overhead unrelated to the JSON serialization or
//! streaming logic measured here.
//!
//! Fixture shape (`make_frames`) is versioned by this file's content: a
//! future change to it breaks direct comparison against previously recorded
//! `target/criterion/` baselines. Save a baseline before changing the
//! fixture (`cargo bench -p pjs-bench --bench http_streaming -- --save-baseline <name>`)
//! and compare with `--baseline <name>` afterward.

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use futures::{StreamExt, stream};
use pjson_rs::domain::entities::Frame;
use pjson_rs::domain::value_objects::{JsonData, StreamId};
use pjson_rs::infrastructure::http::streaming::{BatchFrameStream, StreamFormat};
use std::collections::HashMap;
use std::hint::black_box;
use std::time::Duration;

const FRAME_COUNTS: [usize; 3] = [10, 100, 1000];

/// Frames deliberately carry escape-requiring text, unicode, and a float per
/// frame, so this fixture avoids the easiest/most favorable input for
/// sonic_rs (uniform plain ASCII, no escapes, no floats) rather than the
/// opposite: whether escape-dense strings help or hurt sonic_rs's relative
/// margin over serde_json is not established by this file — SIMD escape
/// scanning bulk-copies the run *before* the next character needing escape,
/// so a denser escape rate shortens those runs and could plausibly narrow
/// sonic_rs's edge rather than widen it. No causal claim is made either way.
fn make_frames(n: usize) -> Vec<Frame> {
    let stream_id = StreamId::new();
    (0..n)
        .map(|i| {
            let payload = JsonData::Object(HashMap::from([
                ("id".to_string(), JsonData::Integer(i as i64)),
                (
                    "name".to_string(),
                    JsonData::String(format!("Itëm \"{i}\" — ünïcödé\n\ttab")),
                ),
                (
                    "email".to_string(),
                    JsonData::String(format!("user.{i}+tag@exämple.com")),
                ),
                (
                    "status".to_string(),
                    JsonData::String(if i % 2 == 0 { "active" } else { "inactive" }.to_string()),
                ),
                ("active".to_string(), JsonData::Bool(i % 2 == 0)),
                (
                    "score".to_string(),
                    JsonData::Float(i as f64 * 1.2345 + 0.6789),
                ),
                (
                    "tags".to_string(),
                    JsonData::Array(vec![
                        JsonData::String("plain".to_string()),
                        JsonData::String("back\\slash".to_string()),
                        JsonData::String("ünïcödé™".to_string()),
                    ]),
                ),
            ]));
            Frame::skeleton(stream_id, i as u64, payload)
        })
        .collect()
}

/// Bench-local mirror of `infrastructure::http::streaming`'s private
/// `frame_to_value` — duplicated here (not exported by `pjs-core`) so the
/// serialization benchmarks below exercise the exact same value shape
/// production code serializes, without pulling `frame_to_value`'s own prep
/// cost inside the timed region.
///
/// KEEP IN SYNC WITH `crates/pjs-core/src/infrastructure/http/streaming.rs:127`
/// (`frame_to_value`) — a field added there with no matching change here
/// silently leaves this bench measuring a stale, smaller value shape.
fn frame_to_value(frame: &Frame) -> serde_json::Value {
    serde_json::json!({
        "type": format!("{:?}", frame.frame_type()),
        "priority": frame.priority().value(),
        "sequence": frame.sequence(),
        "timestamp": frame.timestamp().to_rfc3339(),
        "payload": frame.payload(),
        "metadata": frame.metadata()
    })
}

fn many_small_sonic(values: &[serde_json::Value]) {
    for v in values {
        black_box(sonic_rs::to_vec(v).unwrap());
    }
}

fn many_small_serde(values: &[serde_json::Value]) {
    for v in values {
        black_box(serde_json::to_vec(v).unwrap());
    }
}

fn one_big_sonic(values: &[serde_json::Value]) {
    black_box(sonic_rs::to_vec(values).unwrap());
}

fn one_big_serde(values: &[serde_json::Value]) {
    black_box(serde_json::to_vec(values).unwrap());
}

/// Isolated sonic_rs-vs-serde_json comparison, "many small calls" shape
/// (mirrors `format_batch_owned`'s `Json`/`NdJson`/`ServerSentEvents`
/// branches: one serializer call per frame).
fn bench_serialization_many_small_calls(c: &mut Criterion) {
    let mut group = c.benchmark_group("http_streaming_serialization_many_small_calls");

    for frame_count in FRAME_COUNTS {
        let frames = make_frames(frame_count);
        let values: Vec<serde_json::Value> = frames.iter().map(frame_to_value).collect();
        let bytes: u64 = values
            .iter()
            .map(|v| serde_json::to_vec(v).unwrap().len() as u64)
            .sum();
        let sonic_bytes: u64 = values
            .iter()
            .map(|v| sonic_rs::to_vec(v).unwrap().len() as u64)
            .sum();
        assert_eq!(
            bytes, sonic_bytes,
            "sonic_rs/serde_json byte length diverged at frame_count={frame_count} — \
             Throughput::Bytes below is computed from serde_json's length for both arms \
             and would silently misreport the sonic_rs arm's rate if this ever fails"
        );
        group.throughput(Throughput::Bytes(bytes));

        group.bench_with_input(
            BenchmarkId::new("sonic_rs", frame_count),
            &values,
            |b, v| {
                b.iter(|| many_small_sonic(v));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("serde_json", frame_count),
            &values,
            |b, v| {
                b.iter(|| many_small_serde(v));
            },
        );
    }
    group.finish();
}

/// Isolated sonic_rs-vs-serde_json comparison, "one big call" shape (mirrors
/// `format_batch_owned`'s `Binary` branch: a single serializer call over the
/// whole batch — the regime a `batch_size = 100` sweep would need to reach
/// sonic_rs's stronger amortization per #510's own caveat).
fn bench_serialization_one_big_call(c: &mut Criterion) {
    let mut group = c.benchmark_group("http_streaming_serialization_one_big_call");

    for frame_count in FRAME_COUNTS {
        let frames = make_frames(frame_count);
        let values: Vec<serde_json::Value> = frames.iter().map(frame_to_value).collect();
        let bytes = serde_json::to_vec(&values).unwrap().len() as u64;
        let sonic_bytes = sonic_rs::to_vec(&values).unwrap().len() as u64;
        assert_eq!(
            bytes, sonic_bytes,
            "sonic_rs/serde_json byte length diverged at frame_count={frame_count} — \
             Throughput::Bytes below is computed from serde_json's length for both arms \
             and would silently misreport the sonic_rs arm's rate if this ever fails"
        );
        group.throughput(Throughput::Bytes(bytes));

        group.bench_with_input(
            BenchmarkId::new("sonic_rs", frame_count),
            &values,
            |b, v| {
                b.iter(|| one_big_sonic(v));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("serde_json", frame_count),
            &values,
            |b, v| {
                b.iter(|| one_big_serde(v));
            },
        );
    }
    group.finish();
}

/// End-to-end `BatchFrameStream::into_stream()` regression baseline —
/// the production call chain, not a serializer micro-comparison (see module
/// doc). `frames.clone()` and the fresh `stream::iter` are built inside
/// `iter_batched`'s untimed setup so per-iteration allocation noise isn't
/// counted in the sample; `BatchSize::PerIteration` (not `SmallInput`) keeps
/// only one clone alive at a time instead of pre-cloning a whole batch of
/// iterations up front, which otherwise perturbs allocator state inside the
/// timed region at `frame_count = 1000` (tens of MB of clones alive
/// simultaneously under `SmallInput`). `noise_threshold` is raised above
/// criterion's 0.01 default because that per-iteration clone still isn't
/// free at 1000 frames — this group is a regression baseline, not a
/// regression *gate*; use the `http_streaming_serialization_*` groups (no
/// per-iteration allocation, so typically tighter drift outside of the
/// fastest few-µs cells — see the module doc's Stability section) for
/// `>10%` P1 gating per `.claude/rules/continuous-improvement.md`.
fn bench_batch_frame_stream_e2e(c: &mut Criterion) {
    let mut group = c.benchmark_group("http_streaming_batch_frame_stream_e2e");
    group.measurement_time(Duration::from_secs(8));
    group.noise_threshold(0.15);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    for frame_count in FRAME_COUNTS {
        let frames = make_frames(frame_count);
        group.throughput(Throughput::Elements(frame_count as u64));

        for (format_name, format) in [
            ("json", StreamFormat::Json),
            ("sse", StreamFormat::ServerSentEvents),
        ] {
            for batch_size in [1usize, 100usize] {
                group.bench_with_input(
                    BenchmarkId::new(format!("{format_name}_batch_{batch_size}"), frame_count),
                    &frames,
                    |b, frames| {
                        b.iter_batched(
                            || frames.clone(),
                            |frames| {
                                rt.block_on(async {
                                    let batch = BatchFrameStream::new(
                                        stream::iter(frames),
                                        format,
                                        batch_size,
                                    );
                                    black_box(batch.into_stream().collect::<Vec<_>>().await)
                                })
                            },
                            BatchSize::PerIteration,
                        );
                    },
                );
            }
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_serialization_many_small_calls,
    bench_serialization_one_big_call,
    bench_batch_frame_stream_e2e,
);
criterion_main!(benches);
