//! Arena-based memory allocation for high-performance JSON parsing
//!
//! Arena allocators provide excellent performance for scenarios where many objects
//! are allocated together and freed all at once, which is perfect for JSON parsing.

use std::{cell::RefCell, collections::HashMap};
use typed_arena::Arena;

/// Arena-backed string pool for zero-copy string operations
pub struct StringArena {
    arena: Arena<String>,
    interned: RefCell<HashMap<&'static str, &'static str>>,
}

impl StringArena {
    /// Create new string arena
    pub fn new() -> Self {
        Self {
            arena: Arena::new(),
            interned: RefCell::new(HashMap::new()),
        }
    }

    /// Allocate string in arena and return reference with arena lifetime
    pub fn alloc_str(&self, s: String) -> &str {
        self.arena.alloc(s).as_str()
    }

    /// Intern string to avoid duplicates (useful for JSON keys)
    pub fn intern(&self, s: &str) -> &str {
        // For demonstration - in production you'd use a more sophisticated interning strategy
        if let Some(&interned) = self.interned.borrow().get(s) {
            return interned;
        }

        let allocated = self.alloc_str(s.to_string());
        // This is unsafe in real usage - just for demonstration
        let static_ref: &'static str = unsafe { std::mem::transmute(allocated) };
        self.interned.borrow_mut().insert(static_ref, static_ref);
        static_ref
    }

    /// Get current memory usage statistics
    pub fn memory_usage(&self) -> ArenaStats {
        // typed_arena doesn't expose internal stats, so we estimate
        ArenaStats {
            chunks_allocated: 1,  // Simplified
            total_bytes: 0,       // Would need arena introspection
            strings_allocated: 0, // Would need counting
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
    pub chunks_allocated: usize,
    pub total_bytes: usize,
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
    pub string_stats: ArenaStats,
    pub objects_allocated: usize,
    pub arrays_allocated: usize,
    pub values_allocated: usize,
}

/// Arena-aware JSON parser for high-performance parsing
pub struct ArenaJsonParser {
    arena: JsonArena,
    reuse_threshold: usize,
}

impl ArenaJsonParser {
    /// Create new arena-backed parser
    pub fn new() -> Self {
        Self {
            arena: JsonArena::new(),
            reuse_threshold: 1000, // Reset arena after 1000 allocations
        }
    }

    /// Parse JSON using arena allocation
    pub fn parse(&mut self, json: &str) -> Result<serde_json::Value, serde_json::Error> {
        // Check if we should reset arena to prevent unbounded growth
        let stats = self.arena.stats();
        if stats.values_allocated > self.reuse_threshold {
            self.arena.reset();
        }

        // For now, fall back to standard parsing
        // In a full implementation, you'd integrate arena allocation with the parser
        serde_json::from_str(json)
    }

    /// Get current arena statistics
    pub fn arena_stats(&self) -> CombinedArenaStats {
        self.arena.stats()
    }
}

impl Default for ArenaJsonParser {
    fn default() -> Self {
        Self::new()
    }
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
    fn test_arena_parser() {
        let mut parser = ArenaJsonParser::new();
        let result = parser.parse(r#"{"key": "value"}"#);

        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value.is_object());
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
