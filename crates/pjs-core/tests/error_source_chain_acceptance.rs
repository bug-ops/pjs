//! Acceptance tests for #599: `pjson_rs::Error` keeps typed, downcastable
//! source chains, is not `Clone`, and never repeats the source text in its own
//! `Display`.
//!
//! `#[non_exhaustive]` cannot be asserted from an integration test (a
//! wildcard arm compiles either way); it is checked by a `compile_fail`
//! doctest on `Error` and by review.
#![cfg(all(
    feature = "websocket-server",
    feature = "websocket-client",
    feature = "compression",
    feature = "partial-parse"
))]

use std::error::Error as StdError;
use std::io;

use pjson_rs::Error;
use pjson_rs::compression::secure::{ByteCodec, SecureCompressedData, SecureCompressor};
use pjson_rs::domain::SessionId;
use pjson_rs::error::{CodecKind, CodecOperation, SessionIdViolation};
use pjson_rs::infrastructure::websocket::{CreateSessionError, PjsWebSocketClient};
use pjson_rs::parser::partial::{JiterConfig, JiterPartialParser, PartialJsonParser};
use pjson_rs::security::{CompressionBombError, SecurityValidator};
use tokio::net::TcpListener;

fn assert_send_sync_static<T: Send + Sync + 'static>() {}

trait AmbiguousIfClone<A> {
    fn some_item() {}
}
impl<T: ?Sized> AmbiguousIfClone<()> for T {}
impl<T: Clone> AmbiguousIfClone<u8> for T {}

/// Compiles only while `Error` does not implement `Clone`: with `Clone` the
/// call is ambiguous.
const _: fn() = || {
    let _ = <Error as AmbiguousIfClone<_>>::some_item;
};

fn source_of(error: &Error) -> &(dyn StdError + 'static) {
    error.source().expect("error must expose its source")
}

fn assert_display_omits_source(error: &Error) {
    let source_text = source_of(error).to_string();
    assert!(
        !error.to_string().contains(&source_text),
        "Display `{error}` repeats the source text `{source_text}`"
    );
}

#[test]
fn test_error_is_send_sync_and_static() {
    assert_send_sync_static::<Error>();
}

#[test]
fn test_new_client_invalid_url_exposes_url_parse_error_source() {
    let Err(error) = PjsWebSocketClient::new("not a url") else {
        panic!("an unparsable URL must be rejected");
    };

    assert!(matches!(error, Error::InvalidUrl(_)), "{error:?}");
    assert!(
        source_of(&error)
            .downcast_ref::<url::ParseError>()
            .is_some()
    );
    assert_display_omits_source(&error);
    assert_eq!(error.category(), "client");
}

#[tokio::test]
async fn test_connect_refused_exposes_tungstenite_source_and_redacted_url() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let client = PjsWebSocketClient::new(format!(
        "ws://user:secret@127.0.0.1:{port}/ws?token=abc#frag"
    ))
    .expect("valid url");

    let error = client
        .connect()
        .await
        .expect_err("nothing listens on the port");

    let Error::ConnectionFailed { ref url, .. } = error else {
        panic!("expected ConnectionFailed");
    };
    assert!(url.contains("127.0.0.1"), "redacted url lost the host");
    for (label, leaked) in [
        ("userinfo name", url.contains("user")),
        ("password", url.contains("secret")),
        ("query key", url.contains("token")),
        ("query value", url.contains("abc")),
        ("fragment", url.contains("frag")),
    ] {
        assert!(!leaked, "the redacted url still carries the {label}");
    }
    assert!(
        source_of(&error)
            .downcast_ref::<tokio_tungstenite::tungstenite::Error>()
            .is_some()
    );
    assert_display_omits_source(&error);
    assert_eq!(error.category(), "network");
    assert!(error.is_network_error());
}

#[test]
fn test_from_serde_json_error_exposes_it_as_source() {
    let parse_error = serde_json::from_str::<serde_json::Value>("{").expect_err("truncated JSON");

    let error = Error::from(parse_error);

    assert!(matches!(error, Error::Serialization(_)), "{error:?}");
    assert!(
        source_of(&error)
            .downcast_ref::<serde_json::Error>()
            .is_some()
    );
    assert_display_omits_source(&error);
    assert_eq!(error.category(), "json");
}

#[test]
fn test_partial_parser_invalid_json_exposes_jiter_source() {
    let input = b"[1, x]";
    let parser = JiterPartialParser::new(JiterConfig::default());

    let error = parser.parse_partial(input).expect_err("`x` is not JSON");

    let Error::InvalidJson { position, .. } = error else {
        panic!("expected InvalidJson, got {error:?}");
    };
    assert!(position <= input.len());
    assert!(
        source_of(&error)
            .downcast_ref::<jiter::JsonError>()
            .is_some()
    );
    assert_display_omits_source(&error);
    assert_eq!(error.category(), "json");
}

#[test]
fn test_decompress_protected_corrupt_gzip_is_codec_error_with_io_source() {
    let compressor = SecureCompressor::with_default_security(ByteCodec::Gzip);
    let corrupt = SecureCompressedData {
        data: vec![
            0x1f, 0x8b, 0x08, 0x00, 0xde, 0xad, 0xbe, 0xef, 0x00, 0x01, 0x02, 0x03,
        ],
        original_size: 64,
        compression_ratio: 1.0,
        codec: ByteCodec::Gzip,
    };

    let error = compressor
        .decompress_protected(&corrupt)
        .expect_err("corrupt gzip must fail");

    let Error::Codec {
        ref codec,
        ref operation,
        ..
    } = error
    else {
        panic!("expected Codec, got {error:?}");
    };
    assert_eq!(*codec, CodecKind::Gzip);
    assert_eq!(*operation, CodecOperation::Decompress);
    assert!(source_of(&error).downcast_ref::<io::Error>().is_some());
    assert!(
        !error.is_security_error(),
        "corrupt input is not a security event"
    );
    assert_display_omits_source(&error);
    assert_eq!(error.category(), "compression");
}

#[test]
fn test_decompress_protected_ratio_bomb_is_compression_bomb_error() {
    let compressor = SecureCompressor::with_default_security(ByteCodec::Gzip);
    let bomb = compressor
        .compress(&vec![0u8; 8 * 1024 * 1024])
        .expect("zeros compress");

    let error = compressor
        .decompress_protected(&bomb)
        .expect_err("a ratio far above 300x must be refused");

    assert!(
        matches!(
            error,
            Error::CompressionBomb(CompressionBombError::RatioExceeded { .. })
        ),
        "{error:?}"
    );
    assert!(error.is_security_error());
    assert_eq!(error.category(), "security");
}

#[test]
fn test_validate_object_keys_over_limit_is_limit_exceeded() {
    let validator = SecurityValidator::default();

    let error = validator
        .validate_object_keys(10_001)
        .expect_err("10 001 keys exceed the default 10 000");

    let Error::LimitExceeded { actual, max, .. } = error else {
        panic!("expected LimitExceeded, got {error:?}");
    };
    assert_eq!(usize::try_from(actual).unwrap(), 10_001);
    assert_eq!(usize::try_from(max).unwrap(), 10_000);
    assert_eq!(error.category(), "security");
}

#[test]
fn test_validate_session_id_violations_are_typed() {
    let validator = SecurityValidator::default();

    let short = validator.validate_session_id("short").expect_err("5 < 8");
    let long = validator
        .validate_session_id(&"a".repeat(129))
        .expect_err("129 > 128");
    let bad_char = validator
        .validate_session_id("has space!")
        .expect_err("space is not allowed");

    assert!(matches!(
        short,
        Error::InvalidSessionId(SessionIdViolation::TooShort { len: 5, min: 8 })
    ));
    assert!(matches!(
        long,
        Error::InvalidSessionId(SessionIdViolation::TooLong { len: 129, max: 128 })
    ));
    assert!(matches!(
        bad_char,
        Error::InvalidSessionId(SessionIdViolation::InvalidCharacter)
    ));
    assert_eq!(short.category(), "validation");
}

#[test]
fn test_from_create_session_error_maps_to_typed_variants() {
    let id = SessionId::new();
    let limit = std::num::NonZeroUsize::new(64).unwrap();

    let exists = Error::from(CreateSessionError::AlreadyExists(id));
    let too_many = Error::from(CreateSessionError::TooManySessions { limit });

    assert!(matches!(exists, Error::SessionAlreadyExists(got) if got == id));
    assert!(matches!(too_many, Error::TooManySessions { limit: got } if got == limit));
}
