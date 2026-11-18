//! Acceptance tests for #755/#756: allocation budgets of the priority heuristic
//! and of domain patch extraction. A counting global allocator (per thread, so
//! parallel tests do not disturb each other) measures the work inside a closure.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use pjson_rs_domain::Stream;
use pjson_rs_domain::entities::frame::FramePatch;
use pjson_rs_domain::entities::stream::StreamConfig;
use pjson_rs_domain::services::{PriorityHeuristicConfig, ValueShape, compute_priority};
use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority, SessionId};
use serde_json::{Map, Value, json};

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

// SAFETY: every method forwards unchanged to `System`, which upholds the `GlobalAlloc` contract.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        // SAFETY: the caller upholds the `GlobalAlloc::alloc` contract for `layout`, and `System` is the underlying allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` come from a matching `alloc` of this allocator, and `System` is the underlying allocator.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        // SAFETY: `ptr` and `layout` come from this allocator and the caller guarantees `new_size`, and `System` is the underlying allocator.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn count_allocations<R>(work: impl FnOnce() -> R) -> (usize, R) {
    let before = ALLOCATIONS.with(Cell::get);
    let result = work();
    let after = ALLOCATIONS.with(Cell::get);
    (after - before, result)
}

#[test]
fn compute_priority_allocates_nothing() {
    let cfg = PriorityHeuristicConfig::default();
    let paths = [
        JsonPath::new("$.SomeUnknownFieldName").unwrap(),
        JsonPath::new("$.Description").unwrap(),
        JsonPath::new("$.USER_ID").unwrap(),
        JsonPath::new("$.a.b.c.d.e.f.g").unwrap(),
    ];
    let shapes = [
        ValueShape::Scalar,
        ValueShape::String { len: 2000 },
        ValueShape::Array { len: 500 },
    ];

    let (allocations, ()) = count_allocations(|| {
        for path in &paths {
            for shape in shapes {
                black_box(compute_priority(black_box(&cfg), black_box(path), shape));
            }
        }
    });

    assert_eq!(allocations, 0);
}

fn wide_stream(leaves: usize) -> Stream {
    let source: Map<String, Value> = (0..leaves)
        .map(|i| (format!("field_{i}"), json!(i)))
        .collect();
    let mut config = StreamConfig::default();
    for rule in ["$.a", "$.b", "$.c"] {
        config
            .priority_rules
            .insert(rule.to_owned(), Priority::HIGH);
    }
    let mut stream = Stream::new(
        SessionId::new(),
        JsonData::from(Value::Object(source)),
        config,
    );
    stream.start_streaming().unwrap();
    stream
}

#[test]
fn extract_prioritized_patches_builds_the_heuristic_once_not_per_leaf() {
    const LEAVES: usize = 1000;
    let stream = wide_stream(LEAVES);

    let (allocations, patches) = count_allocations(|| {
        stream
            .extract_prioritized_patches(Priority::BACKGROUND)
            .unwrap()
    });

    assert_eq!(patches.len(), LEAVES);
    // Per leaf: one key and one path vector for the emitted patch plus amortised growth.
    // Rebuilding the heuristic per leaf costs about 30 allocations each.
    assert!(
        allocations < LEAVES * 8,
        "{allocations} allocations for {LEAVES} leaves"
    );
}

#[test]
fn chunk_patches_for_commit_moves_patches_instead_of_cloning_them() {
    const PATCHES: usize = 1000;
    let patches: Vec<(FramePatch, Priority)> = (0..PATCHES)
        .map(|i| {
            let path = JsonPath::root().append_key(&format!("field_{i}"));
            (
                FramePatch::set(path, JsonData::String(format!("value-{i:08}"))),
                Priority::MEDIUM,
            )
        })
        .collect();

    let (allocations, chunks) = count_allocations(|| Stream::chunk_patches_for_commit(patches, 10));

    assert_eq!(chunks.iter().map(|(_, c)| c.len()).sum::<usize>(), PATCHES);
    assert!(
        allocations < 100,
        "{allocations} allocations for {PATCHES} patches"
    );
}
