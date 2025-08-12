// Framework helper utilities and macros
//
// This module provides utilities and macros to simplify integration
// with popular Rust web frameworks.

use super::{UniversalRequest, UniversalResponse, IntegrationResult};
use std::collections::HashMap;

/// Helper trait for converting framework-specific types to universal types
pub trait IntoUniversalRequest {
    /// Convert to universal request format
    fn into_universal(self) -> IntegrationResult<UniversalRequest>;
}

/// Helper trait for converting universal responses to framework-specific types
pub trait FromUniversalResponse<T> {
    /// Convert from universal response format
    fn from_universal(response: UniversalResponse) -> IntegrationResult<T>;
}

/// Common HTTP methods
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    /// Convert to string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}

impl From<&str> for HttpMethod {
    fn from(method: &str) -> Self {
        match method.to_uppercase().as_str() {
            "GET" => Self::Get,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            _ => Self::Get, // Default fallback
        }
    }
}

/// Common HTTP status codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpStatus {
    Ok = 200,
    Created = 201,
    Accepted = 202,
    NoContent = 204,
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    Conflict = 409,
    UnprocessableEntity = 422,
    InternalServerError = 500,
    NotImplemented = 501,
    BadGateway = 502,
    ServiceUnavailable = 503,
}

impl HttpStatus {
    /// Get the status code as u16
    pub fn code(&self) -> u16 {
        *self as u16
    }

    /// Get the reason phrase
    pub fn reason_phrase(&self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Created => "Created",
            Self::Accepted => "Accepted",
            Self::NoContent => "No Content",
            Self::BadRequest => "Bad Request",
            Self::Unauthorized => "Unauthorized",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::Conflict => "Conflict",
            Self::UnprocessableEntity => "Unprocessable Entity",
            Self::InternalServerError => "Internal Server Error",
            Self::NotImplemented => "Not Implemented",
            Self::BadGateway => "Bad Gateway",
            Self::ServiceUnavailable => "Service Unavailable",
        }
    }
}

/// Utility for extracting common request information
pub struct RequestExtractor;

impl RequestExtractor {
    /// Extract query parameters from a URL
    pub fn extract_query_params(url: &str) -> HashMap<String, String> {
        let mut params = HashMap::new();
        
        if let Some(query_start) = url.find('?') {
            let query = &url[query_start + 1..];
            for pair in query.split('&') {
                if let Some(eq_pos) = pair.find('=') {
                    let key = &pair[..eq_pos];
                    let value = &pair[eq_pos + 1..];
                    params.insert(
                        urlencoding::decode(key).unwrap_or_default().to_string(),
                        urlencoding::decode(value).unwrap_or_default().to_string(),
                    );
                } else if !pair.is_empty() {
                    params.insert(
                        urlencoding::decode(pair).unwrap_or_default().to_string(),
                        String::new(),
                    );
                }
            }
        }
        
        params
    }

    /// Extract path from URL
    pub fn extract_path(url: &str) -> String {
        if let Some(query_start) = url.find('?') {
            url[..query_start].to_string()
        } else {
            url.to_string()
        }
    }

    /// Parse Content-Type header
    pub fn parse_content_type(content_type: &str) -> (String, HashMap<String, String>) {
        let mut parts = content_type.split(';');
        let mime_type = parts.next().unwrap_or("").trim().to_string();
        let mut params = HashMap::new();

        for part in parts {
            if let Some(eq_pos) = part.find('=') {
                let key = part[..eq_pos].trim().to_string();
                let value = part[eq_pos + 1..].trim().trim_matches('"').to_string();
                params.insert(key, value);
            }
        }

        (mime_type, params)
    }
}

/// Utility for building responses
pub struct ResponseBuilder {
    response: UniversalResponse,
}

impl ResponseBuilder {
    /// Create a new response builder
    pub fn new() -> Self {
        Self {
            response: UniversalResponse::json(crate::domain::value_objects::JsonData::Null),
        }
    }

    /// Set the status code
    pub fn status(mut self, status: HttpStatus) -> Self {
        self.response.status_code = status.code();
        self
    }

    /// Add a header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.response.headers.insert(name.into(), value.into());
        self
    }

    /// Set the content type
    pub fn content_type(mut self, content_type: impl Into<String>) -> Self {
        self.response.content_type = content_type.into();
        self
    }

    /// Set JSON body
    pub fn json(mut self, data: crate::domain::value_objects::JsonData) -> Self {
        self.response.body = super::ResponseBody::Json(data);
        self.response.content_type = "application/json".to_string();
        self
    }

    /// Set binary body
    pub fn binary(mut self, data: Vec<u8>) -> Self {
        self.response.body = super::ResponseBody::Binary(data);
        self.response.content_type = "application/octet-stream".to_string();
        self
    }

    /// Build the response
    pub fn build(self) -> UniversalResponse {
        self.response
    }
}

impl Default for ResponseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Common CORS headers
pub fn default_cors_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("Access-Control-Allow-Origin".to_string(), "*".to_string());
    headers.insert("Access-Control-Allow-Methods".to_string(), "GET, POST, PUT, DELETE, OPTIONS".to_string());
    headers.insert("Access-Control-Allow-Headers".to_string(), "Content-Type, Authorization, Accept".to_string());
    headers.insert("Access-Control-Max-Age".to_string(), "86400".to_string());
    headers
}

/// Common security headers
pub fn default_security_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("X-Content-Type-Options".to_string(), "nosniff".to_string());
    headers.insert("X-Frame-Options".to_string(), "DENY".to_string());
    headers.insert("X-XSS-Protection".to_string(), "1; mode=block".to_string());
    headers.insert("Referrer-Policy".to_string(), "strict-origin-when-cross-origin".to_string());
    headers
}

/// Macro for creating framework-specific adapters
#[macro_export]
macro_rules! create_framework_adapter {
    ($framework:ident, $request:ty, $response:ty, $error:ty) => {
        /// Framework-specific adapter implementation
        pub struct $framework {
            base: $crate::infrastructure::integration::UniversalAdapter<$request, $response, $error>,
        }

        impl $framework {
            /// Create a new adapter
            pub fn new() -> Self {
                Self {
                    base: $crate::infrastructure::integration::UniversalAdapter::new(),
                }
            }

            /// Create adapter with configuration
            pub fn with_config(
                config: $crate::infrastructure::integration::AdapterConfig,
            ) -> Self {
                Self {
                    base: $crate::infrastructure::integration::UniversalAdapter::with_config(config),
                }
            }
        }

        impl Default for $framework {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_conversion() {
        assert_eq!(HttpMethod::from("GET"), HttpMethod::Get);
        assert_eq!(HttpMethod::from("post"), HttpMethod::Post);
        assert_eq!(HttpMethod::from("INVALID"), HttpMethod::Get);
    }

    #[test]
    fn test_http_status() {
        assert_eq!(HttpStatus::Ok.code(), 200);
        assert_eq!(HttpStatus::NotFound.code(), 404);
        assert_eq!(HttpStatus::InternalServerError.reason_phrase(), "Internal Server Error");
    }

    #[test]
    fn test_query_param_extraction() {
        let params = RequestExtractor::extract_query_params("/api/test?foo=bar&baz=qux");
        assert_eq!(params.get("foo"), Some(&"bar".to_string()));
        assert_eq!(params.get("baz"), Some(&"qux".to_string()));
    }

    #[test]
    fn test_path_extraction() {
        assert_eq!(
            RequestExtractor::extract_path("/api/test?foo=bar"),
            "/api/test"
        );
        assert_eq!(
            RequestExtractor::extract_path("/api/test"),
            "/api/test"
        );
    }

    #[test]
    fn test_content_type_parsing() {
        let (mime_type, params) = RequestExtractor::parse_content_type(
            "application/json; charset=utf-8"
        );
        assert_eq!(mime_type, "application/json");
        assert_eq!(params.get("charset"), Some(&"utf-8".to_string()));
    }

    #[test]
    fn test_response_builder() {
        let response = ResponseBuilder::new()
            .status(HttpStatus::Created)
            .header("X-Test", "test")
            .content_type("application/json")
            .build();

        assert_eq!(response.status_code, 201);
        assert_eq!(response.headers.get("X-Test"), Some(&"test".to_string()));
        assert_eq!(response.content_type, "application/json");
    }

    #[test]
    fn test_cors_headers() {
        let headers = default_cors_headers();
        assert!(headers.contains_key("Access-Control-Allow-Origin"));
        assert!(headers.contains_key("Access-Control-Allow-Methods"));
    }
}