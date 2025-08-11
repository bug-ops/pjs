//! In-memory repository implementations

use std::collections::HashMap;

/// Memory-based storage placeholder
pub struct MemoryRepository {
    _data: HashMap<String, Vec<u8>>,
}

impl MemoryRepository {
    pub fn new() -> Self {
        Self {
            _data: HashMap::new(),
        }
    }
}

impl Default for MemoryRepository {
    fn default() -> Self {
        Self::new()
    }
}