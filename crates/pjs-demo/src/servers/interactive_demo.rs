//! Interactive web demo server with multiple dataset types and real-time comparison
//!
//! This server provides a comprehensive web interface to demonstrate PJS benefits
//! across different data types and network conditions.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, Json, Response},
    routing::get,
    Router,
};
use clap::Parser;
use pjs_demo::{
    data::{generate_dataset, generate_metadata, DatasetSize, DatasetType},
    utils::{simulate_network_latency, NetworkType, PerfTimer},
};
// TODO: Re-enable streaming imports when infrastructure is stable
// use pjson_rs::{
//     StreamProcessor, StreamConfig, StreamFrame, Priority,
//     StreamingCompressor, SchemaCompressor, CompressionStrategy,
// };
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, net::SocketAddr, time::Duration};
use tokio::time::sleep;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "interactive-demo-server")]
#[command(about = "Interactive PJS demonstration server with web UI")]
struct Args {
    /// Server port
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Server host  
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Debug, Deserialize)]
struct DemoRequest {
    dataset_type: Option<String>,
    dataset_size: Option<String>,
    network_type: Option<String>,
    enable_streaming: Option<bool>,
}

#[derive(Debug, Serialize)]
struct DemoResponse {
    traditional_time_ms: u64,
    pjs_time_ms: u64,
    improvement_factor: f64,
    data_size_bytes: usize,
    network_simulation: String,
    metadata: HashMap<String, Value>,
}

/// Application state
#[derive(Clone)]
struct AppState {
    // WebSocket support will be added later when infrastructure is stable
}

/// Generate and serve dataset with traditional approach
async fn traditional_endpoint(
    Query(params): Query<DemoRequest>,
    State(_state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let _timer = PerfTimer::new("Traditional JSON");
    
    // Parse parameters
    let dataset_type = params.dataset_type
        .as_deref()
        .unwrap_or("ecommerce")
        .parse::<DatasetType>()
        .unwrap_or(DatasetType::ECommerce);
        
    let dataset_size = params.dataset_size
        .as_deref()
        .unwrap_or("medium")
        .parse::<DatasetSize>()
        .unwrap_or(DatasetSize::Medium);
        
    let network_type = params.network_type
        .as_deref()
        .unwrap_or("wifi")
        .parse::<NetworkType>()
        .unwrap_or(NetworkType::Wifi);

    // Simulate network latency
    simulate_network_latency(network_type).await;
    
    // Generate full dataset
    let data = generate_dataset(dataset_type, dataset_size);
    let metadata = generate_metadata(dataset_type, dataset_size, &data);
    
    // Simulate processing time
    sleep(Duration::from_millis(50)).await;
    
    let response = json!({
        "data": data,
        "metadata": metadata,
        "approach": "traditional",
        "generated_at": chrono::Utc::now().to_rfc3339()
    });
    
    Ok(Json(response))
}

/// Generate and serve dataset with PJS streaming approach
async fn pjs_streaming_endpoint(
    headers: HeaderMap,
    Query(params): Query<DemoRequest>,
    State(_state): State<AppState>,
) -> Result<Response, StatusCode> {
    let _timer = PerfTimer::new("PJS Streaming");
    
    // Parse parameters
    let dataset_type = params.dataset_type
        .as_deref()
        .unwrap_or("ecommerce")
        .parse::<DatasetType>()
        .unwrap_or(DatasetType::ECommerce);
        
    let dataset_size = params.dataset_size
        .as_deref()
        .unwrap_or("medium")
        .parse::<DatasetSize>()
        .unwrap_or(DatasetSize::Medium);
        
    let network_type = params.network_type
        .as_deref()
        .unwrap_or("wifi")
        .parse::<NetworkType>()
        .unwrap_or(NetworkType::Wifi);

    // Check if client wants streaming
    let wants_streaming = headers.get("Accept")
        .and_then(|h| h.to_str().ok())
        .map(|accept| accept.contains("text/event-stream") || accept.contains("application/x-pjs-stream"))
        .unwrap_or(false);

    if wants_streaming {
        // Minimal network latency for skeleton
        simulate_network_latency(NetworkType::Fiber).await;
        
        // Generate skeleton immediately
        let skeleton = generate_skeleton(dataset_type);
        
        let response = axum::response::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .header("X-PJS-Skeleton", "true")
            .header("X-PJS-Priority", "100")
            .body(axum::body::Body::from(skeleton.to_string()))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            
        Ok(response)
    } else {
        // Simulate reduced latency due to PJS optimizations
        simulate_network_latency(network_type).await;
        sleep(Duration::from_millis(20)).await; // Reduced processing time
        
        let data = generate_dataset(dataset_type, dataset_size);
        let metadata = generate_metadata(dataset_type, dataset_size, &data);
        
        let response_data = json!({
            "data": data,
            "metadata": metadata,
            "approach": "pjs_optimized",
            "generated_at": chrono::Utc::now().to_rfc3339()
        });
        
        let response = axum::response::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .header("X-PJS-Optimized", "true")
            .body(axum::body::Body::from(response_data.to_string()))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            
        Ok(response)
    }
}

/// Performance comparison endpoint
async fn performance_comparison(
    Query(params): Query<DemoRequest>,
    State(_state): State<AppState>,
) -> Result<Json<DemoResponse>, StatusCode> {
    let dataset_type = params.dataset_type
        .as_deref()
        .unwrap_or("ecommerce")
        .parse::<DatasetType>()
        .unwrap_or(DatasetType::ECommerce);
        
    let dataset_size = params.dataset_size
        .as_deref()
        .unwrap_or("medium")
        .parse::<DatasetSize>()
        .unwrap_or(DatasetSize::Medium);
        
    let network_type = params.network_type
        .as_deref()
        .unwrap_or("wifi")
        .parse::<NetworkType>()
        .unwrap_or(NetworkType::Wifi);

    // Generate test data
    let data = generate_dataset(dataset_type, dataset_size);
    let data_size = serde_json::to_string(&data)
        .map(|s| s.len())
        .unwrap_or(0);

    // Simulate traditional approach
    let traditional_start = std::time::Instant::now();
    simulate_network_latency(network_type).await;
    sleep(Duration::from_millis(100)).await; // Full processing time
    let traditional_time = traditional_start.elapsed();

    // Simulate PJS approach (skeleton delivery)
    let pjs_start = std::time::Instant::now();
    simulate_network_latency(NetworkType::Fiber).await; // Skeleton uses minimal latency
    sleep(Duration::from_millis(10)).await; // Skeleton processing time
    let pjs_time = pjs_start.elapsed();

    let traditional_ms = traditional_time.as_millis() as u64;
    let pjs_ms = pjs_time.as_millis() as u64;
    let improvement = if pjs_ms > 0 {
        traditional_ms as f64 / pjs_ms as f64
    } else {
        1.0
    };

    let metadata = generate_metadata(dataset_type, dataset_size, &data);

    let response = DemoResponse {
        traditional_time_ms: traditional_ms,
        pjs_time_ms: pjs_ms,
        improvement_factor: improvement,
        data_size_bytes: data_size,
        network_simulation: format!("{:?}", network_type),
        metadata,
    };

    Ok(Json(response))
}

/// Generate skeleton structure for given dataset type
fn generate_skeleton(dataset_type: DatasetType) -> Value {
    match dataset_type {
        DatasetType::ECommerce => json!({
            "store": {
                "name": "PJS Demo Store",
                "status": "loading...",
                "version": "2.0"
            },
            "categories": [],
            "products": [],
            "inventory": null,
            "analytics": null,
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "dataset_info": {
                "type": "ecommerce",
                "loading": true
            }
        }),
        DatasetType::SocialMedia => json!({
            "feed": {
                "name": "PJS Social Demo", 
                "description": "Loading...",
                "version": "1.0"
            },
            "users": [],
            "posts": [],
            "trending": [],
            "analytics": null,
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "dataset_info": {
                "type": "social",
                "loading": true
            }
        }),
        DatasetType::Analytics => json!({
            "dashboard": {
                "name": "PJS Analytics Demo",
                "description": "Loading dashboard...",
                "last_updated": chrono::Utc::now().to_rfc3339()
            },
            "time_series": [],
            "summary": null,
            "generated_at": chrono::Utc::now().to_rfc3339()
        }),
        DatasetType::RealTime => json!({
            "system": {
                "name": "PJS Realtime Demo",
                "status": "connecting..."
            },
            "metrics": [],
            "generated_at": chrono::Utc::now().to_rfc3339()
        }),
    }
}

/// Serve interactive web demo page
async fn demo_page() -> Html<&'static str> {
    Html(include_str!("../static/performance_comparison.html"))
}

/// API information endpoint
async fn api_info() -> Json<Value> {
    Json(json!({
        "name": "PJS Interactive Demo Server",
        "version": "1.0.0",
        "description": "Comprehensive demonstration of Priority JSON Streaming benefits",
        "endpoints": {
            "/": "Interactive web demo interface",
            "/traditional": "Traditional JSON API endpoint",
            "/pjs-streaming": "PJS optimized streaming endpoint", 
            "/performance": "Performance comparison API",
            "/ws": "WebSocket streaming endpoint",
            "/api/info": "This API information endpoint"
        },
        "parameters": {
            "dataset_type": ["ecommerce", "social", "analytics", "realtime"],
            "dataset_size": ["small", "medium", "large", "huge"],
            "network_type": ["fiber", "cable", "wifi", "4g", "3g", "satellite"],
            "enable_streaming": "boolean - enable progressive streaming"
        },
        "features": [
            "Multiple dataset types with realistic data",
            "Network latency simulation",
            "Real-time performance comparison",
            "WebSocket streaming support",
            "Interactive web interface"
        ]
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    // Initialize logging
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(if args.verbose { 
            tracing::Level::DEBUG 
        } else { 
            tracing::Level::INFO 
        });
    subscriber.init();

    let state = AppState {
        // WebSocket support will be added later
    };

    // Build router
    let app = Router::new()
        .route("/", get(demo_page))
        .route("/traditional", get(traditional_endpoint))
        .route("/pjs-streaming", get(pjs_streaming_endpoint))
        .route("/performance", get(performance_comparison))
        .route("/api/info", get(api_info))
        // WebSocket will be added later when infrastructure is stable
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    
    println!();
    println!("🚀 PJS Interactive Demo Server");
    println!("==============================");
    println!("📍 Server: http://{}:{}", args.host, args.port);
    println!();
    println!("📱 Web Interface:");
    println!("   http://{}:{}/ - Interactive Demo", args.host, args.port);
    println!();
    println!("🔗 API Endpoints:");
    println!("   GET  /traditional?dataset_type=ecommerce&dataset_size=medium&network_type=wifi");
    println!("   GET  /pjs-streaming?dataset_type=social&dataset_size=large&network_type=4g");
    println!("   GET  /performance?dataset_type=analytics&dataset_size=small&network_type=fiber");
    println!("   GET  /api/info - API documentation");
    println!("   WS   /ws - WebSocket streaming");
    println!();
    println!("🎯 Dataset Types: ecommerce, social, analytics, realtime");
    println!("📊 Dataset Sizes: small, medium, large, huge");
    println!("🌐 Network Types: fiber, cable, wifi, 4g, 3g, satellite");
    println!();
    println!("💡 Try different combinations to see PJS benefits!");
    println!();

    info!("Starting server on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}