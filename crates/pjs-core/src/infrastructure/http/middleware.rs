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

/// Rate limiting middleware for PJS endpoints
#[derive(Clone)]
pub struct RateLimitMiddleware {
    #[allow(dead_code)] // Future feature: rate limiting implementation
    requests_per_minute: u32,
    #[allow(dead_code)] // Future feature: rate limiting implementation
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
    fn test_rate_limit_creation() {
        let rate_limit = RateLimitMiddleware::new(100);
        assert_eq!(rate_limit.requests_per_minute, 100);
        assert_eq!(rate_limit.burst_size, 25);
    }

    #[test]
    fn test_content_validation_config_default() {
        let config = ContentValidationConfig::default();
        assert_eq!(config.max_content_length, 10 * 1024 * 1024);
        assert_eq!(config.allowed_content_types.len(), 2);
        assert!(config.require_content_type);
    }
}
