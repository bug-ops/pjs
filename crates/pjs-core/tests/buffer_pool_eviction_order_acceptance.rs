//! Acceptance tests for #773 (track A, coverage gap): `BufferPool::acquire`
//! evicts only the idle bytes it needs, largest size class first, and keeps
//! the smaller idle classes for reuse.

use pjson_rs::config::{SecurityConfig, Validate};
use pjson_rs::parser::{BufferPool, BufferSize, PooledBuffer};

const MEDIUM: usize = BufferSize::Medium as usize;
const LARGE: usize = BufferSize::Large as usize;
const XLARGE: usize = BufferSize::XLarge as usize;

fn pool(budget: usize) -> BufferPool {
    let mut config = SecurityConfig::default();
    config.buffers.max_total_memory = budget;
    config.buffers.max_buffer_size = budget;
    config.buffers.buffer_ttl_secs = 300;
    config.sessions.max_session_data_size = budget;
    BufferPool::with_security_config(config.validated().expect("consistent config"))
}

fn hold(pool: &BufferPool, size: BufferSize, count: usize) -> Vec<PooledBuffer> {
    (0..count)
        .map(|_| pool.acquire(size).expect("within budget"))
        .collect()
}

#[test]
fn acquire_evicts_exactly_the_buffers_needed_to_fit() {
    let pool = pool(4 * XLARGE);
    drop(hold(&pool, BufferSize::XLarge, 4));
    assert_eq!(pool.current_memory_usage(), 4 * XLARGE);

    let large = pool
        .acquire(BufferSize::Large)
        .expect("fits after eviction");

    assert_eq!(pool.current_memory_usage(), 3 * XLARGE + LARGE);
    drop(large);
}

#[test]
fn acquire_evicts_the_largest_idle_class_before_smaller_ones() {
    let pool = pool(2 * XLARGE);
    drop(hold(&pool, BufferSize::XLarge, 1));
    drop(hold(&pool, BufferSize::Large, XLARGE / LARGE));
    assert_eq!(pool.current_memory_usage(), 2 * XLARGE);

    let medium = pool
        .acquire(BufferSize::Medium)
        .expect("fits after eviction");

    // The one XLarge went; every idle Large stayed.
    assert_eq!(pool.current_memory_usage(), XLARGE + MEDIUM);
    drop(medium);
    let before = pool.current_memory_usage();
    let reused = hold(&pool, BufferSize::Large, XLARGE / LARGE);
    assert_eq!(
        pool.current_memory_usage(),
        before,
        "the idle Large buffers were kept, so reusing them allocates nothing"
    );
    drop(reused);
}

#[test]
fn acquire_evicts_nothing_when_the_new_buffer_already_fits() {
    let pool = pool(4 * XLARGE);
    drop(hold(&pool, BufferSize::XLarge, 2));

    let large = pool.acquire(BufferSize::Large).expect("fits");

    assert_eq!(pool.current_memory_usage(), 2 * XLARGE + LARGE);
    drop(large);
}
