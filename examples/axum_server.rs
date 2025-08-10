//! Example HTTP server using Axum integration for PJS streaming
//!
//! This example demonstrates how to set up a complete PJS streaming server
//! using Axum HTTP framework with DDD architecture.

use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use axum::Router;
use tower_http::trace::TraceLayer;

use pjson_rs::{
    application::{
        handlers::{
            command_handler::InMemoryCommandHandler,
            query_handler::InMemoryQueryHandler,
        },
        services::{SessionService, StreamingService},
    },
    domain::{
        aggregates::stream_session::SessionConfig,
        value_objects::SessionId,
    },
    infrastructure::{
        adapters::{
            InMemoryStreamRepository,
            InMemoryStreamStore,
            InMemoryEventPublisher,
            InMemoryMetricsCollector,
        },
        http::{
            axum_adapter::{create_pjs_router, PjsAppState},
            middleware::{
                pjs_cors_middleware,
                security_middleware,
                health_check_middleware,
            },
        },
    },
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::init();

    // Create infrastructure components
    let stream_repository = Arc::new(InMemoryStreamRepository::new());
    let stream_store = Arc::new(InMemoryStreamStore::new());
    let event_publisher = Arc::new(InMemoryEventPublisher::new());
    let metrics_collector = Arc::new(InMemoryMetricsCollector::new());

    // Create CQRS handlers
    let command_handler = Arc::new(InMemoryCommandHandler::new(
        stream_repository.clone(),
        stream_store.clone(),
        event_publisher.clone(),
        metrics_collector.clone(),
    ));

    let query_handler = Arc::new(InMemoryQueryHandler::new(
        stream_repository.clone(),
        stream_store.clone(),
        metrics_collector.clone(),
    ));

    // Create application services
    let session_service = Arc::new(SessionService::new(
        command_handler.clone(),
        query_handler.clone(),
    ));

    let streaming_service = Arc::new(StreamingService::new(
        command_handler.clone(),
    ));

    // Create Axum app state
    let app_state = PjsAppState::new(session_service, streaming_service);

    // Build the router
    let app = Router::new()
        // Include PJS routes
        .merge(create_pjs_router())
        
        // Add example routes
        .nest("/examples", example_routes())
        
        // Global middleware
        .layer(axum::middleware::from_fn(pjs_cors_middleware))
        .layer(axum::middleware::from_fn(security_middleware))
        .layer(axum::middleware::from_fn(health_check_middleware))
        .layer(TraceLayer::new_for_http())
        
        .with_state(app_state);

    // Bind to address
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("🚀 PJS Server starting on http://{}", addr);
    println!("📊 Health check: http://{}/pjs/health", addr);
    println!("📝 API Documentation:");
    println!("   POST http://{}/pjs/sessions - Create new session", addr);
    println!("   GET  http://{}/pjs/sessions/{{id}} - Get session info", addr);
    println!("   POST http://{}/pjs/stream/{{session_id}} - Start streaming", addr);
    println!("   GET  http://{}/pjs/stream/{{session_id}}/sse - Server-sent events", addr);
    println!("   GET  http://{}/examples/demo - Demo data streaming", addr);

    // Start the server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Create example routes for demonstration
fn example_routes<CH, QH>() -> Router<PjsAppState<CH, QH>>
where
    CH: Clone + Send + Sync + 'static,
    QH: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/demo", axum::routing::get(demo_endpoint))
        .route("/metrics", axum::routing::get(metrics_endpoint))
}

/// Demo endpoint that shows how to create and stream data
async fn demo_endpoint<CH, QH>(
    axum::extract::State(state): axum::extract::State<PjsAppState<CH, QH>>,
) -> axum::Json<serde_json::Value>
where
    CH: Clone + Send + Sync + 'static,
    QH: Clone + Send + Sync + 'static,
{
    // This is a simplified demo - in real implementation would use proper handlers
    axum::Json(serde_json::json!({
        "message": "PJS Demo endpoint",
        "instructions": [
            "1. POST /pjs/sessions to create a session",
            "2. POST /pjs/stream/{session_id} with JSON data",
            "3. GET /pjs/stream/{session_id}/sse for real-time streaming"
        ],
        "sample_data": {
            "store": {
                "name": "Demo Store",
                "products": [
                    {"id": 1, "name": "Product A", "price": 19.99},
                    {"id": 2, "name": "Product B", "price": 29.99}
                ],
                "metadata": {
                    "version": "1.0",
                    "last_updated": "2024-01-01T00:00:00Z"
                }
            }
        }
    }))
}

/// Metrics endpoint showing server metrics
async fn metrics_endpoint<CH, QH>(
    axum::extract::State(_state): axum::extract::State<PjsAppState<CH, QH>>,
) -> axum::response::Response<axum::body::Body>
where
    CH: Clone + Send + Sync + 'static,
    QH: Clone + Send + Sync + 'static,
{
    // In real implementation would get metrics from the collector
    let prometheus_metrics = r#"# HELP pjs_demo_requests_total Total demo requests
# TYPE pjs_demo_requests_total counter
pjs_demo_requests_total{endpoint="demo"} 1

# HELP pjs_server_uptime_seconds Server uptime in seconds
# TYPE pjs_server_uptime_seconds gauge
pjs_server_uptime_seconds 300

# HELP pjs_active_sessions Active PJS sessions
# TYPE pjs_active_sessions gauge
pjs_active_sessions 0
"#;

    axum::response::Response::builder()
        .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
        .body(axum::body::Body::from(prometheus_metrics))
        .unwrap()
}