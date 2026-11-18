//! Acceptance test for #755: `PriorityAssigner::calculate_field_priority`
//! scores from the heuristic built once in `with_config`, so a call allocates
//! nothing. A counting global allocator (per thread) measures the calls.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use pjs_wasm::priority_assignment::{PriorityAssigner, PriorityConfig};
use pjson_rs_domain::value_objects::{JsonData, JsonPath};

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

#[test]
fn calculate_field_priority_allocates_nothing_per_call() {
    let mut config = PriorityConfig::default();
    config.add_critical_field("session_id".to_owned());
    let assigner = PriorityAssigner::with_config(config);
    let paths = [
        JsonPath::new("$.session_id").unwrap(),
        JsonPath::new("$.Description").unwrap(),
        JsonPath::new("$.SomeUnknownField").unwrap(),
    ];
    let value = JsonData::Integer(1);

    let before = ALLOCATIONS.with(Cell::get);
    for _ in 0..100 {
        for path in &paths {
            black_box(assigner.calculate_field_priority(black_box(path), black_box(&value)));
        }
    }
    let after = ALLOCATIONS.with(Cell::get);

    assert_eq!(after - before, 0);
}
