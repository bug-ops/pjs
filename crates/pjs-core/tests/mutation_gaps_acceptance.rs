//! Tests for cheap gaps found while validating #747 and #751: reconstructor
//! errors on missing paths, buffer-pool accounting when a bucket is full or
//! holds mixed fresh and expired buffers, and the budget under concurrency.

use std::thread::{scope, sleep};
use std::time::Duration;

use pjson_rs::Error;
use pjson_rs::config::{SecurityConfig, Validate};
use pjson_rs::domain::value_objects::{JsonPath, Priority};
use pjson_rs::error::PatchError;
use pjson_rs::parser::{BufferPool, BufferSize, PooledBuffer};
use pjson_rs::stream::JsonReconstructor;
use pjson_rs::stream::patch::WirePatch;
use pjson_rs::stream::priority::{JsonPatch, PriorityStreamFrame};
use serde_json::{Map, Value, json};

const LARGE: usize = BufferSize::Large as usize;
const XLARGE: usize = BufferSize::XLarge as usize;

fn pool(total: usize, ttl_secs: u64, per_bucket: usize) -> BufferPool {
    let mut config = SecurityConfig::default();
    config.buffers.max_total_memory = total;
    config.buffers.max_buffer_size = total;
    config.buffers.buffer_ttl_secs = ttl_secs;
    config.buffers.max_buffers_per_bucket = per_bucket;
    config.sessions.max_session_data_size = total;
    BufferPool::with_security_config(config.validated().expect("consistent config"))
}

fn patch_error(wire: WirePatch) -> (PatchError, Value) {
    let mut reconstructor = JsonReconstructor::new();
    reconstructor.add_frame(PriorityStreamFrame::Skeleton {
        data: json!({"a": 1}),
        priority: Priority::CRITICAL,
        complete: false,
    });
    reconstructor.add_frame(PriorityStreamFrame::Patch {
        patches: vec![JsonPatch {
            patch: wire,
            priority: Priority::HIGH,
        }],
        priority: Priority::HIGH,
    });
    match reconstructor.process_all_frames() {
        Err(Error::Patch(error)) => (error, reconstructor.current_state().clone()),
        other => panic!("expected Error::Patch, got {other:?}"),
    }
}

#[test]
fn reconstructor_reports_key_not_found_for_merge_and_append_str_on_a_missing_path() {
    let missing = JsonPath::new("$.missing").unwrap();
    let rows = [
        WirePatch::Merge {
            path: missing.clone(),
            entries: Map::new(),
        },
        WirePatch::AppendStr {
            path: missing,
            text: "x".to_owned(),
        },
    ];
    for wire in rows {
        let (error, state) = patch_error(wire);
        assert_eq!(error, PatchError::KeyNotFound("missing".to_owned()));
        assert_eq!(state, json!({"a": 1}));
    }
}

#[test]
fn a_full_bucket_frees_the_class_bytes_of_the_buffers_it_cannot_keep() {
    let pool = pool(8 * LARGE, 300, 1);
    let held: Vec<PooledBuffer> = (0..3)
        .map(|_| pool.acquire(BufferSize::Large).unwrap())
        .collect();
    assert_eq!(pool.current_memory_usage(), 3 * LARGE);

    drop(held);

    assert_eq!(
        pool.current_memory_usage(),
        LARGE,
        "only one buffer is retained"
    );
}

#[test]
fn cleanup_frees_only_the_expired_buffers() {
    let pool = pool(8 * LARGE, 1, 10);
    let old = pool.acquire(BufferSize::Large).unwrap();
    let fresh = pool.acquire(BufferSize::Large).unwrap();
    drop(old);
    sleep(Duration::from_millis(1200));
    drop(fresh);

    let cleaned = pool.cleanup().unwrap();

    assert_eq!(cleaned.freed_buffers, 1);
    assert_eq!(pool.current_memory_usage(), LARGE);
}

#[test]
fn concurrent_acquires_never_exceed_the_budget() {
    let pool = pool(8 * XLARGE, 300, 10);

    let held: Vec<PooledBuffer> = scope(|s| {
        let handles: Vec<_> = (0..16)
            .map(|_| s.spawn(|| pool.acquire(BufferSize::XLarge).ok()))
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().unwrap())
            .collect()
    });

    assert_eq!(held.len(), 8);
    assert_eq!(pool.current_memory_usage(), 8 * XLARGE);
}
