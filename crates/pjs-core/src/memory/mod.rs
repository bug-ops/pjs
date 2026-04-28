//! Memory management utilities for high-performance JSON processing

pub mod arena;

pub use arena::{ArenaStats, CombinedArenaStats, JsonArena, StringArena, ValueArena};
