//! Memory management utilities for high-performance JSON processing

pub mod arena;

pub use arena::{
    ArenaJsonParser, ArenaStats, CombinedArenaStats, JsonArena, StringArena, ValueArena,
};
