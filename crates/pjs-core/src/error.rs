//! Error types for PJS operations

/// Result type alias for PJS operations
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for PJS operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Invalid JSON syntax
    #[error("Invalid JSON syntax at position {position}: {message}")]
    InvalidJson {
        /// Position in the input where error occurred
        position: usize,
        /// Error description
        message: String,
    },

    /// Frame format error
    #[error("Invalid frame format: {0}")]
    InvalidFrame(String),

    /// Schema validation error
    #[error("Schema validation failed: {0}")]
    SchemaValidation(String),

    /// Semantic type mismatch
    #[error("Semantic type mismatch: expected {expected}, got {actual}")]
    SemanticTypeMismatch {
        /// Expected semantic type
        expected: String,
        /// Actual semantic type
        actual: String,
    },

    /// Buffer overflow or underflow
    #[error("Buffer error: {0}")]
    Buffer(String),

    /// Memory allocation error
    #[error("Memory allocation failed: {0}")]
    Memory(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(String),

    /// Connection failed error
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Client error
    #[error("Client error: {0}")]
    ClientError(String),

    /// Invalid session error
    #[error("Invalid session: {0}")]
    InvalidSession(String),

    /// Invalid URL error
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// UTF-8 conversion error
    #[error("UTF-8 conversion failed: {0}")]
    Utf8(String),

    /// Security violation error
    #[error("Security error: {0}")]
    SecurityError(String),

    /// Generic error for other cases
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Create an invalid JSON error
    pub fn invalid_json(position: usize, message: impl Into<String>) -> Self {
        Self::InvalidJson {
            position,
            message: message.into(),
        }
    }

    /// Create an invalid frame error
    pub fn invalid_frame(message: impl Into<String>) -> Self {
        Self::InvalidFrame(message.into())
    }

    /// Create a schema validation error
    pub fn schema_validation(message: impl Into<String>) -> Self {
        Self::SchemaValidation(message.into())
    }

    /// Create a semantic type mismatch error
    pub fn semantic_type_mismatch(expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::SemanticTypeMismatch {
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Create a buffer error
    pub fn buffer(message: impl Into<String>) -> Self {
        Self::Buffer(message.into())
    }

    /// Create a memory error
    pub fn memory(message: impl Into<String>) -> Self {
        Self::Memory(message.into())
    }

    /// Create a connection failed error
    pub fn connection_failed(message: impl Into<String>) -> Self {
        Self::ConnectionFailed(message.into())
    }

    /// Create a client error
    pub fn client_error(message: impl Into<String>) -> Self {
        Self::ClientError(message.into())
    }

    /// Create an invalid session error
    pub fn invalid_session(message: impl Into<String>) -> Self {
        Self::InvalidSession(message.into())
    }

    /// Create an invalid URL error
    pub fn invalid_url(message: impl Into<String>) -> Self {
        Self::InvalidUrl(message.into())
    }

    /// Create a serialization error
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::Serialization(message.into())
    }

    /// Create a UTF-8 error
    pub fn utf8(message: impl Into<String>) -> Self {
        Self::Utf8(message.into())
    }

    /// Create a security error
    pub fn security_error(message: impl Into<String>) -> Self {
        Self::SecurityError(message.into())
    }

    /// Create a generic error
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other(message.into())
    }

    /// Check if the error is related to JSON parsing
    pub fn is_json_error(&self) -> bool {
        matches!(self, Self::InvalidJson { .. } | Self::Serialization(_))
    }

    /// Check if the error is a network-related error
    pub fn is_network_error(&self) -> bool {
        matches!(self, Self::ConnectionFailed(_) | Self::Io(_))
    }

    /// Check if the error is a validation error
    pub fn is_validation_error(&self) -> bool {
        matches!(
            self,
            Self::SchemaValidation(_) | Self::SemanticTypeMismatch { .. } | Self::InvalidFrame(_)
        )
    }

    /// Check if the error is a security-related error
    pub fn is_security_error(&self) -> bool {
        matches!(self, Self::SecurityError(_))
    }

    /// Get error category as string
    pub fn category(&self) -> &'static str {
        match self {
            Self::InvalidJson { .. } | Self::Serialization(_) => "json",
            Self::InvalidFrame(_)
            | Self::SchemaValidation(_)
            | Self::SemanticTypeMismatch { .. } => "validation",
            Self::Buffer(_) | Self::Memory(_) => "memory",
            Self::Io(_) | Self::ConnectionFailed(_) => "network",
            Self::ClientError(_) | Self::InvalidSession(_) | Self::InvalidUrl(_) => "client",
            Self::Utf8(_) => "encoding",
            Self::SecurityError(_) => "security",
            Self::Other(_) => "other",
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(err: std::str::Utf8Error) -> Self {
        Error::Utf8(err.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Serialization(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation_constructors() {
        let json_err = Error::invalid_json(42, "unexpected token");
        if let Error::InvalidJson { position, message } = json_err {
            assert_eq!(position, 42);
            assert_eq!(message, "unexpected token");
        } else {
            panic!("Expected InvalidJson error");
        }

        let frame_err = Error::invalid_frame("malformed frame");
        assert!(matches!(frame_err, Error::InvalidFrame(_)));

        let schema_err = Error::schema_validation("required field missing");
        assert!(matches!(schema_err, Error::SchemaValidation(_)));

        let type_err = Error::semantic_type_mismatch("string", "number");
        if let Error::SemanticTypeMismatch { expected, actual } = type_err {
            assert_eq!(expected, "string");
            assert_eq!(actual, "number");
        } else {
            panic!("Expected SemanticTypeMismatch error");
        }
    }

    #[test]
    fn test_all_error_constructors() {
        assert!(matches!(Error::buffer("overflow"), Error::Buffer(_)));
        assert!(matches!(
            Error::memory("allocation failed"),
            Error::Memory(_)
        ));
        assert!(matches!(
            Error::connection_failed("timeout"),
            Error::ConnectionFailed(_)
        ));
        assert!(matches!(
            Error::client_error("bad request"),
            Error::ClientError(_)
        ));
        assert!(matches!(
            Error::invalid_session("expired"),
            Error::InvalidSession(_)
        ));
        assert!(matches!(
            Error::invalid_url("malformed"),
            Error::InvalidUrl(_)
        ));
        assert!(matches!(
            Error::serialization("json error"),
            Error::Serialization(_)
        ));
        assert!(matches!(Error::utf8("invalid utf8"), Error::Utf8(_)));
        assert!(matches!(Error::other("unknown"), Error::Other(_)));
    }

    #[test]
    fn test_error_classification() {
        let json_err = Error::invalid_json(0, "test");
        assert!(json_err.is_json_error());
        assert!(!json_err.is_network_error());
        assert!(!json_err.is_validation_error());

        let serialization_err = Error::serialization("test");
        assert!(serialization_err.is_json_error());

        let network_err = Error::connection_failed("test");
        assert!(network_err.is_network_error());
        assert!(!network_err.is_json_error());

        let io_err = Error::Io("test".to_string());
        assert!(io_err.is_network_error());

        let validation_err = Error::schema_validation("test");
        assert!(validation_err.is_validation_error());
        assert!(!validation_err.is_json_error());

        let frame_err = Error::invalid_frame("test");
        assert!(frame_err.is_validation_error());

        let type_err = Error::semantic_type_mismatch("a", "b");
        assert!(type_err.is_validation_error());
    }

    #[test]
    fn test_error_categories() {
        assert_eq!(Error::invalid_json(0, "test").category(), "json");
        assert_eq!(Error::serialization("test").category(), "json");
        assert_eq!(Error::invalid_frame("test").category(), "validation");
        assert_eq!(Error::schema_validation("test").category(), "validation");
        assert_eq!(
            Error::semantic_type_mismatch("a", "b").category(),
            "validation"
        );
        assert_eq!(Error::buffer("test").category(), "memory");
        assert_eq!(Error::memory("test").category(), "memory");
        assert_eq!(Error::Io("test".to_string()).category(), "network");
        assert_eq!(Error::connection_failed("test").category(), "network");
        assert_eq!(Error::client_error("test").category(), "client");
        assert_eq!(Error::invalid_session("test").category(), "client");
        assert_eq!(Error::invalid_url("test").category(), "client");
        assert_eq!(Error::utf8("test").category(), "encoding");
        assert_eq!(Error::other("test").category(), "other");
    }

    #[test]
    fn test_error_display() {
        let json_err = Error::invalid_json(42, "unexpected token");
        assert_eq!(
            json_err.to_string(),
            "Invalid JSON syntax at position 42: unexpected token"
        );

        let type_err = Error::semantic_type_mismatch("string", "number");
        assert_eq!(
            type_err.to_string(),
            "Semantic type mismatch: expected string, got number"
        );

        let frame_err = Error::invalid_frame("malformed");
        assert_eq!(frame_err.to_string(), "Invalid frame format: malformed");
    }

    #[test]
    fn test_from_std_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let pjs_err = Error::from(io_err);

        assert!(matches!(pjs_err, Error::Io(_)));
        assert!(pjs_err.is_network_error());
        assert_eq!(pjs_err.category(), "network");
    }

    #[test]
    fn test_from_utf8_error() {
        // Create invalid UTF-8 dynamically to avoid compiler warning
        let mut invalid_utf8 = vec![0xF0, 0x28, 0x8C, 0xBC]; // Invalid UTF-8 sequence
        invalid_utf8[1] = 0x28; // Make it definitely invalid

        let utf8_err = std::str::from_utf8(&invalid_utf8).unwrap_err();
        let pjs_err = Error::from(utf8_err);

        assert!(matches!(pjs_err, Error::Utf8(_)));
        assert_eq!(pjs_err.category(), "encoding");
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let pjs_err = Error::from(json_err);

        assert!(matches!(pjs_err, Error::Serialization(_)));
        assert!(pjs_err.is_json_error());
        assert_eq!(pjs_err.category(), "json");
    }

    #[test]
    fn test_error_debug_format() {
        let err = Error::invalid_json(10, "syntax error");
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("InvalidJson"));
        assert!(debug_str.contains("10"));
        assert!(debug_str.contains("syntax error"));
    }

    #[test]
    fn test_error_clone() {
        let original = Error::semantic_type_mismatch("expected", "actual");
        let cloned = original.clone();

        assert_eq!(original.category(), cloned.category());
        assert_eq!(original.to_string(), cloned.to_string());
    }

    #[test]
    fn test_result_type_alias() {
        fn returns_result() -> Result<i32> {
            Ok(42)
        }

        fn returns_error() -> Result<i32> {
            Err(Error::other("test error"))
        }

        assert_eq!(returns_result().unwrap(), 42);
        assert!(returns_error().is_err());
    }
}
