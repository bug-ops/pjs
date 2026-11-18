//! Acceptance tests for #747: `max_total_memory` bounds the bytes a `BufferPool`
//! hands out plus the bytes it retains, `PoolStats` memory counters are real,
//! and sizing past the largest class is an error instead of a silent clamp.

use std::thread::sleep;
use std::time::Duration;

use pjson_rs::config::{SecurityConfig, Validate};
use pjson_rs::domain::DomainError;
use pjson_rs::parser::{BufferPool, BufferSize, PoolConfig, PooledBuffer};

const SMALL: usize = BufferSize::Small as usize;
const LARGE: usize = BufferSize::Large as usize;
const XLARGE: usize = BufferSize::XLarge as usize;
const HUGE: usize = BufferSize::Huge as usize;

fn pool_with(total: usize, ttl_secs: u64) -> BufferPool {
    let mut config = SecurityConfig::default();
    config.buffers.max_total_memory = total;
    config.buffers.max_buffer_size = total;
    config.buffers.buffer_ttl_secs = ttl_secs;
    config.sessions.max_session_data_size = total;
    BufferPool::with_security_config(config.validated().expect("consistent config"))
}

fn live(pool: &BufferPool) -> usize {
    pool.current_memory_usage()
}

#[test]
fn acquire_bounds_the_buffers_held_outstanding() {
    let pool = pool_with(4 * XLARGE, 300);
    let held: Vec<PooledBuffer> = (0..4)
        .map(|_| pool.acquire(BufferSize::XLarge).expect("within budget"))
        .collect();
    assert_eq!(live(&pool), 4 * XLARGE);

    let refused = pool.acquire(BufferSize::XLarge);

    assert!(
        matches!(refused, Err(DomainError::ResourceExhausted(_))),
        "{:?}",
        refused.as_ref().err()
    );
    assert_eq!(
        live(&pool),
        4 * XLARGE,
        "a refused acquire must not change the live bytes"
    );
    drop(held);
}

#[test]
fn acquire_on_the_low_memory_preset_fails_at_the_first_buffer_over_the_limit() {
    let pool = BufferPool::with_config(PoolConfig::low_memory());
    let mut held = Vec::new();

    let failure = loop {
        match pool.acquire(BufferSize::XLarge) {
            Ok(buffer) => held.push(buffer),
            Err(error) => break error,
        }
        assert!(held.len() <= 200, "the limit was never enforced");
    };

    assert!(
        matches!(failure, DomainError::ResourceExhausted(_)),
        "{failure:?}"
    );
    assert_eq!(
        held.len(),
        100,
        "50 MiB holds exactly 100 buffers of 512 KiB"
    );
    assert_eq!(live(&pool), 100 * XLARGE);
}

#[test]
fn dropping_a_buffer_retains_it_and_reuse_keeps_the_live_bytes() {
    let pool = pool_with(2 * LARGE, 300);
    let first = pool.acquire(BufferSize::Large).unwrap();
    let second = pool.acquire(BufferSize::Large).unwrap();
    assert_eq!(live(&pool), 2 * LARGE);

    drop(first);
    assert_eq!(
        live(&pool),
        2 * LARGE,
        "an idle buffer still counts as live"
    );

    let reused = pool
        .acquire(BufferSize::Large)
        .expect("reuse never fails on the budget");
    assert_eq!(live(&pool), 2 * LARGE);
    assert_eq!(pool.stats().unwrap().cache_hits, 1);
    drop((second, reused));
}

#[test]
fn acquire_drop_acquire_is_a_cache_hit_on_every_preset() {
    let presets = [
        PoolConfig::default(),
        PoolConfig::simd_optimized(),
        PoolConfig::low_memory(),
        PoolConfig::development(),
    ];
    for config in presets {
        let pool = BufferPool::with_config(config.clone());
        let first = pool.acquire(BufferSize::Large).unwrap();
        let held_elsewhere = pool.acquire(BufferSize::Large).unwrap();

        drop(first);
        let retained = live(&pool);
        let again = pool.acquire(BufferSize::Large).unwrap();

        assert_eq!(
            retained,
            2 * LARGE,
            "{config:?}: the dropped buffer must be retained"
        );
        assert_eq!(
            live(&pool),
            2 * LARGE,
            "{config:?}: reuse must not change the live bytes"
        );
        drop((held_elsewhere, again));
    }
}

#[test]
fn a_buffer_grown_while_checked_out_is_freed_on_drop() {
    let pool = pool_with(8 * 1024 * 1024, 300);
    let mut buffer = pool.acquire(BufferSize::Large).unwrap();
    buffer
        .buffer_mut()
        .expect("buffer present")
        .reserve(1024 * 1024)
        .expect("grows");
    assert!(buffer.capacity() > LARGE);

    drop(buffer);

    assert_eq!(
        live(&pool),
        0,
        "a buffer that left its size class is not retained"
    );
    let _next = pool.acquire(BufferSize::Large).unwrap();
    let stats = pool.stats().unwrap();
    assert_eq!((stats.cache_hits, stats.cache_misses), (0, 2));
}

#[test]
fn pool_stats_report_real_memory_even_when_tracking_is_off() {
    let pool = BufferPool::with_config(PoolConfig::low_memory());
    let held: Vec<PooledBuffer> = (0..3)
        .map(|_| pool.acquire(BufferSize::XLarge).unwrap())
        .collect();

    let stats = pool.stats().unwrap();
    assert_eq!(stats.current_memory_usage, 3 * XLARGE);
    assert_eq!(stats.peak_memory_usage, 3 * XLARGE);

    drop(held);
    let stats = pool.stats().unwrap();
    assert_eq!(stats.current_memory_usage, live(&pool));
    assert_eq!(stats.peak_memory_usage, 3 * XLARGE);
}

#[test]
fn cleanup_releases_the_live_bytes_of_expired_buffers() {
    let pool = pool_with(8 * XLARGE, 1);
    let held: Vec<PooledBuffer> = (0..2)
        .map(|_| pool.acquire(BufferSize::XLarge).unwrap())
        .collect();
    drop(held);
    assert_eq!(live(&pool), 2 * XLARGE);

    sleep(Duration::from_millis(1200));
    let cleaned = pool.cleanup().unwrap();

    assert_eq!(cleaned.freed_buffers, 2);
    assert_eq!(live(&pool), 0);
    let stats = pool.stats().unwrap();
    assert_eq!(stats.current_memory_usage, 0);
    assert_eq!(stats.peak_memory_usage, 2 * XLARGE);
}

#[test]
fn for_capacity_picks_the_smallest_class_and_refuses_beyond_the_largest() {
    let rows = [
        (0, Some(BufferSize::Small)),
        (SMALL, Some(BufferSize::Small)),
        (SMALL + 1, Some(BufferSize::Medium)),
        (8192, Some(BufferSize::Medium)),
        (8193, Some(BufferSize::Large)),
        (LARGE, Some(BufferSize::Large)),
        (LARGE + 1, Some(BufferSize::XLarge)),
        (XLARGE, Some(BufferSize::XLarge)),
        (XLARGE + 1, Some(BufferSize::Huge)),
        (HUGE, Some(BufferSize::Huge)),
        (HUGE + 1, None),
        (usize::MAX, None),
    ];
    for (capacity, expected) in rows {
        assert_eq!(
            BufferSize::for_capacity(capacity),
            expected,
            "capacity {capacity}"
        );
    }
}

#[test]
fn acquire_with_capacity_errors_instead_of_clamping_to_the_largest_class() {
    let pool = BufferPool::new();

    let refused = pool.acquire_with_capacity(HUGE + 1);
    assert!(
        matches!(refused, Err(DomainError::InvalidInput(_))),
        "{:?}",
        refused.as_ref().err()
    );

    let fitted = pool.acquire_with_capacity(5000).expect("fits Medium");
    assert!(fitted.capacity() >= 5000);
}
