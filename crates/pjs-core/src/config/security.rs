//! Security configuration and limits

use crate::security::compression_bomb::CompressionBombConfig;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Security configuration for the PJS system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// JSON processing limits
    pub json: JsonLimits,
    
    /// Buffer management limits
    pub buffers: BufferLimits,
    
    /// Network and connection limits
    pub network: NetworkLimits,
    
    /// Session management limits
    pub sessions: SessionLimits,
}

/// JSON processing security limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonLimits {
    /// Maximum JSON input size in bytes
    pub max_input_size: usize,
    
    /// Maximum JSON nesting depth
    pub max_depth: usize,
    
    /// Maximum number of keys in a JSON object
    pub max_object_keys: usize,
    
    /// Maximum array length
    pub max_array_length: usize,
    
    /// Maximum string length in JSON
    pub max_string_length: usize,
}

/// Buffer management security limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferLimits {
    /// Maximum individual buffer size
    pub max_buffer_size: usize,
    
    /// Maximum number of buffers in pool
    pub max_pool_size: usize,
    
    /// Maximum total memory for all buffer pools
    pub max_total_memory: usize,
    
    /// Buffer time-to-live before cleanup
    pub buffer_ttl_secs: u64,
    
    /// Maximum buffers per size bucket
    pub max_buffers_per_bucket: usize,
}

/// Network and connection security limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkLimits {
    /// Maximum WebSocket frame size
    pub max_websocket_frame_size: usize,
    
    /// Maximum number of concurrent connections
    pub max_concurrent_connections: usize,
    
    /// Connection timeout in seconds
    pub connection_timeout_secs: u64,
    
    /// Maximum request rate per connection (requests per second)
    pub max_requests_per_second: u32,
    
    /// Maximum payload size for HTTP requests
    pub max_http_payload_size: usize,
    
    /// Rate limiting configuration
    pub rate_limiting: RateLimitingConfig,
    
    /// Compression bomb protection configuration
    pub compression_bomb: CompressionBombConfig,
}

/// Rate limiting configuration for DoS protection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitingConfig {
    /// Maximum requests per time window per IP
    pub max_requests_per_window: u32,
    
    /// Time window for rate limiting in seconds
    pub window_duration_secs: u64,
    
    /// Maximum concurrent connections per IP
    pub max_connections_per_ip: usize,
    
    /// Maximum WebSocket messages per second per connection
    pub max_messages_per_second: u32,
    
    /// Burst allowance (extra messages above rate)
    pub burst_allowance: u32,
    
    /// Enable rate limiting
    pub enabled: bool,
}

/// Session management security limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLimits {
    /// Maximum session ID length
    pub max_session_id_length: usize,
    
    /// Minimum session ID length
    pub min_session_id_length: usize,
    
    /// Maximum streams per session
    pub max_streams_per_session: usize,
    
    /// Session idle timeout in seconds
    pub session_timeout_secs: u64,
    
    /// Maximum session data size
    pub max_session_data_size: usize,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            json: JsonLimits::default(),
            buffers: BufferLimits::default(),
            network: NetworkLimits::default(),
            sessions: SessionLimits::default(),
        }
    }
}

impl Default for JsonLimits {
    fn default() -> Self {
        Self {
            max_input_size: 100 * 1024 * 1024, // 100MB
            max_depth: 64,
            max_object_keys: 10_000,
            max_array_length: 1_000_000,
            max_string_length: 10 * 1024 * 1024, // 10MB
        }
    }
}

impl Default for BufferLimits {
    fn default() -> Self {
        Self {
            max_buffer_size: 256 * 1024 * 1024, // 256MB
            max_pool_size: 1000,
            max_total_memory: 512 * 1024 * 1024, // 512MB
            buffer_ttl_secs: 300, // 5 minutes
            max_buffers_per_bucket: 50,
        }
    }
}

impl Default for NetworkLimits {
    fn default() -> Self {
        Self {
            max_websocket_frame_size: 16 * 1024 * 1024, // 16MB
            max_concurrent_connections: 10_000,
            connection_timeout_secs: 30,
            max_requests_per_second: 100,
            max_http_payload_size: 50 * 1024 * 1024, // 50MB
            rate_limiting: RateLimitingConfig::default(),
            compression_bomb: CompressionBombConfig::default(),
        }
    }
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            max_requests_per_window: 100,
            window_duration_secs: 60,
            max_connections_per_ip: 10,
            max_messages_per_second: 30,
            burst_allowance: 5,
            enabled: true,
        }
    }
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            max_session_id_length: 128,
            min_session_id_length: 8,
            max_streams_per_session: 100,
            session_timeout_secs: 3600, // 1 hour
            max_session_data_size: 100 * 1024 * 1024, // 100MB
        }
    }
}

impl SecurityConfig {
    /// Create a configuration optimized for high-throughput scenarios
    pub fn high_throughput() -> Self {
        Self {
            json: JsonLimits {
                max_input_size: 500 * 1024 * 1024, // 500MB
                max_depth: 128,
                max_object_keys: 50_000,
                max_array_length: 5_000_000,
                max_string_length: 50 * 1024 * 1024, // 50MB
            },
            buffers: BufferLimits {
                max_buffer_size: 1024 * 1024 * 1024, // 1GB
                max_pool_size: 5000,
                max_total_memory: 2 * 1024 * 1024 * 1024, // 2GB
                buffer_ttl_secs: 600, // 10 minutes
                max_buffers_per_bucket: 200,
            },
            network: NetworkLimits {
                max_websocket_frame_size: 100 * 1024 * 1024, // 100MB
                max_concurrent_connections: 50_000,
                connection_timeout_secs: 60,
                max_requests_per_second: 1000,
                max_http_payload_size: 200 * 1024 * 1024, // 200MB
                rate_limiting: RateLimitingConfig {
                    max_requests_per_window: 1000,
                    window_duration_secs: 60,
                    max_connections_per_ip: 50,
                    max_messages_per_second: 100,
                    burst_allowance: 20,
                    enabled: true,
                },
                compression_bomb: CompressionBombConfig::high_throughput(),
            },
            sessions: SessionLimits {
                max_session_id_length: 256,
                min_session_id_length: 16,
                max_streams_per_session: 1000,
                session_timeout_secs: 7200, // 2 hours
                max_session_data_size: 500 * 1024 * 1024, // 500MB
            },
        }
    }

    /// Create a configuration optimized for low-memory environments
    pub fn low_memory() -> Self {
        Self {
            json: JsonLimits {
                max_input_size: 10 * 1024 * 1024, // 10MB
                max_depth: 32,
                max_object_keys: 1_000,
                max_array_length: 100_000,
                max_string_length: 1 * 1024 * 1024, // 1MB
            },
            buffers: BufferLimits {
                max_buffer_size: 10 * 1024 * 1024, // 10MB
                max_pool_size: 100,
                max_total_memory: 50 * 1024 * 1024, // 50MB
                buffer_ttl_secs: 60, // 1 minute
                max_buffers_per_bucket: 10,
            },
            network: NetworkLimits {
                max_websocket_frame_size: 1 * 1024 * 1024, // 1MB
                max_concurrent_connections: 1_000,
                connection_timeout_secs: 15,
                max_requests_per_second: 10,
                max_http_payload_size: 5 * 1024 * 1024, // 5MB
                rate_limiting: RateLimitingConfig {
                    max_requests_per_window: 20,
                    window_duration_secs: 60,
                    max_connections_per_ip: 2,
                    max_messages_per_second: 5,
                    burst_allowance: 2,
                    enabled: true,
                },
                compression_bomb: CompressionBombConfig::low_memory(),
            },
            sessions: SessionLimits {
                max_session_id_length: 64,
                min_session_id_length: 8,
                max_streams_per_session: 10,
                session_timeout_secs: 900, // 15 minutes
                max_session_data_size: 10 * 1024 * 1024, // 10MB
            },
        }
    }

    /// Create a configuration optimized for development/testing
    pub fn development() -> Self {
        Self {
            json: JsonLimits {
                max_input_size: 50 * 1024 * 1024, // 50MB
                max_depth: 64,
                max_object_keys: 5_000,
                max_array_length: 500_000,
                max_string_length: 5 * 1024 * 1024, // 5MB
            },
            buffers: BufferLimits {
                max_buffer_size: 100 * 1024 * 1024, // 100MB
                max_pool_size: 500,
                max_total_memory: 200 * 1024 * 1024, // 200MB
                buffer_ttl_secs: 120, // 2 minutes
                max_buffers_per_bucket: 25,
            },
            network: NetworkLimits {
                max_websocket_frame_size: 10 * 1024 * 1024, // 10MB
                max_concurrent_connections: 1_000,
                connection_timeout_secs: 30,
                max_requests_per_second: 50,
                max_http_payload_size: 25 * 1024 * 1024, // 25MB
                rate_limiting: RateLimitingConfig {
                    max_requests_per_window: 200,
                    window_duration_secs: 60,
                    max_connections_per_ip: 20,
                    max_messages_per_second: 50,
                    burst_allowance: 10,
                    enabled: true,
                },
                compression_bomb: CompressionBombConfig::default(),
            },
            sessions: SessionLimits {
                max_session_id_length: 128,
                min_session_id_length: 8,
                max_streams_per_session: 50,
                session_timeout_secs: 1800, // 30 minutes
                max_session_data_size: 50 * 1024 * 1024, // 50MB
            },
        }
    }

    /// Get buffer TTL as Duration
    pub fn buffer_ttl(&self) -> Duration {
        Duration::from_secs(self.buffers.buffer_ttl_secs)
    }

    /// Get connection timeout as Duration
    pub fn connection_timeout(&self) -> Duration {
        Duration::from_secs(self.network.connection_timeout_secs)
    }

    /// Get session timeout as Duration
    pub fn session_timeout(&self) -> Duration {
        Duration::from_secs(self.sessions.session_timeout_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_security_config() {
        let config = SecurityConfig::default();
        
        // Test reasonable defaults
        assert!(config.json.max_input_size > 0);
        assert!(config.buffers.max_buffer_size > 0);
        assert!(config.network.max_concurrent_connections > 0);
        assert!(config.sessions.max_session_id_length >= config.sessions.min_session_id_length);
    }

    #[test]
    fn test_high_throughput_config() {
        let config = SecurityConfig::high_throughput();
        let default = SecurityConfig::default();
        
        // High throughput should have higher limits
        assert!(config.json.max_input_size >= default.json.max_input_size);
        assert!(config.buffers.max_total_memory >= default.buffers.max_total_memory);
        assert!(config.network.max_concurrent_connections >= default.network.max_concurrent_connections);
    }

    #[test]
    fn test_low_memory_config() {
        let config = SecurityConfig::low_memory();
        let default = SecurityConfig::default();
        
        // Low memory should have lower limits
        assert!(config.json.max_input_size <= default.json.max_input_size);
        assert!(config.buffers.max_total_memory <= default.buffers.max_total_memory);
        assert!(config.buffers.max_buffers_per_bucket <= default.buffers.max_buffers_per_bucket);
    }

    #[test]
    fn test_duration_conversions() {
        let config = SecurityConfig::default();
        
        assert!(config.buffer_ttl().as_secs() > 0);
        assert!(config.connection_timeout().as_secs() > 0);
        assert!(config.session_timeout().as_secs() > 0);
    }
}