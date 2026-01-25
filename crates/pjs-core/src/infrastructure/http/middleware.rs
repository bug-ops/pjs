//! HTTP middleware for PJS optimization and monitoring

use axum::{
    extract::Request,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
};
use std::time::Instant;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Layer, Service};

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
            if let Some(content_length) = request.headers().get(header::CONTENT_LENGTH)
                && let Ok(length_str) = content_length.to_str()
                && let Ok(length) = length_str.parse::<usize>()
                && length > config.max_request_size
            {
                return Ok(Response::builder()
                    .status(StatusCode::PAYLOAD_TOO_LARGE)
                    .body("Request too large".into())
                    .map_err(|_| Response::new("Failed to build error response".into()))
                    .unwrap_or_else(|err_response| err_response));
            }

            // Process request
            let mut response = inner.call(request).await?;

            // Add performance headers
            if config.enable_metrics {
                let duration = start_time.elapsed();
                if let Ok(duration_value) = HeaderValue::from_str(&duration.as_millis().to_string())
                {
                    response
                        .headers_mut()
                        .insert("X-PJS-Duration-Ms", duration_value);
                }

                let version_value = HeaderValue::from_static(env!("CARGO_PKG_VERSION"));
                response
                    .headers_mut()
                    .insert("X-PJS-Version", version_value);
            }

            // Add compression hints
            if config.enable_compression {
                response
                    .headers_mut()
                    .insert("X-PJS-Compression", HeaderValue::from_static("available"));
            }

            Ok(response)
        })
    }
}

/// Rate limiting configuration for HTTP endpoints
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per time window (default: 100)
    pub max_requests_per_window: u32,
    /// Time window duration (default: 60 seconds)
    pub window_duration: std::time::Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests_per_window: 100,
            window_duration: std::time::Duration::from_secs(60),
        }
    }
}

impl RateLimitConfig {
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            max_requests_per_window: requests_per_minute,
            window_duration: std::time::Duration::from_secs(60),
        }
    }

    pub fn with_window(mut self, duration: std::time::Duration) -> Self {
        self.window_duration = duration;
        self
    }
}

/// Rate limiting middleware for PJS endpoints
///
/// Uses token bucket algorithm from security::rate_limit module
/// Returns 429 Too Many Requests when limit exceeded
/// Adds X-RateLimit-* headers per RFC 6585
#[derive(Clone)]
pub struct RateLimitMiddleware {
    limiter: std::sync::Arc<crate::security::rate_limit::WebSocketRateLimiter>,
}

impl RateLimitMiddleware {
    pub fn new(config: RateLimitConfig) -> Self {
        let rate_limit_config = crate::security::rate_limit::RateLimitConfig {
            max_requests_per_window: config.max_requests_per_window,
            window_duration: config.window_duration,
            ..Default::default()
        };

        Self {
            limiter: std::sync::Arc::new(crate::security::rate_limit::WebSocketRateLimiter::new(
                rate_limit_config,
            )),
        }
    }

    pub fn from_limiter(
        limiter: std::sync::Arc<crate::security::rate_limit::WebSocketRateLimiter>,
    ) -> Self {
        Self { limiter }
    }
}

impl<S> Layer<S> for RateLimitMiddleware {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            limiter: self.limiter.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    limiter: std::sync::Arc<crate::security::rate_limit::WebSocketRateLimiter>,
}

impl<S> Service<Request> for RateLimitService<S>
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
        let limiter = self.limiter.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Extract client IP from connection info or X-Forwarded-For header
            let client_ip = extract_client_ip(&request);

            // Check rate limit
            match limiter.check_request(client_ip) {
                Ok(()) => {
                    // Rate limit passed - process request
                    let response = inner.call(request).await?;

                    // Add rate limit headers to response
                    let mut response = response;
                    add_rate_limit_headers(&mut response, &limiter, client_ip);

                    Ok(response)
                }
                Err(err) => {
                    // Rate limit exceeded - return 429
                    let error_body = serde_json::json!({
                        "error": "Too Many Requests",
                        "message": err.to_string(),
                        "retry_after": 60
                    })
                    .to_string();

                    let mut response = Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .header(header::CONTENT_TYPE, "application/json")
                        .header("Retry-After", "60")
                        .body(error_body.into())
                        .unwrap_or_else(|_| Response::new("Too Many Requests".into()));

                    add_rate_limit_headers(&mut response, &limiter, client_ip);

                    Ok(response)
                }
            }
        })
    }
}

/// Extract client IP address from request
///
/// Priority:
/// 1. X-Forwarded-For header (behind proxy)
/// 2. X-Real-IP header
/// 3. Default to localhost (fallback)
fn extract_client_ip(request: &Request) -> std::net::IpAddr {
    use std::net::{IpAddr, Ipv4Addr};

    // Try X-Forwarded-For header
    if let Some(forwarded_for) = request.headers().get("x-forwarded-for") {
        if let Ok(forwarded_str) = forwarded_for.to_str() {
            // Take first IP in the chain
            if let Some(first_ip) = forwarded_str.split(',').next() {
                if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }

    // Try X-Real-IP header
    if let Some(real_ip) = request.headers().get("x-real-ip") {
        if let Ok(ip_str) = real_ip.to_str() {
            if let Ok(ip) = ip_str.parse::<IpAddr>() {
                return ip;
            }
        }
    }

    // Fallback to localhost
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
}

/// Add X-RateLimit-* headers to response per RFC 6585
fn add_rate_limit_headers(
    response: &mut Response,
    limiter: &crate::security::rate_limit::WebSocketRateLimiter,
    client_ip: std::net::IpAddr,
) {
    use std::time::SystemTime;

    // Get stats for the client (we'll need to access internals or add a method)
    // For now, add standard headers with static values
    // TODO: Add method to WebSocketRateLimiter to get current limit status

    response
        .headers_mut()
        .insert("X-RateLimit-Limit", HeaderValue::from_static("100"));

    // Calculate remaining requests (simplified - would need access to client state)
    response
        .headers_mut()
        .insert("X-RateLimit-Remaining", HeaderValue::from_static("99"));

    // Calculate reset time (current time + 60 seconds)
    if let Ok(reset_time) = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() + 60)
    {
        if let Ok(reset_value) = HeaderValue::from_str(&reset_time.to_string()) {
            response
                .headers_mut()
                .insert("X-RateLimit-Reset", reset_value);
        }
    }

    // Suppress unused variable warning
    let _ = (limiter, client_ip);
}

/// Connection upgrade middleware for WebSocket support
pub async fn websocket_upgrade_middleware(
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Check if this is a WebSocket upgrade request
    if headers
        .get(header::UPGRADE)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_lowercase())
        == Some("websocket".to_string())
    {
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
    Response::builder()
        .status(StatusCode::NOT_IMPLEMENTED)
        .body("WebSocket support coming soon".into())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Compression middleware for reducing bandwidth
pub async fn compression_middleware(headers: HeaderMap, request: Request, next: Next) -> Response {
    let accepts_compression = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.contains("gzip") || s.contains("deflate"))
        .unwrap_or(false);

    let mut response = next.run(request).await;

    // Add compression headers if client supports it
    if accepts_compression {
        response.headers_mut().insert(
            "X-PJS-Compression-Available",
            HeaderValue::from_static("gzip,deflate"),
        );

        // In production, would apply actual compression here
        // using tower-http::compression::CompressionLayer
    }

    response
}

/// CORS middleware specifically configured for PJS streaming
pub async fn pjs_cors_middleware(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;

    // Add CORS headers for streaming endpoints
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET,POST,OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type,Authorization,X-PJS-Priority,X-PJS-Format"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("X-PJS-Duration-Ms,X-PJS-Version,X-PJS-Stream-Id"),
    );

    response
}

/// Security middleware for PJS endpoints
pub async fn security_middleware(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;

    // Add security headers
    let headers = response.headers_mut();
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static("default-src 'self'"),
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
pub async fn health_check_middleware(request: Request, next: Next) -> Response {
    // Add health metrics to response headers
    let mut response = next.run(request).await;

    // In production, would check actual service health
    response
        .headers_mut()
        .insert("X-PJS-Health", HeaderValue::from_static("healthy"));

    response
}

/// Content validation middleware configuration
#[derive(Debug, Clone)]
pub struct ContentValidationConfig {
    /// Maximum allowed Content-Length in bytes (default: 10MB)
    pub max_content_length: usize,

    /// Allowed Content-Type values (default: application/json, application/pjs+json)
    pub allowed_content_types: Vec<String>,

    /// Require Content-Type header for POST/PUT/PATCH (default: true)
    pub require_content_type: bool,
}

impl Default for ContentValidationConfig {
    fn default() -> Self {
        Self {
            max_content_length: 10 * 1024 * 1024, // 10MB
            allowed_content_types: vec![
                "application/json".to_string(),
                "application/pjs+json".to_string(),
            ],
            require_content_type: true,
        }
    }
}

/// Content validation middleware handler
///
/// Validates Content-Type and Content-Length headers to prevent:
/// - Unsupported media types (415 error)
/// - Oversized payloads (413 error)
/// - DoS attacks via malformed headers
pub async fn content_validation_middleware(
    config: ContentValidationConfig,
    req: Request,
    next: Next,
) -> Response {
    // Extract method and headers
    let method = req.method().clone();
    let headers = req.headers();

    // Validate Content-Length
    if let Some(content_length_header) = headers.get(header::CONTENT_LENGTH) {
        match content_length_header.to_str() {
            Ok(content_length_str) => match content_length_str.parse::<usize>() {
                Ok(content_length) => {
                    if content_length > config.max_content_length {
                        let error_body = serde_json::json!({
                            "error": "Payload Too Large",
                            "max_size": config.max_content_length,
                            "received_size": content_length
                        })
                        .to_string();

                        return Response::builder()
                            .status(StatusCode::PAYLOAD_TOO_LARGE)
                            .header(header::CONTENT_TYPE, "application/json")
                            .body(error_body.into())
                            .unwrap_or_else(|_| Response::new("Payload Too Large".into()));
                    }
                }
                Err(_) => {
                    let error_body = serde_json::json!({
                        "error": "Invalid Content-Length header"
                    })
                    .to_string();

                    return Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(error_body.into())
                        .unwrap_or_else(|_| Response::new("Bad Request".into()));
                }
            },
            Err(_) => {
                let error_body = serde_json::json!({
                    "error": "Invalid Content-Length header encoding"
                })
                .to_string();

                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(error_body.into())
                    .unwrap_or_else(|_| Response::new("Bad Request".into()));
            }
        }
    }

    // Validate Content-Type for POST/PUT/PATCH requests
    if config.require_content_type && (method == "POST" || method == "PUT" || method == "PATCH") {
        match headers.get(header::CONTENT_TYPE) {
            Some(content_type_header) => {
                let content_type = content_type_header.to_str().unwrap_or("");

                // Extract base content type (ignore charset and other parameters)
                let base_content_type = content_type.split(';').next().unwrap_or("").trim();

                if !config
                    .allowed_content_types
                    .iter()
                    .any(|allowed| base_content_type.eq_ignore_ascii_case(allowed))
                {
                    let error_body = serde_json::json!({
                        "error": "Unsupported Media Type",
                        "accepted": config.allowed_content_types,
                        "received": content_type
                    })
                    .to_string();

                    return Response::builder()
                        .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(error_body.into())
                        .unwrap_or_else(|_| Response::new("Unsupported Media Type".into()));
                }
            }
            None => {
                let error_body = serde_json::json!({
                    "error": "Unsupported Media Type",
                    "message": "Content-Type header is required for POST/PUT/PATCH requests",
                    "accepted": config.allowed_content_types
                })
                .to_string();

                return Response::builder()
                    .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(error_body.into())
                    .unwrap_or_else(|_| Response::new("Unsupported Media Type".into()));
            }
        }
    }

    // All validations passed, continue to next middleware/handler
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_requests_per_window, 100);
        assert_eq!(config.window_duration, std::time::Duration::from_secs(60));
    }

    #[test]
    fn test_rate_limit_config_new() {
        let config = RateLimitConfig::new(50);
        assert_eq!(config.max_requests_per_window, 50);
    }

    #[test]
    fn test_rate_limit_config_with_window() {
        let config = RateLimitConfig::new(100).with_window(std::time::Duration::from_secs(30));
        assert_eq!(config.window_duration, std::time::Duration::from_secs(30));
    }

    #[test]
    fn test_rate_limit_middleware_creation() {
        let config = RateLimitConfig::default();
        let _middleware = RateLimitMiddleware::new(config);
    }

    #[test]
    fn test_content_validation_config_default() {
        let config = ContentValidationConfig::default();
        assert_eq!(config.max_content_length, 10 * 1024 * 1024);
        assert_eq!(config.allowed_content_types.len(), 2);
        assert!(config.require_content_type);
    }
}
