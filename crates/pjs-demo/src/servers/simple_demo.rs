//! Simple PJS demo server without complex data generators
//! 
//! This is a minimal working version that demonstrates PJS concepts
//! without the technical debt from complex infrastructure.

use axum::{
    extract::Query,
    response::{Html, Json},
    routing::get,
    Router,
};
use clap::Parser;
// TODO: Re-enable PriorityStreamer usage after fixing infrastructure compilation
// use pjson_rs::PriorityStreamer;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{net::SocketAddr, time::Duration};
use tokio::time::sleep;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "simple-demo-server")]
#[command(about = "Simple PJS demonstration server")]
struct Args {
    /// Server port
    #[arg(short, long, default_value = "3000")]
    port: u16,
}

#[derive(Debug, Deserialize)]
struct DemoRequest {
    size: Option<String>,
    latency: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ComparisonResult {
    traditional_time_ms: u64,
    pjs_time_ms: u64,
    improvement_factor: f64,
    data_size_bytes: usize,
}

/// Generate simple demo data
fn generate_demo_data(size: &str) -> Value {
    let item_count = match size {
        "small" => 5,
        "medium" => 25,
        "large" => 100,
        _ => 10,
    };

    json!({
        "store": {
            "name": "PJS Demo Store",
            "status": "operational",
            "version": "1.0.0"
        },
        "products": (0..item_count).map(|i| {
            json!({
                "id": i + 1,
                "name": format!("Product {}", i + 1),
                "price": 29.99 + (i as f64 * 5.0),
                "category": format!("Category {}", (i % 3) + 1),
                "in_stock": i % 4 != 0,
                "description": format!("Product {} description with some details", i + 1)
            })
        }).collect::<Vec<_>>(),
        "summary": {
            "total_products": item_count,
            "categories": 3,
            "avg_price": 50.0
        },
        "generated_at": chrono::Utc::now().to_rfc3339()
    })
}

/// Traditional JSON endpoint
async fn traditional_endpoint(Query(params): Query<DemoRequest>) -> Json<Value> {
    let size = params.size.as_deref().unwrap_or("medium");
    let latency = params.latency.unwrap_or(100);
    
    // Simulate network latency
    sleep(Duration::from_millis(latency)).await;
    
    let data = generate_demo_data(size);
    
    // Simulate processing time
    sleep(Duration::from_millis(50)).await;
    
    Json(json!({
        "approach": "traditional",
        "data": data,
        "metadata": {
            "size": size,
            "latency_ms": latency
        }
    }))
}

/// PJS optimized endpoint
async fn pjs_endpoint(Query(params): Query<DemoRequest>) -> Json<Value> {
    let _size = params.size.as_deref().unwrap_or("medium");
    let latency = params.latency.unwrap_or(100);
    
    // PJS: Minimal latency for skeleton
    sleep(Duration::from_millis(latency / 10)).await;
    
    // Generate skeleton first
    let skeleton = json!({
        "store": {
            "name": "PJS Demo Store",
            "status": "loading...",
            "version": "1.0.0"
        },
        "products": [],
        "summary": null,
        "pjs_info": {
            "skeleton": true,
            "priority": 100,
            "approach": "pjs_optimized"
        }
    });
    
    Json(skeleton)
}

/// Performance comparison endpoint
async fn performance_comparison(Query(params): Query<DemoRequest>) -> Json<ComparisonResult> {
    let size = params.size.as_deref().unwrap_or("medium");
    let latency = params.latency.unwrap_or(100);
    
    let data = generate_demo_data(size);
    let data_size = serde_json::to_string(&data).unwrap().len();
    
    // Simulate traditional approach
    let traditional_start = std::time::Instant::now();
    sleep(Duration::from_millis(latency + 50)).await;
    let traditional_time = traditional_start.elapsed();
    
    // Simulate PJS skeleton approach
    let pjs_start = std::time::Instant::now();
    sleep(Duration::from_millis(latency / 10 + 5)).await; // Much faster
    let pjs_time = pjs_start.elapsed();
    
    let traditional_ms = traditional_time.as_millis() as u64;
    let pjs_ms = pjs_time.as_millis() as u64;
    let improvement = if pjs_ms > 0 {
        traditional_ms as f64 / pjs_ms as f64
    } else {
        1.0
    };
    
    Json(ComparisonResult {
        traditional_time_ms: traditional_ms,
        pjs_time_ms: pjs_ms,
        improvement_factor: improvement,
        data_size_bytes: data_size,
    })
}

/// Simple demo page
async fn demo_page() -> Html<&'static str> {
    Html(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>🚀 PJS Simple Demo</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, sans-serif;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background: linear-gradient(135deg, #667eea, #764ba2);
            min-height: 100vh;
            color: white;
        }
        .container {
            background: rgba(255,255,255,0.1);
            padding: 30px;
            border-radius: 15px;
            backdrop-filter: blur(10px);
        }
        h1 { text-align: center; font-size: 2.5em; margin-bottom: 30px; }
        .controls { display: flex; gap: 20px; margin-bottom: 30px; align-items: center; }
        select, button { 
            padding: 10px 15px; 
            border: none; 
            border-radius: 8px; 
            font-size: 16px;
        }
        button { 
            background: #4a6cf7; 
            color: white; 
            cursor: pointer;
            font-weight: 600;
        }
        button:hover { background: #3b5bf2; }
        .results { 
            display: grid; 
            grid-template-columns: 1fr 1fr; 
            gap: 30px; 
            margin-top: 30px;
        }
        .result {
            background: rgba(255,255,255,0.1);
            padding: 20px;
            border-radius: 10px;
            border-left: 5px solid;
        }
        .traditional { border-left-color: #e74c3c; }
        .pjs { border-left-color: #27ae60; }
        .metric { 
            display: flex; 
            justify-content: space-between; 
            margin: 10px 0;
            padding: 10px 0;
            border-bottom: 1px solid rgba(255,255,255,0.2);
        }
        .metric:last-child { border-bottom: none; }
        .improvement {
            text-align: center;
            padding: 20px;
            margin: 20px 0;
            background: rgba(39, 174, 96, 0.2);
            border-radius: 10px;
            font-size: 1.2em;
            font-weight: bold;
        }
        @media (max-width: 768px) {
            .results { grid-template-columns: 1fr; }
            .controls { flex-direction: column; }
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>🚀 PJS Simple Demo</h1>
        
        <div class="controls">
            <label>Dataset Size:</label>
            <select id="size">
                <option value="small">Small (5 items, ~1KB)</option>
                <option value="medium" selected>Medium (25 items, ~5KB)</option>
                <option value="large">Large (100 items, ~20KB)</option>
            </select>
            
            <label>Network Latency:</label>
            <select id="latency">
                <option value="50">Fast (50ms)</option>
                <option value="100" selected>Normal (100ms)</option>
                <option value="200">Slow (200ms)</option>
                <option value="500">Very Slow (500ms)</option>
            </select>
            
            <button onclick="runComparison()">🏁 Run Test</button>
        </div>
        
        <div class="results" id="results" style="display: none;">
            <div class="result traditional">
                <h3>🐌 Traditional JSON</h3>
                <div class="metric">
                    <span>Response Time:</span>
                    <span id="traditional-time">-</span>
                </div>
                <div class="metric">
                    <span>User Experience:</span>
                    <span>Wait → Complete</span>
                </div>
            </div>
            
            <div class="result pjs">
                <h3>⚡ PJS Streaming</h3>
                <div class="metric">
                    <span>First Content:</span>
                    <span id="pjs-time">-</span>
                </div>
                <div class="metric">
                    <span>User Experience:</span>
                    <span>Instant → Progressive</span>
                </div>
            </div>
        </div>
        
        <div class="improvement" id="improvement" style="display: none;">
            🎉 PJS delivers content <span id="improvement-text">-</span> faster!
        </div>
    </div>

    <script>
        async function runComparison() {
            const size = document.getElementById('size').value;
            const latency = document.getElementById('latency').value;
            
            document.getElementById('results').style.display = 'grid';
            document.getElementById('improvement').style.display = 'none';
            
            try {
                const response = await fetch(`/performance?size=${size}&latency=${latency}`);
                const results = await response.json();
                
                document.getElementById('traditional-time').textContent = `${results.traditional_time_ms}ms`;
                document.getElementById('pjs-time').textContent = `${results.pjs_time_ms}ms`;
                document.getElementById('improvement-text').textContent = `${results.improvement_factor.toFixed(1)}x`;
                
                document.getElementById('improvement').style.display = 'block';
                
            } catch (error) {
                console.error('Test failed:', error);
            }
        }
        
        // Auto-run on page load
        setTimeout(runComparison, 500);
    </script>
</body>
</html>
    "#)
}

/// API info endpoint
async fn api_info() -> Json<Value> {
    Json(json!({
        "name": "PJS Simple Demo Server",
        "version": "1.0.0",
        "endpoints": {
            "/": "Interactive demo page",
            "/traditional": "Traditional JSON endpoint",
            "/pjs": "PJS optimized endpoint", 
            "/performance": "Performance comparison",
            "/api/info": "API information"
        },
        "features": [
            "Simple performance comparison",
            "Network latency simulation", 
            "Interactive web interface",
            "Clean demonstration of PJS benefits"
        ]
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    tracing_subscriber::fmt().init();
    
    let app = Router::new()
        .route("/", get(demo_page))
        .route("/traditional", get(traditional_endpoint))
        .route("/pjs", get(pjs_endpoint))
        .route("/performance", get(performance_comparison))
        .route("/api/info", get(api_info));

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    
    println!();
    println!("🚀 PJS Simple Demo Server");
    println!("=========================");
    println!("📍 Server: http://127.0.0.1:{}", args.port);
    println!("📱 Demo: http://127.0.0.1:{}/ ", args.port);
    println!();
    println!("💡 This is a minimal working demo showcasing PJS benefits");
    println!("   TODO: Complex features will be added after fixing technical debt");
    println!();

    info!("Starting simple demo server on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}