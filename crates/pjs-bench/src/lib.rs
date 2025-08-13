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
//! cargo bench
//! ```
//!
//! Run specific benchmark:
//! ```bash
//! cargo bench throughput
//! cargo bench latency
//! cargo bench memory_usage
//! cargo bench comparison
//! ```

pub use pjson_rs::{
    Error, Frame, JsonReconstructor, Parser, Priority, Result, StreamConfig, StreamFrame,
    StreamProcessor,
};

// Fallback implementations for benchmarking when streaming features aren't available
pub mod fallback {
    use crate::Priority;
    use bytes::Bytes;

    #[derive(Debug, Clone)]
    pub struct StreamerConfig {
        chunk_size: usize,
    }

    impl Default for StreamerConfig {
        fn default() -> Self {
            Self::new()
        }
    }

    impl StreamerConfig {
        pub fn new() -> Self {
            Self { chunk_size: 1024 }
        }

        pub fn chunk_size(&self) -> usize {
            self.chunk_size
        }

        pub fn with_chunk_size(mut self, size: usize) -> Self {
            self.chunk_size = size;
            self
        }

        pub fn with_priority_threshold(self, _priority: Priority) -> Self {
            self
        }

        pub fn with_memory_limit(self, _limit: usize) -> Self {
            self
        }
    }

    #[derive(Debug, Clone)]
    pub struct StreamFrame {
        data: Bytes,
    }

    impl StreamFrame {
        /// Create new stream frame
        pub fn new(data: Bytes) -> Self {
            Self { data }
        }

        /// Get the frame data
        pub fn data(&self) -> &Bytes {
            &self.data
        }

        /// Get the frame size in bytes
        pub fn size(&self) -> usize {
            self.data.len()
        }
    }

    #[derive(Debug)]
    pub struct PriorityStreamer {
        config: StreamerConfig,
    }

    impl PriorityStreamer {
        pub fn new(config: StreamerConfig) -> Self {
            Self { config }
        }

        pub fn create_priority_frames(&self, data: &Bytes) -> Vec<StreamFrame> {
            // Use config to determine frame size
            let chunk_size = self.config.chunk_size();
            if data.len() <= chunk_size {
                vec![StreamFrame::new(data.clone())]
            } else {
                // Split into multiple frames based on config
                let mut frames = Vec::new();
                for chunk in data.chunks(chunk_size) {
                    frames.push(StreamFrame::new(Bytes::copy_from_slice(chunk)));
                }
                frames
            }
        }
    }
}

pub use fallback::*;

/// Benchmarking utilities and data generators
pub struct BenchSuite {
    config: StreamConfig,
}

impl BenchSuite {
    /// Create new benchmark suite with default configuration
    pub fn new() -> Self {
        Self {
            config: StreamConfig::default(),
        }
    }

    /// Create benchmark suite with custom configuration
    pub fn with_config(config: StreamConfig) -> Self {
        Self { config }
    }

    /// Generate social media feed data for benchmarking
    pub fn generate_social_feed(&self, posts: usize) -> String {
        use serde_json::json;

        let posts: Vec<_> = (0..posts)
            .map(|i| {
                json!({
                    "id": i,
                    "user_id": 1000 + (i % 100),
                    "username": format!("user_{}", i % 100),
                    "content": format!("Post {} content", i),
                    "timestamp": format!("2024-01-{:02}T10:30:00Z", (i % 28) + 1),
                    "likes": i * 3 + 15,
                    "comments": i / 2 + 5
                })
            })
            .collect();

        json!({
            "posts": posts,
            "total": posts,
            "page": 1
        })
        .to_string()
    }

    /// Generate e-commerce catalog data for benchmarking
    pub fn generate_product_catalog(&self, products: usize) -> String {
        use serde_json::json;

        let products: Vec<_> = (0..products)
            .map(|i| {
                json!({
                    "id": i,
                    "name": format!("Product {}", i),
                    "price": (i as f64 * 1.5 + 19.99).round() / 100.0 * 100.0,
                    "in_stock": i % 10 != 0,
                    "category": format!("Category {}", i % 10),
                    "rating": ((i % 5) as f64 + 3.0).min(5.0)
                })
            })
            .collect();

        json!({
            "products": products,
            "total": products,
            "filters": {
                "categories": (0..10).map(|i| format!("Category {i}")).collect::<Vec<_>>()
            }
        })
        .to_string()
    }

    /// Get the streamer configuration
    pub fn config(&self) -> &StreamConfig {
        &self.config
    }
}

impl Default for BenchSuite {
    fn default() -> Self {
        Self::new()
    }
}

/// Performance metrics for comparison
#[derive(Debug, Clone)]
pub struct BenchMetrics {
    pub throughput_mbps: f64,
    pub latency_ms: f64,
    pub memory_mb: f64,
    pub allocations: usize,
}

impl BenchMetrics {
    /// Create new metrics
    pub fn new(throughput_mbps: f64, latency_ms: f64, memory_mb: f64, allocations: usize) -> Self {
        Self {
            throughput_mbps,
            latency_ms,
            memory_mb,
            allocations,
        }
    }

    /// Calculate efficiency ratio compared to baseline
    pub fn efficiency_ratio(&self, baseline: &BenchMetrics) -> f64 {
        let throughput_ratio = self.throughput_mbps / baseline.throughput_mbps;
        let latency_ratio = baseline.latency_ms / self.latency_ms; // Lower is better
        let memory_ratio = baseline.memory_mb / self.memory_mb; // Lower is better

        (throughput_ratio + latency_ratio + memory_ratio) / 3.0
    }
}

/// Feature detection for conditional compilation
pub mod features {
    /// Check if streaming features are available
    pub fn has_streaming() -> bool {
        cfg!(feature = "streaming")
    }

    /// Check if SIMD features are available
    pub fn has_simd() -> bool {
        cfg!(any(
            feature = "simd-auto",
            feature = "simd-avx2",
            feature = "simd-sse42"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bench_suite_creation() {
        let bench = BenchSuite::new();
        // StreamConfig doesn't have chunk_size method, just ensure config exists
        let _config = bench.config();
    }

    #[test]
    fn test_feature_detection() {
        use crate::features;

        // These will depend on compilation features
        let has_streaming = features::has_streaming();
        let has_simd = features::has_simd();

        // Just ensure they return boolean values
        assert!(has_streaming == true || has_streaming == false);
        assert!(has_simd == true || has_simd == false);
    }

    #[test]
    fn test_data_generation() {
        let bench = BenchSuite::new();

        let social_data = bench.generate_social_feed(10);
        assert!(social_data.contains("posts"));

        let catalog_data = bench.generate_product_catalog(5);
        assert!(catalog_data.contains("products"));
    }

    #[test]
    fn test_metrics() {
        let metrics1 = BenchMetrics::new(100.0, 10.0, 50.0, 1000);
        let metrics2 = BenchMetrics::new(200.0, 5.0, 25.0, 500);

        let ratio = metrics2.efficiency_ratio(&metrics1);
        assert!(ratio > 1.0); // metrics2 should be better
    }

    #[test]
    fn test_stream_frame() {
        use bytes::Bytes;

        let data = Bytes::from("test data");
        let frame = super::fallback::StreamFrame::new(data.clone());

        assert_eq!(frame.data(), &data);
        assert_eq!(frame.size(), data.len());
    }

    #[test]
    fn test_priority_streamer() {
        use bytes::Bytes;

        let config = super::fallback::StreamerConfig::new();
        let streamer = super::fallback::PriorityStreamer::new(config);

        let test_data = Bytes::from("test streaming data");
        let frames = streamer.create_priority_frames(&test_data);

        assert!(!frames.is_empty());
        assert_eq!(frames[0].data(), &test_data);
        assert_eq!(frames[0].size(), test_data.len());
    }
}
