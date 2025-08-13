//! WebSocket security integration with rate limiting

use crate::security::{WebSocketRateLimiter, RateLimitConfig, RateLimitError, RateLimitGuard};
use std::net::IpAddr;
use std::sync::Arc;
use tracing::{warn, error, info};

/// Security-enhanced WebSocket handler with rate limiting
#[derive(Debug)]
pub struct SecureWebSocketHandler {
    rate_limiter: Arc<WebSocketRateLimiter>,
}

impl SecureWebSocketHandler {
    /// Create new secure handler with rate limiting configuration
    pub fn new(rate_limit_config: RateLimitConfig) -> Self {
        let rate_limiter = Arc::new(WebSocketRateLimiter::new(rate_limit_config));
        
        // Start background cleanup task
        let limiter_cleanup = rate_limiter.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300)); // 5 minutes
            loop {
                interval.tick().await;
                limiter_cleanup.cleanup_expired();
                info!("Rate limiter cleanup completed");
            }
        });
        
        Self { rate_limiter }
    }
    
    /// Create with default security configuration
    pub fn with_default_security() -> Self {
        Self::new(RateLimitConfig::default())
    }
    
    /// Create with high-traffic configuration
    pub fn with_high_traffic_security() -> Self {
        Self::new(RateLimitConfig::high_traffic())
    }
    
    /// Create with low-resource configuration
    pub fn with_low_resource_security() -> Self {
        Self::new(RateLimitConfig::low_resource())
    }
    
    /// Check if HTTP upgrade request is allowed
    pub fn check_upgrade_request(&self, client_ip: IpAddr) -> Result<(), RateLimitError> {
        match self.rate_limiter.check_request(client_ip) {
            Ok(()) => {
                info!("WebSocket upgrade request allowed for IP: {}", client_ip);
                Ok(())
            }
            Err(e) => {
                warn!("WebSocket upgrade request denied for IP {}: {}", client_ip, e);
                Err(e)
            }
        }
    }
    
    /// Create connection guard for a new WebSocket connection
    pub fn create_connection_guard(&self, client_ip: IpAddr) -> Result<RateLimitGuard, RateLimitError> {
        match RateLimitGuard::new(self.rate_limiter.clone(), client_ip) {
            Ok(guard) => {
                info!("WebSocket connection established for IP: {}", client_ip);
                Ok(guard)
            }
            Err(e) => {
                warn!("WebSocket connection denied for IP {}: {}", client_ip, e);
                Err(e)
            }
        }
    }
    
    /// Validate WebSocket message before processing
    pub fn validate_message(&self, guard: &RateLimitGuard, frame_size: usize) -> Result<(), RateLimitError> {
        match guard.check_message(frame_size) {
            Ok(()) => Ok(()),
            Err(e) => {
                error!("WebSocket message validation failed: {}", e);
                Err(e)
            }
        }
    }
    
    /// Get current rate limiting statistics
    pub fn get_security_stats(&self) -> crate::security::RateLimitStats {
        self.rate_limiter.get_stats()
    }
    
    /// Force cleanup of expired rate limit entries
    pub fn force_cleanup(&self) {
        self.rate_limiter.cleanup_expired();
    }
}

impl Clone for SecureWebSocketHandler {
    fn clone(&self) -> Self {
        Self {
            rate_limiter: self.rate_limiter.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    
    #[tokio::test]
    async fn test_secure_websocket_handler() {
        let handler = SecureWebSocketHandler::with_default_security();
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        
        // Should allow upgrade request
        assert!(handler.check_upgrade_request(ip).is_ok());
        
        // Should allow connection
        let guard = handler.create_connection_guard(ip).unwrap();
        
        // Should allow message
        assert!(handler.validate_message(&guard, 1024).is_ok());
        
        // Get stats
        let stats = handler.get_security_stats();
        assert!(stats.total_clients > 0);
    }
    
    #[tokio::test]
    async fn test_rate_limiting_enforcement() {
        let config = RateLimitConfig {
            max_requests_per_window: 1,
            max_connections_per_ip: 1,
            max_messages_per_second: 1,
            burst_allowance: 0,
            ..Default::default()
        };
        
        let handler = SecureWebSocketHandler::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        
        // First request should succeed
        assert!(handler.check_upgrade_request(ip).is_ok());
        
        // Second request should be rate limited
        assert!(handler.check_upgrade_request(ip).is_err());
        
        // Connection should succeed after first request
        let _guard = handler.create_connection_guard(ip).unwrap();
        
        // Second connection should fail
        assert!(handler.create_connection_guard(ip).is_err());
    }
    
    #[tokio::test]
    async fn test_different_security_levels() {
        let default_handler = SecureWebSocketHandler::with_default_security();
        let high_traffic_handler = SecureWebSocketHandler::with_high_traffic_security();
        let low_resource_handler = SecureWebSocketHandler::with_low_resource_security();
        
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        
        // All should allow initial requests
        assert!(default_handler.check_upgrade_request(ip).is_ok());
        assert!(high_traffic_handler.check_upgrade_request(ip).is_ok());
        assert!(low_resource_handler.check_upgrade_request(ip).is_ok());
        
        // Create connections with different limits
        let _guard1 = default_handler.create_connection_guard(ip).unwrap();
        let _guard2 = high_traffic_handler.create_connection_guard(ip).unwrap();
        let _guard3 = low_resource_handler.create_connection_guard(ip).unwrap();
    }
}