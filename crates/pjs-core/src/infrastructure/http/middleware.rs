//! HTTP middleware for PJS optimization and monitoring

use axum::{
    extract::Request,
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use std::time::Instant;
use tower::{Layer, Service};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Middleware for performance monitoring and optimization
#[derive(Clone)]
pub struct PjsMiddleware {
    enable_compression: bool,
    enable_metrics: bool,
    max_request_size: usize,
}

impl PjsMiddleware {
    pub fn new() -> Self {
        Self {
            enable_compression: true,
            enable_metrics: true,
            max_request_size: 10 * 1024 * 1024, // 10MB
        }
    }
    
    pub fn with_compression(mut self, enabled: bool) -> Self {
        self.enable_compression = enabled;
        self
    }
    
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.enable_metrics = enabled;
        self
    }
    
    pub fn with_max_request_size(mut self, size: usize) -> Self {
        self.max_request_size = size;
        self
    }
}

impl Default for PjsMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for PjsMiddleware {
    type Service = PjsMiddlewareService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PjsMiddlewareService {
            inner,
            config: self.clone(),
        }
    }
}

#[derive(Clone)]
pub struct PjsMiddlewareService<S> {
    inner: S,
    config: PjsMiddleware,
}

impl<S> Service<Request> for PjsMiddlewareService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let mut inner = self.inner.clone();
        let config = self.config.clone();
        
        Box::pin(async move {
            let start_time = Instant::now();
            
            // Check request size
            if let Some(content_length) = request.headers().get(header::CONTENT_LENGTH) {
                if let Ok(length_str) = content_length.to_str() {
                    if let Ok(length) = length_str.parse::<usize>() {
                        if length > config.max_request_size {
                            return Ok(Response::builder()
                                .status(StatusCode::PAYLOAD_TOO_LARGE)
                                .body("Request too large".into())
                                .unwrap());
                        }
                    }
                }
            }
            
            // Process request
            let mut response = inner.call(request).await?;
            
            // Add performance headers
            if config.enable_metrics {
                let duration = start_time.elapsed();
                response.headers_mut().insert(
                    "X-PJS-Duration-Ms",
                    duration.as_millis().to_string().parse().unwrap()
                );
                
                response.headers_mut().insert(
                    "X-PJS-Version",
                    env!("CARGO_PKG_VERSION").parse().unwrap()
                );
            }
            
            // Add compression hints
            if config.enable_compression {
                response.headers_mut().insert(
                    "X-PJS-Compression",
                    "available".parse().unwrap()
                );
            }
            
            Ok(response)
        })
    }
}

/// Rate limiting middleware for PJS endpoints
#[derive(Clone)]
pub struct RateLimitMiddleware {
    requests_per_minute: u32,
    burst_size: u32,
}

impl RateLimitMiddleware {
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            requests_per_minute,
            burst_size: requests_per_minute / 4, // Allow 25% burst
        }
    }
}

/// Connection upgrade middleware for WebSocket support
pub async fn websocket_upgrade_middleware(
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Check if this is a WebSocket upgrade request
    if headers.get(header::UPGRADE)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_lowercase())
        == Some("websocket".to_string()) {
        
        // Handle WebSocket upgrade for PJS streaming
        // This would integrate with the WebSocket handler
        return handle_websocket_upgrade(request).await;
    }
    
    // Continue with regular HTTP handling
    Ok(next.run(request).await)
}

/// Handle WebSocket upgrade for real-time PJS streaming
async fn handle_websocket_upgrade(_request: Request) -> Result<Response, StatusCode> {
    // Placeholder - would implement actual WebSocket upgrade logic
    // using axum-websocket or similar
    Ok(Response::builder()
        .status(StatusCode::NOT_IMPLEMENTED)
        .body("WebSocket support coming soon".into())
        .unwrap())
}

/// Compression middleware for reducing bandwidth
pub async fn compression_middleware(
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let accepts_compression = headers.get(header::ACCEPT_ENCODING)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.contains("gzip") || s.contains("deflate"))
        .unwrap_or(false);
    
    let mut response = next.run(request).await;
    
    // Add compression headers if client supports it
    if accepts_compression {
        response.headers_mut().insert(
            "X-PJS-Compression-Available",
            "gzip,deflate".parse().unwrap()
        );
        
        // In production, would apply actual compression here
        // using tower-http::compression::CompressionLayer
    }
    
    response
}

/// CORS middleware specifically configured for PJS streaming
pub async fn pjs_cors_middleware(
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    
    // Add CORS headers for streaming endpoints
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        "GET,POST,OPTIONS".parse().unwrap()
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        "Content-Type,Authorization,X-PJS-Priority,X-PJS-Format".parse().unwrap()
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        "X-PJS-Duration-Ms,X-PJS-Version,X-PJS-Stream-Id".parse().unwrap()
    );
    
    response
}

/// Security middleware for PJS endpoints
pub async fn security_middleware(
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    
    // Add security headers
    let headers = response.headers_mut();
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    headers.insert("X-Frame-Options", "DENY".parse().unwrap());
    headers.insert(
        "Content-Security-Policy",
        "default-src 'self'".parse().unwrap()
    );
    
    response
}

/// Circuit breaker middleware for resilience
#[derive(Clone)]
pub struct CircuitBreakerMiddleware {
    failure_threshold: usize,
    recovery_timeout_seconds: u64,
}

impl CircuitBreakerMiddleware {
    pub fn new() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout_seconds: 30,
        }
    }
    
    pub fn with_failure_threshold(mut self, threshold: usize) -> Self {
        self.failure_threshold = threshold;
        self
    }
    
    pub fn with_recovery_timeout(mut self, seconds: u64) -> Self {
        self.recovery_timeout_seconds = seconds;
        self
    }
}

impl Default for CircuitBreakerMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

/// Health check middleware that monitors PJS service health
pub async fn health_check_middleware(
    request: Request,
    next: Next,
) -> Response {
    // Add health metrics to response headers
    let mut response = next.run(request).await;
    
    // In production, would check actual service health
    response.headers_mut().insert(
        "X-PJS-Health",
        "healthy".parse().unwrap()
    );
    
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Method};
    
    #[tokio::test]
    async fn test_pjs_middleware_creation() {
        let middleware = PjsMiddleware::new()
            .with_compression(true)
            .with_metrics(true)
            .with_max_request_size(5 * 1024 * 1024);
        
        assert!(middleware.enable_compression);
        assert!(middleware.enable_metrics);
        assert_eq!(middleware.max_request_size, 5 * 1024 * 1024);
    }
    
    #[test]
    fn test_rate_limit_creation() {
        let rate_limit = RateLimitMiddleware::new(100);
        assert_eq!(rate_limit.requests_per_minute, 100);
        assert_eq!(rate_limit.burst_size, 25);
    }
}