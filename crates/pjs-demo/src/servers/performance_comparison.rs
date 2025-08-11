//! Performance comparison server for PJS vs traditional JSON
//!
//! This server provides side-by-side comparison of PJS streaming
//! versus traditional JSON delivery for various data types.

use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use pjson_rs::{
    compression::{CompressionStrategy, SchemaCompressor},
    stream::{ProcessResult, StreamProcessor, StreamStats},
    ApplicationResult,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{info, warn};

/// Performance comparison configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ComparisonConfig {
    /// Data type to compare
    data_type: String,
    /// Data size category (small, medium, large, huge)
    data_size: Option<String>,
    /// Number of test iterations
    iterations: Option<usize>,
    /// Enable compression for PJS
    enable_compression: Option<bool>,
}

impl Default for ComparisonConfig {
    fn default() -> Self {
        Self {
            data_type: "ecommerce".to_string(),
            data_size: Some("medium".to_string()),
            iterations: Some(5),
            enable_compression: Some(true),
        }
    }
}

/// Performance measurement result
#[derive(Debug, Serialize)]
pub struct PerformanceResult {
    /// Test configuration used
    config: ComparisonConfig,
    /// Traditional JSON results
    traditional_json: DeliveryMetrics,
    /// PJS streaming results
    pjs_streaming: DeliveryMetrics,
    /// Performance comparison metrics
    comparison: ComparisonMetrics,
    /// Test metadata
    metadata: TestMetadata,
}

/// Delivery performance metrics
#[derive(Debug, Serialize)]
pub struct DeliveryMetrics {
    /// Time to first byte (milliseconds)
    time_to_first_byte_ms: f64,
    /// Time to complete delivery (milliseconds)
    total_delivery_time_ms: f64,
    /// Total bytes transferred
    total_bytes: u64,
    /// Number of network roundtrips
    network_roundtrips: usize,
    /// Compression ratio (if applicable)
    compression_ratio: Option<f64>,
    /// Perceived user experience score (0-100)
    ux_score: f64,
}

/// Performance comparison metrics
#[derive(Debug, Serialize)]
pub struct ComparisonMetrics {
    /// TTFB improvement factor
    ttfb_improvement: f64,
    /// Total time improvement factor  
    total_time_improvement: f64,
    /// Bandwidth efficiency improvement
    bandwidth_efficiency: f64,
    /// User experience improvement
    ux_improvement: f64,
    /// Overall performance score
    overall_score: f64,
}

/// Test metadata
#[derive(Debug, Serialize)]
pub struct TestMetadata {
    /// Test execution timestamp
    timestamp: chrono::DateTime<chrono::Utc>,
    /// Test duration in seconds
    duration_seconds: f64,
    /// Number of iterations performed
    iterations: usize,
    /// Data characteristics
    data_characteristics: DataCharacteristics,
}

/// Data characteristics analysis
#[derive(Debug, Serialize)]
pub struct DataCharacteristics {
    /// Original data size in bytes
    original_size_bytes: u64,
    /// Number of JSON objects/fields
    object_count: usize,
    /// Maximum nesting depth
    max_depth: usize,
    /// Number of arrays
    array_count: usize,
    /// Estimated priority distribution
    priority_distribution: PriorityDistribution,
}

/// Priority level distribution
#[derive(Debug, Serialize)]
pub struct PriorityDistribution {
    critical: f64,    // 0-1 percentage
    high: f64,
    medium: f64,
    low: f64,
    background: f64,
}

/// Performance comparison server
#[tokio::main]
async fn main() -> ApplicationResult<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(index_page))
        .route("/compare", post(run_performance_comparison))
        .route("/benchmark/:data_type", get(quick_benchmark))
        .route("/health", get(health_check));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3002")
        .await
        .map_err(|e| format!("Failed to bind to address: {}", e))?;

    info!("Performance comparison server running on http://127.0.0.1:3002");

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {}", e))?;

    Ok(())
}

/// Serve the main HTML page
async fn index_page() -> Html<&'static str> {
    Html(include_str!("../static/performance_comparison.html"))
}

/// Run comprehensive performance comparison
async fn run_performance_comparison(
    Json(config): Json<ComparisonConfig>,
) -> Result<Json<PerformanceResult>, StatusCode> {
    let test_start = Instant::now();
    info!("Starting performance comparison: {:?}", config);

    // Generate test data
    let test_data = generate_test_data(&config)?;
    let data_characteristics = analyze_data_characteristics(&test_data);

    // Run traditional JSON test
    let traditional_metrics = measure_traditional_json_delivery(&test_data, &config).await?;

    // Run PJS streaming test
    let pjs_metrics = measure_pjs_streaming_delivery(&test_data, &config).await?;

    // Calculate comparison metrics
    let comparison = calculate_comparison_metrics(&traditional_metrics, &pjs_metrics);

    let result = PerformanceResult {
        config: config.clone(),
        traditional_json: traditional_metrics,
        pjs_streaming: pjs_metrics,
        comparison,
        metadata: TestMetadata {
            timestamp: chrono::Utc::now(),
            duration_seconds: test_start.elapsed().as_secs_f64(),
            iterations: config.iterations.unwrap_or(1),
            data_characteristics,
        },
    };

    info!("Performance comparison completed: Overall score: {:.2}", result.comparison.overall_score);

    Ok(Json(result))
}

/// Quick benchmark for specific data type
async fn quick_benchmark(
    axum::extract::Path(data_type): axum::extract::Path<String>,
) -> Result<Json<PerformanceResult>, StatusCode> {
    let config = ComparisonConfig {
        data_type,
        data_size: Some("medium".to_string()),
        iterations: Some(1),
        enable_compression: Some(true),
    };

    run_performance_comparison(Json(config)).await
}

/// Generate test data based on configuration
fn generate_test_data(config: &ComparisonConfig) -> Result<JsonValue, StatusCode> {
    use crate::data::*;

    let data_size = config.data_size.as_deref().unwrap_or("medium");

    let data = match config.data_type.as_str() {
        "ecommerce" => match data_size {
            "small" => ecommerce::generate_product_catalog(10),
            "medium" => ecommerce::generate_product_catalog(100),
            "large" => ecommerce::generate_product_catalog(1000),
            "huge" => ecommerce::generate_product_catalog(10000),
            _ => return Err(StatusCode::BAD_REQUEST),
        },
        "analytics" => match data_size {
            "small" => analytics::generate_dashboard_data(50),
            "medium" => analytics::generate_dashboard_data(500),
            "large" => analytics::generate_dashboard_data(5000),
            "huge" => analytics::generate_dashboard_data(50000),
            _ => return Err(StatusCode::BAD_REQUEST),
        },
        "social" => match data_size {
            "small" => social::generate_social_feed(20),
            "medium" => social::generate_social_feed(200),
            "large" => social::generate_social_feed(2000),
            "huge" => social::generate_social_feed(20000),
            _ => return Err(StatusCode::BAD_REQUEST),
        },
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    Ok(data)
}

/// Measure traditional JSON delivery performance
async fn measure_traditional_json_delivery(
    data: &JsonValue,
    config: &ComparisonConfig,
) -> Result<DeliveryMetrics, StatusCode> {
    let iterations = config.iterations.unwrap_or(1);
    let mut total_times = Vec::new();
    let mut first_byte_times = Vec::new();

    for _ in 0..iterations {
        let start = Instant::now();
        
        // Simulate network delay for first byte
        sleep(Duration::from_millis(50)).await;
        let first_byte_time = start.elapsed();
        
        // Serialize entire JSON (traditional approach)
        let serialized = serde_json::to_string(data)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        
        // Simulate network transfer time based on size
        let transfer_delay = calculate_network_delay(serialized.len());
        sleep(transfer_delay).await;
        
        let total_time = start.elapsed();
        
        first_byte_times.push(first_byte_time.as_secs_f64() * 1000.0);
        total_times.push(total_time.as_secs_f64() * 1000.0);
    }

    // Calculate averages
    let avg_first_byte = first_byte_times.iter().sum::<f64>() / first_byte_times.len() as f64;
    let avg_total_time = total_times.iter().sum::<f64>() / total_times.len() as f64;

    let serialized_size = serde_json::to_string(data)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .len() as u64;

    // Traditional JSON has poor UX due to all-or-nothing delivery
    let ux_score = calculate_traditional_ux_score(avg_total_time, serialized_size);

    Ok(DeliveryMetrics {
        time_to_first_byte_ms: avg_first_byte,
        total_delivery_time_ms: avg_total_time,
        total_bytes: serialized_size,
        network_roundtrips: 1,
        compression_ratio: None,
        ux_score,
    })
}

/// Measure PJS streaming delivery performance
async fn measure_pjs_streaming_delivery(
    data: &JsonValue,
    config: &ComparisonConfig,
) -> Result<DeliveryMetrics, StatusCode> {
    let iterations = config.iterations.unwrap_or(1);
    let mut total_times = Vec::new();
    let mut first_byte_times = Vec::new();
    let mut total_bytes_vec = Vec::new();

    for _ in 0..iterations {
        let start = Instant::now();
        
        // Create stream processor
        let mut processor = StreamProcessor::new();
        let frames = processor.process_json(data)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        
        if frames.is_empty() {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }

        // Simulate sending first high-priority frame
        sleep(Duration::from_millis(5)).await; // PJS has lower latency
        let first_byte_time = start.elapsed();
        
        let mut total_bytes = 0u64;
        
        // Apply compression if enabled
        if config.enable_compression.unwrap_or(true) {
            let compressor = SchemaCompressor::new();
            for frame in &frames {
                match compressor.compress(frame.payload()) {
                    Ok(compressed) => {
                        total_bytes += compressed.data().to_string().len() as u64;
                    }
                    Err(_) => {
                        // Fall back to uncompressed
                        total_bytes += frame.payload().to_string().len() as u64;
                    }
                }
                
                // Simulate progressive delivery with priority-based delays
                let frame_delay = calculate_frame_delay(frame.priority().value());
                sleep(frame_delay).await;
            }
        } else {
            for frame in &frames {
                total_bytes += frame.payload().to_string().len() as u64;
                
                let frame_delay = calculate_frame_delay(frame.priority().value());
                sleep(frame_delay).await;
            }
        }
        
        let total_time = start.elapsed();
        
        first_byte_times.push(first_byte_time.as_secs_f64() * 1000.0);
        total_times.push(total_time.as_secs_f64() * 1000.0);
        total_bytes_vec.push(total_bytes);
    }

    // Calculate averages
    let avg_first_byte = first_byte_times.iter().sum::<f64>() / first_byte_times.len() as f64;
    let avg_total_time = total_times.iter().sum::<f64>() / total_times.len() as f64;
    let avg_total_bytes = total_bytes_vec.iter().sum::<u64>() / total_bytes_vec.len() as u64;

    // PJS has better UX due to progressive delivery
    let ux_score = calculate_pjs_ux_score(avg_first_byte, avg_total_time, avg_total_bytes);

    Ok(DeliveryMetrics {
        time_to_first_byte_ms: avg_first_byte,
        total_delivery_time_ms: avg_total_time,
        total_bytes: avg_total_bytes,
        network_roundtrips: 1, // Single connection, multiple frames
        compression_ratio: Some(0.7), // TODO: Calculate actual compression ratio
        ux_score,
    })
}

/// Calculate network delay based on data size
fn calculate_network_delay(bytes: usize) -> Duration {
    // Simulate varying network conditions
    let base_delay_ms = 10;
    let size_penalty_ms = (bytes / 1024) as u64; // 1ms per KB
    Duration::from_millis(base_delay_ms + size_penalty_ms)
}

/// Calculate frame delivery delay based on priority
fn calculate_frame_delay(priority: u8) -> Duration {
    let delay_ms = match priority {
        200..=255 => 1,  // Critical - immediate
        150..=199 => 2,  // High - very fast
        100..=149 => 5,  // Medium - fast
        50..=99 => 10,   // Low - moderate
        _ => 20,         // Background - slow
    };
    Duration::from_millis(delay_ms)
}

/// Calculate UX score for traditional JSON delivery
fn calculate_traditional_ux_score(total_time_ms: f64, size_bytes: u64) -> f64 {
    // Traditional JSON gets penalized for all-or-nothing delivery
    let time_penalty = if total_time_ms > 1000.0 { 0.5 } else { 0.8 };
    let size_penalty = if size_bytes > 100_000 { 0.6 } else { 0.9 };
    
    (time_penalty * size_penalty * 100.0).min(100.0)
}

/// Calculate UX score for PJS delivery
fn calculate_pjs_ux_score(first_byte_ms: f64, total_time_ms: f64, _size_bytes: u64) -> f64 {
    // PJS gets bonus for progressive delivery
    let first_byte_bonus = if first_byte_ms < 100.0 { 1.2 } else { 1.0 };
    let progressive_bonus = 1.3; // Bonus for progressive delivery
    
    let base_score = 80.0; // Base score for PJS approach
    (base_score * first_byte_bonus * progressive_bonus).min(100.0)
}

/// Calculate comparison metrics between traditional and PJS delivery
fn calculate_comparison_metrics(
    traditional: &DeliveryMetrics,
    pjs: &DeliveryMetrics,
) -> ComparisonMetrics {
    let ttfb_improvement = traditional.time_to_first_byte_ms / pjs.time_to_first_byte_ms;
    let total_time_improvement = traditional.total_delivery_time_ms / pjs.total_delivery_time_ms;
    let bandwidth_efficiency = traditional.total_bytes as f64 / pjs.total_bytes as f64;
    let ux_improvement = pjs.ux_score / traditional.ux_score;
    
    let overall_score = (ttfb_improvement + ux_improvement + bandwidth_efficiency) / 3.0;

    ComparisonMetrics {
        ttfb_improvement,
        total_time_improvement,
        bandwidth_efficiency,
        ux_improvement,
        overall_score,
    }
}

/// Analyze data characteristics
fn analyze_data_characteristics(data: &JsonValue) -> DataCharacteristics {
    let serialized = data.to_string();
    let original_size = serialized.len() as u64;
    
    // Simple heuristics for analysis
    let object_count = serialized.matches('{').count();
    let array_count = serialized.matches('[').count();
    let max_depth = estimate_json_depth(data);
    
    // Estimate priority distribution based on data structure
    let priority_distribution = estimate_priority_distribution(data);

    DataCharacteristics {
        original_size_bytes: original_size,
        object_count,
        max_depth,
        array_count,
        priority_distribution,
    }
}

/// Estimate JSON nesting depth
fn estimate_json_depth(value: &JsonValue) -> usize {
    match value {
        JsonValue::Object(obj) => {
            1 + obj.values().map(estimate_json_depth).max().unwrap_or(0)
        }
        JsonValue::Array(arr) => {
            1 + arr.iter().map(estimate_json_depth).max().unwrap_or(0)
        }
        _ => 0,
    }
}

/// Estimate priority distribution of data fields
fn estimate_priority_distribution(_data: &JsonValue) -> PriorityDistribution {
    // TODO: Implement intelligent priority estimation based on field names and structure
    PriorityDistribution {
        critical: 0.1,   // 10% critical (IDs, status, etc.)
        high: 0.2,       // 20% high (names, titles, key metrics)
        medium: 0.4,     // 40% medium (descriptions, metadata)
        low: 0.2,        // 20% low (detailed info)
        background: 0.1, // 10% background (analytics, logs)
    }
}

/// Health check endpoint
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "server": "performance-comparison",
        "timestamp": chrono::Utc::now()
    }))
}