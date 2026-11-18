//! Acceptance tests for #773 (track A): `BufferPool::acquire` evicts idle
//! buffers before it fails on the budget, so one size class cannot starve the
//! others. It still fails when checked-out buffers alone fill the budget.

use pjson_rs::config::{SecurityConfig, Validate};
use pjson_rs::domain::DomainError;
use pjson_rs::parser::{BufferPool, BufferSize, PooledBuffer};

const SMALL: usize = BufferSize::Small as usize;
const LARGE: usize = BufferSize::Large as usize;
const XLARGE: usize = BufferSize::XLarge as usize;
const BUDGET: usize = 4 * XLARGE;

fn pool() -> BufferPool {
    let mut config = SecurityConfig::default();
    config.buffers.max_total_memory = BUDGET;
    config.buffers.max_buffer_size = BUDGET;
    config.buffers.buffer_ttl_secs = 300;
    config.sessions.max_session_data_size = BUDGET;
    BufferPool::with_security_config(config.validated().expect("consistent config"))
}

fn hold(pool: &BufferPool, size: BufferSize, count: usize) -> Vec<PooledBuffer> {
    (0..count)
        .map(|_| pool.acquire(size).expect("within budget"))
        .collect()
}

#[test]
fn acquire_evicts_idle_buffers_of_another_class_instead_of_failing() {
    let pool = pool();
    drop(hold(&pool, BufferSize::XLarge, 4));
    assert_eq!(
        pool.current_memory_usage(),
        BUDGET,
        "the idle buffers fill the whole budget"
    );

    let small = pool.acquire(BufferSize::Small);

    assert!(small.is_ok(), "{:?}", small.as_ref().err());
    assert!(pool.current_memory_usage() <= BUDGET);
    assert!(pool.current_memory_usage() >= SMALL);
}

#[test]
fn acquire_evicts_idle_bytes_but_keeps_checked_out_buffers() {
    let pool = pool();
    let mut buffers = hold(&pool, BufferSize::XLarge, 4);
    drop(buffers.split_off(3));
    assert_eq!(
        pool.current_memory_usage(),
        BUDGET,
        "one idle buffer fills the rest of the budget"
    );

    let large = pool
        .acquire(BufferSize::Large)
        .expect("idle bytes are reclaimable");

    assert_eq!(buffers.len(), 3, "checked-out buffers are never evicted");
    assert!(pool.current_memory_usage() <= BUDGET);
    assert!(pool.current_memory_usage() >= 3 * XLARGE + LARGE);
    drop(large);
}

#[test]
fn acquire_still_fails_when_checked_out_buffers_fill_the_budget() {
    let pool = pool();
    let held = hold(&pool, BufferSize::XLarge, 4);

    let refused = pool.acquire(BufferSize::Small);

    assert!(
        matches!(refused, Err(DomainError::ResourceExhausted(_))),
        "{:?}",
        refused.as_ref().err()
    );
    assert_eq!(pool.current_memory_usage(), BUDGET);
    drop(held);
}
