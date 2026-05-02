//! Arena-based memory allocation for high-performance JSON parsing
//!
//! Arena allocators provide excellent performance for scenarios where many objects
//! are allocated together and freed all at once, which is perfect for JSON parsing.

use std::{cell::RefCell, collections::HashMap};
use typed_arena::Arena;

/// Arena-backed string pool for zero-copy string operations
///
/// `StringArena` is `!Send` and `!Sync` because the `interned` map stores raw pointers
/// into arena-allocated memory. All access is single-threaded via `RefCell`.
pub struct StringArena {
    arena: Arena<String>,
    /// Key is the owned canonical string; value is a raw pointer into arena memory.
    interned: RefCell<HashMap<String, *const str>>,
    strings_allocated: RefCell<usize>,
    total_bytes: RefCell<usize>,
}

impl StringArena {
    /// Create new string arena
    pub fn new() -> Self {
        Self {
            arena: Arena::new(),
            interned: RefCell::new(HashMap::new()),
            strings_allocated: RefCell::new(0),
            total_bytes: RefCell::new(0),
        }
    }

    /// Allocate string in arena and return reference with arena lifetime
    pub fn alloc_str(&self, s: String) -> &str {
        self.arena.alloc(s).as_str()
    }

    /// Intern string to avoid duplicates (useful for JSON keys).
    ///
    /// The returned reference is valid for the lifetime of `&self`.
    pub fn intern<'a>(&'a self, s: &str) -> &'a str {
        if let Some(&ptr) = self.interned.borrow().get(s) {
            // SAFETY: The raw pointer was stored from a reference into `self.arena`.
            // `typed_arena::Arena<String>` never reallocates or frees individual elements
            // while the arena is alive, so the pointer remains valid for `'a` (the lifetime
            // of `&self`).
            return unsafe { &*ptr };
        }

        let allocated = self.alloc_str(s.to_string());
        *self.strings_allocated.borrow_mut() += 1;
        *self.total_bytes.borrow_mut() += allocated.len();
        self.interned
            .borrow_mut()
            .insert(s.to_string(), allocated as *const str);
        allocated
    }

    /// Get current memory usage statistics.
    ///
    /// `chunks_allocated` is always 1 because `typed_arena` does not expose its
    /// internal chunk count.
    pub fn memory_usage(&self) -> ArenaStats {
        ArenaStats {
            chunks_allocated: 1,
            total_bytes: *self.total_bytes.borrow(),
            strings_allocated: *self.strings_allocated.borrow(),
        }
    }
}

impl Default for StringArena {
    fn default() -> Self {
        Self::new()
    }
}

/// Arena for JSON value allocations
pub struct ValueArena<T> {
    arena: Arena<T>,
    allocated_count: RefCell<usize>,
}

impl<T> ValueArena<T> {
    /// Create new value arena
    pub fn new() -> Self {
        Self {
            arena: Arena::new(),
            allocated_count: RefCell::new(0),
        }
    }

    /// Allocate value in arena
    pub fn alloc(&self, value: T) -> &mut T {
        *self.allocated_count.borrow_mut() += 1;
        self.arena.alloc(value)
    }

    /// Allocate multiple values
    pub fn alloc_extend<I: IntoIterator<Item = T>>(&self, iter: I) -> &mut [T] {
        let values: Vec<T> = iter.into_iter().collect();
        *self.allocated_count.borrow_mut() += values.len();
        self.arena.alloc_extend(values)
    }

    /// Get count of allocated objects
    pub fn allocated_count(&self) -> usize {
        *self.allocated_count.borrow()
    }
}

impl<T> Default for ValueArena<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory usage statistics for arena
#[derive(Debug, Clone)]
pub struct ArenaStats {
    /// Number of internal chunks the arena holds (always 1 for `typed_arena`).
    pub chunks_allocated: usize,
    /// Total bytes consumed by allocated strings.
    pub total_bytes: usize,
    /// Number of distinct strings allocated.
    pub strings_allocated: usize,
}

/// Combined arena allocator for JSON parsing
pub struct JsonArena {
    /// Arena for string allocations
    pub strings: StringArena,
    /// Arena for object allocations  
    pub objects: ValueArena<serde_json::Map<String, serde_json::Value>>,
    /// Arena for array allocations
    pub arrays: ValueArena<Vec<serde_json::Value>>,
    /// Arena for generic values
    pub values: ValueArena<serde_json::Value>,
}

impl JsonArena {
    /// Create new JSON arena with all allocators
    pub fn new() -> Self {
        Self {
            strings: StringArena::new(),
            objects: ValueArena::new(),
            arrays: ValueArena::new(),
            values: ValueArena::new(),
        }
    }

    /// Get combined memory usage statistics
    pub fn stats(&self) -> CombinedArenaStats {
        CombinedArenaStats {
            string_stats: self.strings.memory_usage(),
            objects_allocated: self.objects.allocated_count(),
            arrays_allocated: self.arrays.allocated_count(),
            values_allocated: self.values.allocated_count(),
        }
    }

    /// Reset all arenas (drops all allocated memory)
    pub fn reset(&mut self) {
        // Drop and recreate arenas to free memory
        *self = Self::new();
    }
}

impl Default for JsonArena {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined arena statistics
#[derive(Debug, Clone)]
pub struct CombinedArenaStats {
    /// Stats from the [`StringArena`] component.
    pub string_stats: ArenaStats,
    /// Number of objects allocated in the object arena.
    pub objects_allocated: usize,
    /// Number of arrays allocated in the array arena.
    pub arrays_allocated: usize,
    /// Number of generic values allocated in the value arena.
    pub values_allocated: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_arena_basic() {
        let arena = StringArena::new();
        let s1 = arena.alloc_str("hello".to_string());
        let s2 = arena.alloc_str("world".to_string());

        assert_eq!(s1, "hello");
        assert_eq!(s2, "world");
    }

    #[test]
    fn test_value_arena_counting() {
        let arena = ValueArena::new();
        assert_eq!(arena.allocated_count(), 0);

        arena.alloc(42);
        assert_eq!(arena.allocated_count(), 1);

        arena.alloc_extend([1, 2, 3]);
        assert_eq!(arena.allocated_count(), 4);
    }

    #[test]
    fn test_json_arena_stats() {
        let arena = JsonArena::new();
        let stats = arena.stats();

        // Initial stats should show zero allocations
        assert_eq!(stats.objects_allocated, 0);
        assert_eq!(stats.arrays_allocated, 0);
        assert_eq!(stats.values_allocated, 0);
    }

    #[test]
    fn test_arena_reset() {
        let mut arena = JsonArena::new();

        // Allocate some values
        arena.values.alloc(serde_json::Value::Null);
        arena.objects.alloc(serde_json::Map::new());

        let initial_stats = arena.stats();
        assert!(initial_stats.values_allocated > 0);

        // Reset should clear everything
        arena.reset();
        let reset_stats = arena.stats();
        assert_eq!(reset_stats.values_allocated, 0);
        assert_eq!(reset_stats.objects_allocated, 0);
    }
}
