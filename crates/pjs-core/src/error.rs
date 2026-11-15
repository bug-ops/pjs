//! Error types for PJS operations.
//!
//! [`Error`] is a closed, typed taxonomy: every variant that wraps a
//! dependency failure keeps it as a [`std::error::Error::source`] so callers
//! can downcast to the underlying type. `Display` never repeats the source's
//! text; use [`std::error::Error::source`] to walk the chain.

use crate::{
    domain::{DomainError, SessionId},
    security::{CompressionBombError, RateLimitError},
};
use std::{error::Error as StdError, fmt, io, num::NonZeroUsize, time::Duration};

/// Result type alias for PJS operations
pub type Result<T> = std::result::Result<T, Error>;

/// Opaque foreign error kept as a source.
pub(crate) type BoxError = Box<dyn StdError + Send + Sync + 'static>;

/// Which configured limit an [`Error::LimitExceeded`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LimitKind {
    /// Raw input size in bytes
    #[error("Input size")]
    InputSize,
    /// JSON nesting depth
    #[error("JSON nesting depth")]
    JsonDepth,
    /// Number of elements in an array
    #[error("Array length")]
    ArrayLength,
    /// Number of keys in an object
    #[error("Object key count")]
    ObjectKeys,
    /// String length
    #[error("String length")]
    StringLength,
    /// WebSocket frame size in bytes
    #[error("WebSocket frame size")]
    WebSocketFrameSize,
    /// Pooled buffer size in bytes
    #[error("Buffer size")]
    BufferSize,
    /// Reconstructed data held for one client session, in bytes
    #[error("Reconstructed session data size")]
    SessionBytes,
}

/// Compression codec an error refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CodecKind {
    /// No compression (bytes stored verbatim)
    #[error("identity")]
    Identity,
    /// Raw deflate (RFC 1951)
    #[error("deflate")]
    Deflate,
    /// Gzip (RFC 1952)
    #[error("gzip")]
    Gzip,
    /// Brotli
    #[error("brotli")]
    Brotli,
    /// Zstandard with a trained dictionary
    #[error("zstd")]
    Zstd,
}

/// Codec operation that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CodecOperation {
    /// Dictionary training
    #[error("train")]
    Train,
    /// Compression
    #[error("compress")]
    Compress,
    /// Decompression
    #[error("decompress")]
    Decompress,
}

/// Why a session id was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SessionIdViolation {
    /// Shorter than the minimum length
    #[error("{len} characters (minimum {min})")]
    TooShort {
        /// Actual length
        len: usize,
        /// Minimum length
        min: usize,
    },
    /// Longer than the maximum length
    #[error("{len} characters (maximum {max})")]
    TooLong {
        /// Actual length
        len: usize,
        /// Maximum length
        max: usize,
    },
    /// Contains a character other than alphanumerics, hyphens and underscores
    #[error("only alphanumeric characters, hyphens and underscores are allowed")]
    InvalidCharacter,
    /// The nil UUID
    #[error("the nil session id is not allowed")]
    Nil,
}

/// Why a trained compression dictionary was rejected.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DictionaryError {
    /// The blob is empty or larger than the permitted maximum
    #[error("dictionary size is out of bounds")]
    Size(#[source] DomainError),
    /// The blob does not start with the zstd dictionary magic
    #[error("missing zstd dictionary magic")]
    BadMagic,
    /// Too few samples to train a dictionary
    #[error("{provided} training samples provided, {required} required")]
    InsufficientSamples {
        /// Samples supplied
        provided: usize,
        /// Samples required
        required: usize,
    },
    /// The compression bomb detector refused the dictionary's size
    #[error("dictionary rejected by the compression bomb detector")]
    Rejected(#[source] CompressionBombError),
}

/// Why a patch could not be applied to a JSON document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PatchError {
    /// An object has no member with this key
    #[error("key `{0}` not found")]
    KeyNotFound(String),
    /// An array index is past the end
    #[error("index {index} is out of bounds for length {len}")]
    IndexOutOfBounds {
        /// Requested index
        index: usize,
        /// Array length
        len: usize,
    },
    /// An array was addressed with a non-numeric key
    #[error("array element key is not an index")]
    NotAnIndex,
    /// The path crosses a value that is not an object
    #[error("expected an object")]
    ExpectedObject,
    /// The path crosses or targets a value that is not an array
    #[error("expected an array")]
    ExpectedArray,
    /// The parent of the target is neither an object nor an array
    #[error("target parent is not an object or array")]
    NotAContainer,
    /// The path is empty where an operation needs a target key
    #[error("path has no target key")]
    NoTargetKey,
    /// The path contains a segment kind the operation cannot follow
    #[error("unsupported path segment")]
    UnsupportedSegment,
}

/// Why stream options were rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StreamOptionsError {
    /// `max_frame_size` below the minimum
    #[error("max_frame_size {requested} is below the minimum {min}")]
    MaxFrameSizeTooSmall {
        /// Requested size
        requested: usize,
        /// Minimum size
        min: usize,
    },
    /// `client_fps` of zero
    #[error("client_fps must not be zero")]
    ZeroClientFps,
}

/// Why a stream plan was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// The connection's live plan budget is exhausted
    #[error("connection plan budget exhausted: {live} live + {requested} requested > {max}")]
    ConnectionBudgetExhausted {
        /// Bytes already reserved on the connection
        live: usize,
        /// Bytes the new plan needs
        requested: usize,
        /// Budget per connection
        max: usize,
    },
}

/// Main error type for PJS operations.
///
/// Wrapped dependency errors are reachable through
/// [`source`](std::error::Error::source) and downcastable. `Display` does not
/// include the source text, so a message printed alone never duplicates the
/// cause; log with a source-chain walker to keep it.
///
/// # Examples
///
/// ```
/// use pjson_rs::Error;
/// use std::error::Error as _;
///
/// let json_error = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
/// let error = Error::from(json_error);
/// assert!(error.source().unwrap().is::<serde_json::Error>());
/// ```
///
/// The enum is not `Clone` and is `#[non_exhaustive]`:
///
/// ```compile_fail
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<pjson_rs::Error>();
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Invalid JSON syntax
    #[error("Invalid JSON syntax at position {position}")]
    InvalidJson {
        /// Position in the input where the error occurred
        position: usize,
        /// Parser error
        #[source]
        source: BoxError,
    },

    /// A configured size, depth or count limit was exceeded
    #[error("{kind} {actual} exceeds maximum allowed {max}")]
    LimitExceeded {
        /// Which limit
        kind: LimitKind,
        /// Observed value
        actual: u64,
        /// Configured maximum
        max: u64,
    },

    /// A session id failed validation
    #[error("Invalid session id")]
    InvalidSessionId(#[source] SessionIdViolation),

    /// A compression bomb guard tripped
    #[error("Compression bomb protection triggered")]
    CompressionBomb(#[from] CompressionBombError),

    /// A compression codec failed on its input
    #[error("{codec} {operation} failed")]
    Codec {
        /// Codec that failed
        codec: CodecKind,
        /// Operation that failed
        operation: CodecOperation,
        /// Codec I/O error
        #[source]
        source: io::Error,
    },

    /// A codec was requested that this build does not include
    #[error("Codec {codec} is unavailable in this build")]
    CodecUnavailable {
        /// Requested codec
        codec: CodecKind,
    },

    /// A trained dictionary was rejected
    #[error("Invalid compression dictionary")]
    InvalidDictionary(#[from] DictionaryError),

    /// A patch could not be applied
    #[error("Patch could not be applied")]
    Patch(#[from] PatchError),

    /// JSON (de)serialization failed
    #[error("Serialization failed")]
    Serialization(#[from] serde_json::Error),

    /// The WebSocket connection could not be established
    #[error("Connection to {url} failed")]
    ConnectionFailed {
        /// Target URL without userinfo, query or fragment
        url: String,
        /// Transport error
        #[source]
        source: BoxError,
    },

    /// A transport error on an established connection
    #[error("Transport error")]
    Transport(#[source] BoxError),

    /// A write did not complete in time
    #[error("Write stalled for {timeout:?}")]
    WriteTimeout {
        /// Configured write timeout
        timeout: Duration,
    },

    /// The URL could not be parsed
    #[error("Invalid URL")]
    InvalidUrl(#[source] BoxError),

    /// The client already has an active connection
    #[error("Client already connected")]
    AlreadyConnected,

    /// The client's outgoing channel is closed
    #[error("Outgoing channel closed")]
    ChannelClosed,

    /// Every client session slot is taken by a completed session
    #[error(
        "Session limit of {limit} reached and every tracked session is complete; \
         close one with `close_session`"
    )]
    SessionLimitReached {
        /// Configured maximum number of tracked sessions
        limit: NonZeroUsize,
    },

    /// The connection has no live session with this id
    #[error("Unknown session {0}")]
    UnknownSession(SessionId),

    /// The connection already has a live session with this id
    #[error("Session {0} already exists")]
    SessionAlreadyExists(SessionId),

    /// The connection already has the maximum number of live sessions
    #[error("Connection already has {limit} live sessions")]
    TooManySessions {
        /// The per-connection cap that was reached
        limit: NonZeroUsize,
    },

    /// A rate limit rejected the request
    #[error("Rate limit violation")]
    RateLimited(#[from] RateLimitError),

    /// The stream options in a request failed validation
    #[error("Invalid stream options")]
    InvalidStreamOptions(#[from] StreamOptionsError),

    /// The stream plan was refused
    #[error("Stream plan rejected")]
    PlanRejected(#[from] PlanError),

    /// The stream options in a request are not supported
    #[error("Unsupported stream options")]
    UnsupportedStreamOptions,
}

impl Error {
    /// Builds [`Error::LimitExceeded`] from `usize` measurements, saturating
    /// at `u64::MAX` on targets where `usize` is wider than `u64`.
    pub(crate) fn limit_exceeded(kind: LimitKind, actual: usize, max: usize) -> Self {
        Self::LimitExceeded {
            kind,
            actual: u64::try_from(actual).unwrap_or(u64::MAX),
            max: u64::try_from(max).unwrap_or(u64::MAX),
        }
    }

    /// Check if the error is related to JSON parsing
    pub fn is_json_error(&self) -> bool {
        matches!(self, Self::InvalidJson { .. } | Self::Serialization(_))
    }

    /// Check if the error is a network-related error
    pub fn is_network_error(&self) -> bool {
        matches!(
            self,
            Self::ConnectionFailed { .. } | Self::Transport(_) | Self::WriteTimeout { .. }
        )
    }

    /// Check if the error is a validation error
    pub fn is_validation_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidSessionId(_) | Self::InvalidStreamOptions(_) | Self::Patch(_)
        )
    }

    /// Check if the error is a security-related error
    pub fn is_security_error(&self) -> bool {
        matches!(
            self,
            Self::LimitExceeded { .. } | Self::CompressionBomb(_) | Self::RateLimited(_)
        )
    }

    /// Get error category as string
    pub fn category(&self) -> &'static str {
        match self {
            Self::InvalidJson { .. } | Self::Serialization(_) => "json",
            Self::InvalidSessionId(_) | Self::InvalidStreamOptions(_) | Self::Patch(_) => {
                "validation"
            }
            Self::LimitExceeded { .. } | Self::CompressionBomb(_) | Self::RateLimited(_) => {
                "security"
            }
            Self::ConnectionFailed { .. } | Self::Transport(_) | Self::WriteTimeout { .. } => {
                "network"
            }
            Self::Codec { .. } | Self::CodecUnavailable { .. } | Self::InvalidDictionary(_) => {
                "compression"
            }
            Self::InvalidUrl(_)
            | Self::AlreadyConnected
            | Self::ChannelClosed
            | Self::SessionLimitReached { .. }
            | Self::UnknownSession(_)
            | Self::SessionAlreadyExists(_)
            | Self::TooManySessions { .. }
            | Self::PlanRejected(_)
            | Self::UnsupportedStreamOptions => "client",
        }
    }
}

/// Renders an error followed by its whole source chain, `": "`-joined, for
/// log lines and for crossing into string-typed error channels without
/// dropping the root cause.
pub(crate) struct ErrorChain<'a>(pub(crate) &'a (dyn StdError + 'static));

impl fmt::Display for ErrorChain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut source = self.0.source();
        while let Some(cause) = source {
            write!(f, ": {cause}")?;
            source = cause.source();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limit_exceeded_display_and_category() {
        let err = Error::limit_exceeded(LimitKind::ArrayLength, 11, 10);
        assert_eq!(
            err.to_string(),
            "Array length 11 exceeds maximum allowed 10"
        );
        assert_eq!(err.category(), "security");
        assert!(err.is_security_error());
    }

    #[test]
    fn test_error_classification() {
        let json = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let json_err = Error::from(json);
        assert!(json_err.is_json_error());
        assert!(!json_err.is_network_error());
        assert!(!json_err.is_validation_error());

        let transport = Error::Transport(Box::new(io::Error::other("boom")));
        assert!(transport.is_network_error());
        assert!(!transport.is_json_error());

        let patch = Error::from(PatchError::ExpectedObject);
        assert!(patch.is_validation_error());
        assert_eq!(patch.category(), "validation");
    }

    #[test]
    fn test_error_categories() {
        let limit = NonZeroUsize::new(4).unwrap();
        assert_eq!(Error::AlreadyConnected.category(), "client");
        assert_eq!(Error::SessionLimitReached { limit }.category(), "client");
        assert_eq!(Error::TooManySessions { limit }.category(), "client");
        assert_eq!(
            Error::CodecUnavailable {
                codec: CodecKind::Gzip
            }
            .category(),
            "compression"
        );
        assert_eq!(
            Error::WriteTimeout {
                timeout: Duration::from_secs(1)
            }
            .category(),
            "network"
        );
    }

    #[test]
    fn test_session_limit_reached_display_mentions_limit() {
        let err = Error::SessionLimitReached {
            limit: NonZeroUsize::new(4).unwrap(),
        };
        assert!(err.to_string().contains('4'));
    }

    #[test]
    fn test_display_omits_source_text() {
        let err = Error::Codec {
            codec: CodecKind::Gzip,
            operation: CodecOperation::Decompress,
            source: io::Error::other("invalid gzip header"),
        };
        assert_eq!(err.to_string(), "gzip decompress failed");
        assert!(err.source().is_some());
    }

    #[test]
    fn test_error_chain_joins_sources() {
        let err = Error::Codec {
            codec: CodecKind::Zstd,
            operation: CodecOperation::Train,
            source: io::Error::other("too uniform"),
        };
        assert_eq!(
            ErrorChain(&err).to_string(),
            "zstd train failed: too uniform"
        );
    }

    #[test]
    fn test_error_chain_without_source_is_display() {
        assert_eq!(
            ErrorChain(&Error::AlreadyConnected).to_string(),
            "Client already connected"
        );
    }

    #[test]
    fn test_from_patch_error_keeps_it_as_source() {
        let err = Error::from(PatchError::KeyNotFound("a".into()));
        let source = err.source().and_then(|s| s.downcast_ref::<PatchError>());
        assert_eq!(source, Some(&PatchError::KeyNotFound("a".into())));
    }

    #[test]
    fn test_result_type_alias() {
        fn returns_result() -> Result<i32> {
            Ok(42)
        }

        fn returns_error() -> Result<i32> {
            Err(Error::ChannelClosed)
        }

        assert_eq!(returns_result().unwrap(), 42);
        assert!(returns_error().is_err());
    }
}
