//! PJS Benchmarking Suite
//!
//! Comprehensive benchmarking for the Priority JSON Streaming Protocol.
//! This crate provides performance comparisons against standard JSON parsing libraries
//! and demonstrates the advantages of priority-based streaming.
//!
//! # Benchmarks
//!
//! - **Throughput**: Raw parsing speed comparison
//! - **Latency**: Time to First Meaningful Data (TTFMD)
//! - **Memory Usage**: Memory consumption and allocation patterns
//! - **Comparison**: Side-by-side with major JSON libraries
//!
//! # Usage
//!
//! Run all benchmarks:
//! ```bash
//! cargo bench -p pjs-bench
//! ```
//!
//! `client_apply_patches` is a stub that prints a skip line unless built with
//! the `pjs_bench` cfg. Run the full suite with:
//! ```bash
//! RUSTFLAGS="-C target-cpu=native --cfg pjs_bench" cargo bench -p pjs-bench
//! ```
//!
//! Run a specific benchmark target:
//! ```bash
//! cargo bench -p pjs-bench --bench simple_throughput
//! cargo bench -p pjs-bench --bench zero_copy_bench
//! cargo bench -p pjs-bench --bench serde_overhead_bench
//! cargo bench -p pjs-bench --bench gat_query_benchmarks
//! RUSTFLAGS="-C target-cpu=native --cfg pjs_bench" cargo bench -p pjs-bench --bench client_apply_patches
//! ```
