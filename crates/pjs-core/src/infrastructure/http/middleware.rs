//! HTTP middleware for PJS optimization and monitoring

use axum::{
    extract::{ConnectInfo, Request},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;
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
    /// Construct middleware with default settings (compression and metrics enabled, 10 MiB cap).
    pub fn new() -> Self {
        Self {
            enable_compression: true,
            enable_metrics: true,
            max_request_size: 10 * 1024 * 1024, // 10MB
        }
    }

    /// Toggle the `X-PJS-Compression` advertisement header.
    pub fn with_compression(mut self, enabled: bool) -> Self {
        self.enable_compression = enabled;
        self
    }

    /// Toggle the `X-PJS-Duration-Ms` and `X-PJS-Version` response headers.
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.enable_metrics = enabled;
        self
    }

    /// Set the maximum allowed `Content-Length` for incoming requests, in bytes.
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

/// Tower service produced by [`PjsMiddleware`].
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

/// Opt-in trusted-proxy allowlist for the HTTP rate limiter.
///
/// By default, [`RateLimitMiddleware`] keys rate limiting on the real TCP peer
/// address and never trusts `X-Forwarded-For`/`X-Real-IP` — an unauthenticated
/// client could otherwise send a fresh spoofed value on every request to get a
/// fresh rate-limit bucket, fully bypassing the limiter. Set this only for
/// deployments that sit behind a known reverse proxy or load balancer whose
/// peer address(es) are listed here; requests from any other peer always use
/// the real peer address regardless of these headers.
///
/// # Proxy contract
///
/// `X-Forwarded-For` is read right-to-left and takes precedence over
/// `X-Real-IP` when both are present. The trusted proxy must *append* the
/// address it saw the connection from to `X-Forwarded-For` rather than
/// overwrite it (e.g. nginx's `$proxy_add_x_forwarded_for`, or any proxy that
/// merges into a single header line rather than emitting a new one). Proxies
/// that instead emit `<ip>:<port>` or bracketed IPv6 entries are not
/// supported by this simple allowlist — the walk fails closed on the first
/// unparseable entry (falls back to `X-Real-IP`, then the peer address)
/// rather than skipping it and guessing from what remains. Repeated
/// `X-Forwarded-For` header lines are read and treated as one comma-joined
/// list in line order, per RFC 9110.
#[derive(Debug, Clone, Default)]
pub struct TrustedProxyConfig {
    /// Peer addresses (the proxy's own TCP source address) allowed to supply
    /// `X-Forwarded-For`/`X-Real-IP`.
    pub trusted_proxies: Vec<std::net::IpAddr>,
}

impl TrustedProxyConfig {
    /// Build a trusted-proxy config from an explicit allowlist of proxy addresses.
    pub fn new(trusted_proxies: Vec<std::net::IpAddr>) -> Self {
        Self { trusted_proxies }
    }

    /// Whether `ip` (already canonicalized via [`IpAddr::to_canonical`]) is in
    /// the allowlist. Allowlist entries are canonicalized before comparison so
    /// an IPv4 proxy configured as `10.0.0.1` still matches when it arrives as
    /// the IPv4-mapped IPv6 address `::ffff:10.0.0.1` on a dual-stack listener.
    fn contains(&self, ip: std::net::IpAddr) -> bool {
        self.trusted_proxies.iter().any(|p| p.to_canonical() == ip)
    }
}

/// Rate limiting configuration for HTTP endpoints
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per time window (default: 100)
    pub max_requests_per_window: u32,
    /// Time window duration (default: 60 seconds)
    pub window_duration: std::time::Duration,
    /// Opt-in trusted-proxy allowlist. `None` (the default) always keys the
    /// rate limiter on the real TCP peer address. See [`TrustedProxyConfig`].
    pub trusted_proxies: Option<TrustedProxyConfig>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests_per_window: 100,
            window_duration: std::time::Duration::from_secs(60),
            trusted_proxies: None,
        }
    }
}

impl RateLimitConfig {
    /// Build a per-minute rate limit (`requests_per_minute` requests per 60-second window).
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            max_requests_per_window: requests_per_minute,
            window_duration: std::time::Duration::from_secs(60),
            trusted_proxies: None,
        }
    }

    /// Override the window duration that `max_requests_per_window` applies to.
    pub fn with_window(mut self, duration: std::time::Duration) -> Self {
        self.window_duration = duration;
        self
    }

    /// Opt in to trusting `X-Forwarded-For`/`X-Real-IP` from the given proxy allowlist.
    pub fn with_trusted_proxies(mut self, config: TrustedProxyConfig) -> Self {
        self.trusted_proxies = Some(config);
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
    trusted_proxies: Option<TrustedProxyConfig>,
}

impl RateLimitMiddleware {
    /// Build a fresh middleware with its own internal `WebSocketRateLimiter`.
    pub fn new(config: RateLimitConfig) -> Self {
        let trusted_proxies = config.trusted_proxies.clone();
        let rate_limit_config = crate::security::rate_limit::RateLimitConfig {
            max_requests_per_window: config.max_requests_per_window,
            window_duration: config.window_duration,
            ..Default::default()
        };

        Self {
            limiter: std::sync::Arc::new(crate::security::rate_limit::WebSocketRateLimiter::new(
                rate_limit_config,
            )),
            trusted_proxies,
        }
    }

    /// Wrap an externally constructed `WebSocketRateLimiter` (lets several middlewares share state).
    pub fn from_limiter(
        limiter: std::sync::Arc<crate::security::rate_limit::WebSocketRateLimiter>,
    ) -> Self {
        Self {
            limiter,
            trusted_proxies: None,
        }
    }

    /// Opt in to trusting `X-Forwarded-For`/`X-Real-IP` from the given proxy allowlist.
    pub fn with_trusted_proxies(mut self, config: TrustedProxyConfig) -> Self {
        self.trusted_proxies = Some(config);
        self
    }
}

impl<S> Layer<S> for RateLimitMiddleware {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            limiter: self.limiter.clone(),
            trusted_proxies: self.trusted_proxies.clone(),
        }
    }
}

/// Tower service produced by [`RateLimitMiddleware`].
#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    limiter: std::sync::Arc<crate::security::rate_limit::WebSocketRateLimiter>,
    trusted_proxies: Option<TrustedProxyConfig>,
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
        let trusted_proxies = self.trusted_proxies.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let client_ip = extract_client_ip(&request, trusted_proxies.as_ref());

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

/// Extract the client IP address used as the rate-limit key.
///
/// Always trusts the real TCP peer address first, populated via axum's
/// [`ConnectInfo`] extension — the router must be served with
/// `into_make_service_with_connect_info::<SocketAddr>()`, otherwise no
/// `ConnectInfo` extension is present, every client collapses onto a single
/// shared bucket keyed on `127.0.0.1`, and a one-time warning is logged (see
/// [`warn_missing_connect_info`]).
///
/// `X-Forwarded-For`/`X-Real-IP` are only consulted when `trusted_proxies` is
/// set and the real peer address is in its allowlist. Trusting these headers
/// unconditionally would let any client forge a fresh rate-limit bucket on
/// every request, fully bypassing the limiter. Both the peer address and
/// allowlist entries are compared via [`IpAddr::to_canonical`] so an IPv4
/// proxy is still recognized when it arrives IPv4-mapped on a dual-stack
/// listener.
fn extract_client_ip(
    request: &Request,
    trusted_proxies: Option<&TrustedProxyConfig>,
) -> std::net::IpAddr {
    use std::net::{IpAddr, Ipv4Addr};

    let Some(peer) = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip().to_canonical())
    else {
        warn_missing_connect_info();
        return IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    };

    if let Some(proxies) = trusted_proxies
        && proxies.contains(peer)
        && let Some(forwarded_ip) = extract_forwarded_ip(request.headers(), proxies)
    {
        return forwarded_ip;
    }

    peer
}

/// Log once (per process) that a request arrived with no `ConnectInfo`
/// extension, so misconfiguration is loud rather than silently collapsing
/// every client onto one rate-limit bucket.
fn warn_missing_connect_info() {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| {
        tracing::warn!(
            "RateLimitMiddleware: request has no ConnectInfo<SocketAddr> extension; \
             serve the router with `.into_make_service_with_connect_info::<SocketAddr>()` \
             or every client will share a single rate-limit bucket keyed on 127.0.0.1 \
             (logged once)"
        );
    });
}

/// Parse the client IP from `X-Forwarded-For` or `X-Real-IP`.
///
/// Only called for peers already verified against [`TrustedProxyConfig`] —
/// these headers must never be trusted from an unverified peer.
///
/// `X-Forwarded-For` is walked right-to-left — across all header lines with
/// that name, since `HeaderMap` allows repeats and they are semantically one
/// comma-joined list in line order — skipping any entry that is itself a
/// trusted proxy, and returns the first entry that is not. A well-behaved
/// proxy *appends* the address it saw the connection from, so the rightmost
/// non-trusted entry is the one appended by the closest trusted hop and
/// cannot be forged by the client — taking the leftmost (client-supplied)
/// entry instead would let a client behind a trusted proxy forge a fresh
/// value on every request and reopen the exact bypass this module exists to
/// close.
///
/// The walk **fails closed** on the first unparseable entry: it stops and
/// falls through to `X-Real-IP` rather than skipping past the malformed
/// entry into entries further left, which are progressively more
/// attacker-controlled the further left they sit in the chain.
fn extract_forwarded_ip(
    headers: &HeaderMap,
    trusted_proxies: &TrustedProxyConfig,
) -> Option<std::net::IpAddr> {
    let entries: Vec<&str> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|s| s.split(','))
        .collect();

    for entry in entries.into_iter().rev() {
        let Ok(ip) = entry.trim().parse::<std::net::IpAddr>() else {
            break;
        };
        let canonical = ip.to_canonical();
        if !trusted_proxies.contains(canonical) {
            return Some(canonical);
        }
    }

    headers
        .get("x-real-ip")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.trim().parse::<std::net::IpAddr>().ok())
        .map(|ip| ip.to_canonical())
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
    if let Some(reset_value) = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() + 60)
        .and_then(|time| HeaderValue::from_str(&time.to_string()).ok())
    {
        response
            .headers_mut()
            .insert("X-RateLimit-Reset", reset_value);
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
    /// Build with default thresholds (5 failures, 30-second recovery).
    pub fn new() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout_seconds: 30,
        }
    }

    /// Override the consecutive-failure threshold that opens the circuit.
    pub fn with_failure_threshold(mut self, threshold: usize) -> Self {
        self.failure_threshold = threshold;
        self
    }

    /// Override the recovery (cool-down) duration in seconds.
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
