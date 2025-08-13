//! Memory management utilities for high-performance JSON processing

pub mod arena;

pub use arena::{
    ArenaJsonParser, 
    JsonArena, 
    StringArena, 
    ValueArena,
    CombinedArenaStats,
    ArenaStats,
};