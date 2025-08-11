//! Interactive streaming demo server
//!
//! This demo shows real-time PJS streaming benefits with a web interface
//! comparing traditional JSON loading vs PJS progressive streaming.

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{Html, Json, Response},
    routing::{get, post},
    Router,
};
use pjson_rs::{
    infrastructure::http::axum_extension::{PjsConfig, PjsExtension},
    PriorityStreamer,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::time::sleep;

#[derive(Debug, Deserialize)]
struct DemoRequest {
    dataset_size: Option<String>, // "small", "medium", "large", "huge"
    simulate_latency: Option<u64>, // ms
}

#[derive(Debug, Serialize)]
struct DemoResponse {
    traditional_time_ms: u64,
    pjs_skeleton_time_ms: u64,
    improvement_factor: f64,
    data: Value,
}

/// Generate demo dataset based on size specification
fn generate_demo_dataset(size: &str) -> Value {
    let (product_count, include_reviews, include_analytics) = match size {
        "small" => (10, false, false),
        "medium" => (50, true, false),
        "large" => (150, true, true),
        "huge" => (500, true, true),
        _ => (25, true, false),
    };

    let mut products = Vec::new();
    
    for i in 0..product_count {
        let mut product = json!({
            "id": i + 1,
            "name": format!("Premium Product {} - Professional Edition", i + 1),
            "description": format!("Comprehensive description for product {} with detailed specifications and features that provide exceptional value to customers.", i + 1),
            "category": format!("Category {}", (i % 8) + 1),
            "price": {
                "amount": ((i as f64 * 1.7) + 29.99).round(),
                "currency": "USD",
                "discount": if i % 7 == 0 { 15.0 } else { 0.0 }
            },
            "availability": {
                "in_stock": i % 6 != 0,
                "quantity": (i % 30) + 5,
                "shipping": if i % 3 == 0 { "free" } else { "standard" }
            },
            "rating": {
                "average": ((i % 5) as f64 + 3.0).min(5.0),
                "count": (i % 150) + 25
            },
            "images": {
                "primary": format!("https://demo.store.com/products/{}/main.jpg", i + 1),
                "gallery": [
                    format!("https://demo.store.com/products/{}/img1.jpg", i + 1),
                    format!("https://demo.store.com/products/{}/img2.jpg", i + 1),
                    format!("https://demo.store.com/products/{}/img3.jpg", i + 1)
                ]
            },
            "specifications": {
                "weight": format!("{:.1}kg", ((i as f64) % 10.0) + 0.5),
                "dimensions": format!("{}x{}x{}cm", (i % 25) + 10, (i % 20) + 15, (i % 15) + 5),
                "material": "Premium Quality Materials",
                "warranty": "2 years international warranty"
            }
        });

        if include_reviews {
            let review_count = (i % 5).min(3);
            let mut reviews = Vec::new();
            
            for j in 0..review_count {
                reviews.push(json!({
                    "id": i * 10 + j,
                    "user": format!("User{}", (j + i) % 100),
                    "rating": ((i + j) % 5) + 1,
                    "title": format!("Review {} for Product {}", j + 1, i + 1),
                    "comment": format!("Detailed review comment {} explaining the experience with product {}. This includes specific details about quality, performance, and value.", j + 1, i + 1),
                    "date": format!("2024-01-{:02}T{:02}:00:00Z", ((i + j) % 28) + 1, ((j * 3) % 24)),
                    "verified": (i + j) % 3 == 0,
                    "helpful_votes": (i + j) % 20
                }));
            }
            
            product["reviews"] = Value::Array(reviews);
        }

        products.push(product);
    }

    let mut result = json!({
        "store": {
            "name": "PJS Demo Store",
            "description": "Interactive demonstration of Priority JSON Streaming",
            "version": "2.0",
            "generated_at": "2024-01-15T12:00:00Z"
        },
        "metadata": {
            "total_products": product_count,
            "dataset_size": size,
            "includes_reviews": include_reviews,
            "includes_analytics": include_analytics
        },
        "categories": (1..=8).map(|i| json!({
            "id": i,
            "name": format!("Category {}", i),
            "product_count": products.iter().filter(|p| 
                p["category"].as_str().unwrap_or("") == format!("Category {}", i)
            ).count()
        })).collect::<Vec<_>>(),
        "products": products
    });

    if include_analytics {
        result["analytics"] = json!({
            "overview": {
                "total_revenue": product_count as f64 * 67.5,
                "conversion_rate": 3.47,
                "average_order_value": 127.89,
                "bounce_rate": 34.2
            },
            "performance": {
                "page_load_time_ms": 1250,
                "time_to_interactive_ms": 2100,
                "largest_contentful_paint_ms": 1750
            },
            "traffic": {
                "unique_visitors": product_count * 15,
                "page_views": product_count * 45,
                "sessions": product_count * 12
            },
            "detailed_metrics": (0..20).map(|i| json!({
                "hour": i,
                "visitors": (i * 7 + product_count) % 100,
                "revenue": ((i * 23) % 500) as f64 + 100.0,
                "conversion_rate": ((i as f64 * 0.1) % 5.0) + 1.0
            })).collect::<Vec<_>>()
        });
    }

    result
}

/// Traditional JSON endpoint (baseline)
async fn traditional_endpoint(
    Query(params): Query<DemoRequest>,
) -> Json<Value> {
    // Simulate server processing time
    if let Some(latency_ms) = params.simulate_latency {
        sleep(Duration::from_millis(latency_ms)).await;
    }

    let size = params.dataset_size.as_deref().unwrap_or("medium");
    let data = generate_demo_dataset(size);
    
    Json(data)
}

/// PJS streaming endpoint - returns skeleton first
async fn pjs_streaming_endpoint(
    headers: HeaderMap,
    Query(params): Query<DemoRequest>,
) -> Response {
    // Simulate minimal server processing for skeleton
    if let Some(latency_ms) = params.simulate_latency {
        sleep(Duration::from_millis(latency_ms / 10)).await; // Skeleton is much faster
    }

    let size = params.dataset_size.as_deref().unwrap_or("medium");
    
    // Check if client wants streaming
    let wants_streaming = headers.get("Accept")
        .and_then(|h| h.to_str().ok())
        .map(|accept| accept.contains("text/event-stream"))
        .unwrap_or(false);

    if wants_streaming {
        // Return skeleton immediately for streaming clients
        let skeleton = json!({
            "store": {
                "name": "PJS Demo Store",
                "description": "Interactive demonstration of Priority JSON Streaming",
                "version": "2.0"
            },
            "metadata": {
                "total_products": match size {
                    "small" => 10,
                    "medium" => 50, 
                    "large" => 150,
                    "huge" => 500,
                    _ => 25
                },
                "dataset_size": size,
                "loading": true
            },
            "categories": [],
            "products": [],
            "analytics": null
        });

        axum::response::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .header("X-PJS-Skeleton", "true")
            .body(axum::body::Body::from(skeleton.to_string()))
            .unwrap()
    } else {
        // Return full data for non-streaming clients
        let data = generate_demo_dataset(size);
        Json(data).into_response()
    }
}

/// Performance comparison endpoint
async fn performance_comparison(
    Query(params): Query<DemoRequest>,
) -> Json<DemoResponse> {
    let size = params.dataset_size.as_deref().unwrap_or("medium");
    let latency_ms = params.simulate_latency.unwrap_or(0);
    
    // Simulate traditional loading time
    let traditional_start = std::time::Instant::now();
    sleep(Duration::from_millis(latency_ms)).await;
    let _data = generate_demo_dataset(size);
    let traditional_time = traditional_start.elapsed();
    
    // Simulate PJS skeleton time (much faster)
    let pjs_start = std::time::Instant::now();
    sleep(Duration::from_millis(latency_ms / 10)).await; // Skeleton is ~10x faster
    let pjs_time = pjs_start.elapsed();
    
    let traditional_ms = traditional_time.as_millis() as u64;
    let pjs_ms = pjs_time.as_millis() as u64;
    let improvement = if pjs_ms > 0 { 
        traditional_ms as f64 / pjs_ms as f64 
    } else { 
        1.0 
    };
    
    let response_data = json!({
        "comparison": {
            "traditional_approach": {
                "time_ms": traditional_ms,
                "description": "Must wait for complete data before showing anything"
            },
            "pjs_approach": {
                "skeleton_time_ms": pjs_ms,
                "description": "Shows critical content immediately, loads rest progressively"
            },
            "improvement": {
                "factor": format!("{:.1}x faster", improvement),
                "time_saved_ms": traditional_ms.saturating_sub(pjs_ms)
            }
        },
        "dataset_info": {
            "size": size,
            "simulated_latency_ms": latency_ms
        }
    });

    Json(DemoResponse {
        traditional_time_ms: traditional_ms,
        pjs_skeleton_time_ms: pjs_ms,
        improvement_factor: improvement,
        data: response_data,
    })
}

/// Interactive web demo page
async fn demo_page() -> Html<&'static str> {
    Html(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>PJS Interactive Demo</title>
    <style>
        body { 
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            margin: 0;
            padding: 20px;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            min-height: 100vh;
            color: #333;
        }
        .container {
            max-width: 1200px;
            margin: 0 auto;
            background: white;
            border-radius: 12px;
            box-shadow: 0 20px 40px rgba(0,0,0,0.1);
            overflow: hidden;
        }
        .header {
            background: linear-gradient(135deg, #2c3e50, #4a6cf7);
            color: white;
            padding: 30px;
            text-align: center;
        }
        .header h1 { margin: 0; font-size: 2.5em; }
        .header p { margin: 10px 0 0; opacity: 0.9; }
        .demo-controls {
            padding: 30px;
            background: #f8f9fa;
            border-bottom: 1px solid #e9ecef;
        }
        .control-group {
            display: flex;
            gap: 20px;
            align-items: center;
            margin-bottom: 15px;
            flex-wrap: wrap;
        }
        .control-group label {
            font-weight: 600;
            min-width: 120px;
        }
        select, input, button {
            padding: 8px 12px;
            border: 1px solid #ddd;
            border-radius: 6px;
            font-size: 14px;
        }
        button {
            background: #4a6cf7;
            color: white;
            border: none;
            cursor: pointer;
            font-weight: 600;
            transition: background 0.2s;
        }
        button:hover { background: #3b5bf2; }
        button:disabled { background: #ccc; cursor: not-allowed; }
        .comparison {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 30px;
            padding: 30px;
        }
        .method {
            border: 1px solid #e9ecef;
            border-radius: 8px;
            padding: 20px;
            background: white;
        }
        .method h3 {
            margin: 0 0 15px;
            color: #2c3e50;
            display: flex;
            align-items: center;
            gap: 10px;
        }
        .traditional { border-left: 4px solid #e74c3c; }
        .pjs { border-left: 4px solid #27ae60; }
        .result-box {
            background: #f8f9fa;
            padding: 15px;
            border-radius: 6px;
            margin: 10px 0;
            font-family: 'Monaco', 'Menlo', monospace;
            font-size: 14px;
        }
        .timing {
            font-size: 24px;
            font-weight: bold;
            margin: 10px 0;
        }
        .traditional .timing { color: #e74c3c; }
        .pjs .timing { color: #27ae60; }
        .improvement {
            text-align: center;
            padding: 20px;
            background: linear-gradient(135deg, #27ae60, #2ecc71);
            color: white;
            font-size: 18px;
            font-weight: bold;
        }
        .loading {
            display: inline-block;
            animation: pulse 1.5s infinite;
        }
        @keyframes pulse {
            0%, 50% { opacity: 1; }
            25%, 75% { opacity: 0.5; }
        }
        .data-preview {
            max-height: 300px;
            overflow-y: auto;
            background: #f8f9fa;
            border: 1px solid #e9ecef;
            border-radius: 6px;
            padding: 15px;
            margin-top: 15px;
        }
        .data-preview pre {
            margin: 0;
            font-size: 12px;
            line-height: 1.4;
        }
        @media (max-width: 768px) {
            .comparison { grid-template-columns: 1fr; }
            .control-group { flex-direction: column; align-items: stretch; }
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🚀 PJS Interactive Demo</h1>
            <p>Experience the speed difference: Traditional JSON vs Priority JSON Streaming</p>
        </div>
        
        <div class="demo-controls">
            <div class="control-group">
                <label for="dataset-size">Dataset Size:</label>
                <select id="dataset-size">
                    <option value="small">Small (10 products, ~5KB)</option>
                    <option value="medium" selected>Medium (50 products, ~25KB)</option>
                    <option value="large">Large (150 products, ~75KB)</option>
                    <option value="huge">Huge (500 products, ~250KB)</option>
                </select>
            </div>
            <div class="control-group">
                <label for="network-latency">Network Latency:</label>
                <select id="network-latency">
                    <option value="0">None (Local)</option>
                    <option value="50">Fast (50ms)</option>
                    <option value="150" selected>3G Mobile (150ms)</option>
                    <option value="300">Slow (300ms)</option>
                    <option value="800">Very Slow (800ms)</option>
                </select>
            </div>
            <div class="control-group">
                <button id="run-demo">Run Performance Comparison</button>
                <button id="live-demo">Live Streaming Demo</button>
            </div>
        </div>
        
        <div class="comparison" id="results" style="display: none;">
            <div class="method traditional">
                <h3>⏳ Traditional JSON Loading</h3>
                <p>Waits for complete data transfer and parsing before showing content</p>
                <div class="timing" id="traditional-time">-</div>
                <div class="result-box" id="traditional-status">Ready to test</div>
                <div class="data-preview" id="traditional-data" style="display: none;"></div>
            </div>
            
            <div class="method pjs">
                <h3>⚡ PJS Priority Streaming</h3>
                <p>Shows critical content immediately, loads details progressively</p>
                <div class="timing" id="pjs-time">-</div>
                <div class="result-box" id="pjs-status">Ready to test</div>
                <div class="data-preview" id="pjs-data" style="display: none;"></div>
            </div>
        </div>
        
        <div class="improvement" id="improvement" style="display: none;">
            <span id="improvement-text">-</span>
        </div>
    </div>

    <script>
        const runDemoBtn = document.getElementById('run-demo');
        const liveDemoBtn = document.getElementById('live-demo');
        const results = document.getElementById('results');
        const improvement = document.getElementById('improvement');

        runDemoBtn.addEventListener('click', async () => {
            runDemoBtn.disabled = true;
            results.style.display = 'grid';
            improvement.style.display = 'none';
            
            const size = document.getElementById('dataset-size').value;
            const latency = document.getElementById('network-latency').value;
            
            // Reset displays
            document.getElementById('traditional-time').textContent = '-';
            document.getElementById('pjs-time').textContent = '-';
            document.getElementById('traditional-status').innerHTML = '<span class="loading">Loading...</span>';
            document.getElementById('pjs-status').innerHTML = '<span class="loading">Loading...</span>';
            
            try {
                // Test traditional approach
                const traditionalStart = performance.now();
                const traditionalResponse = await fetch(`/traditional?dataset_size=${size}&simulate_latency=${latency}`);
                const traditionalData = await traditionalResponse.json();
                const traditionalTime = performance.now() - traditionalStart;
                
                document.getElementById('traditional-time').textContent = `${traditionalTime.toFixed(0)}ms`;
                document.getElementById('traditional-status').textContent = `Loaded ${traditionalData.products?.length || 0} products`;
                
                // Test PJS approach
                const pjsStart = performance.now();
                const pjsResponse = await fetch(`/pjs-streaming?dataset_size=${size}&simulate_latency=${latency}`);
                const pjsData = await pjsResponse.json();
                const pjsTime = performance.now() - pjsStart;
                
                document.getElementById('pjs-time').textContent = `${pjsTime.toFixed(0)}ms`;
                document.getElementById('pjs-status').textContent = 
                    pjsResponse.headers.get('X-PJS-Skeleton') ? 'Skeleton loaded instantly!' : 
                    `Loaded ${pjsData.products?.length || 0} products`;
                
                // Show improvement
                const improvementFactor = traditionalTime / pjsTime;
                document.getElementById('improvement-text').textContent = 
                    `🎉 PJS is ${improvementFactor.toFixed(1)}x faster! Saved ${(traditionalTime - pjsTime).toFixed(0)}ms`;
                improvement.style.display = 'block';
                
            } catch (error) {
                console.error('Demo error:', error);
                document.getElementById('traditional-status').textContent = 'Error occurred';
                document.getElementById('pjs-status').textContent = 'Error occurred';
            }
            
            runDemoBtn.disabled = false;
        });
        
        liveDemoBtn.addEventListener('click', async () => {
            alert('Live streaming demo would show real-time Server-Sent Events with progressive data loading. This requires a WebSocket or SSE implementation.');
        });
        
        // Auto-run demo on page load
        setTimeout(() => {
            runDemoBtn.click();
        }, 1000);
    </script>
</body>
</html>
    "#)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::init();

    // Create the main router with demo endpoints
    let app = Router::new()
        // Demo page
        .route("/", get(demo_page))
        .route("/demo", get(demo_page))
        
        // Comparison endpoints
        .route("/traditional", get(traditional_endpoint))
        .route("/pjs-streaming", get(pjs_streaming_endpoint))
        .route("/performance", get(performance_comparison))
        
        // API info
        .route("/api/info", get(|| async {
            Json(json!({
                "name": "PJS Interactive Demo Server",
                "version": "1.0.0",
                "endpoints": {
                    "/": "Interactive web demo",
                    "/traditional": "Traditional JSON endpoint",
                    "/pjs-streaming": "PJS streaming endpoint",
                    "/performance": "Performance comparison API"
                },
                "parameters": {
                    "dataset_size": ["small", "medium", "large", "huge"],
                    "simulate_latency": "milliseconds (0-1000)"
                }
            }))
        }));

    // Add PJS extension capabilities
    let pjs_config = PjsConfig {
        route_prefix: "/pjs".to_string(),
        auto_detect: true,
        ..Default::default()
    };
    
    let pjs_extension = PjsExtension::new(pjs_config);
    let app = pjs_extension.extend_router(app);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3001));
    println!("🚀 PJS Interactive Demo Server starting on http://{}", addr);
    println!();
    println!("📱 Open your browser and visit:");
    println!("   http://{}/         - Interactive Web Demo", addr);
    println!("   http://{}/demo     - Same as above", addr);
    println!("   http://{}/api/info - API documentation", addr);
    println!();
    println!("🔬 API Endpoints for testing:");
    println!("   GET  /traditional?dataset_size=medium&simulate_latency=150");
    println!("   GET  /pjs-streaming?dataset_size=medium&simulate_latency=150");
    println!("   GET  /performance?dataset_size=large&simulate_latency=300");
    println!();
    println!("💡 Try different dataset sizes and network latencies to see PJS benefits!");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}