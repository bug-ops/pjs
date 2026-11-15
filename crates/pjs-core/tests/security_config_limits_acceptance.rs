//! Acceptance tests for #613 (parser enforces JSON limits), #614 (stricter
//! `SecurityConfig::validate`) and #591.1 (`alloc_aligned` rejects size 0).

use pjson_rs::config::ConfigError;
use pjson_rs::parser::aligned_allocator;
use pjson_rs::{DomainError, LazyParser, SecurityConfig, Validate, ZeroCopyParser};

type Mutator = fn(&mut SecurityConfig);

fn parser_with(configure: Mutator) -> ZeroCopyParser<'static> {
    let mut config = SecurityConfig::default();
    configure(&mut config);
    ZeroCopyParser::with_security_config(config.validated().expect("config stays valid"))
}

fn tight_array(c: &mut SecurityConfig) {
    c.json.max_array_length = 2;
}
fn tight_keys(c: &mut SecurityConfig) {
    c.json.max_object_keys = 1;
}
fn tight_string(c: &mut SecurityConfig) {
    c.json.max_string_length = 3;
}

fn assert_security_violation<T: std::fmt::Debug>(result: Result<T, DomainError>) {
    assert!(
        matches!(result, Err(DomainError::SecurityViolation(_))),
        "expected SecurityViolation, got {result:?}"
    );
}

#[test]
fn test_parse_lazy_array_over_max_array_length_rejected() {
    assert_security_violation(parser_with(tight_array).parse_lazy(b"[1,2,3,4,5]"));
}

#[test]
fn test_parse_lazy_array_at_max_array_length_accepted() {
    assert!(parser_with(tight_array).parse_lazy(b"[1,2]").is_ok());
}

#[test]
fn test_parse_lazy_nested_array_over_limit_rejected() {
    assert_security_violation(parser_with(tight_array).parse_lazy(b"[[1,2,3],1]"));
}

#[test]
fn test_parse_lazy_object_over_max_object_keys_rejected() {
    assert_security_violation(parser_with(tight_keys).parse_lazy(br#"{"a":1,"b":2,"c":3}"#));
}

#[test]
fn test_parse_lazy_object_at_max_object_keys_accepted() {
    assert!(parser_with(tight_keys).parse_lazy(br#"{"a":1}"#).is_ok());
}

#[test]
fn test_parse_lazy_string_over_max_string_length_rejected() {
    assert_security_violation(parser_with(tight_string).parse_lazy(br#""abcdefgh""#));
}

#[test]
fn test_parse_lazy_string_at_max_string_length_accepted() {
    assert!(parser_with(tight_string).parse_lazy(br#""abc""#).is_ok());
}

#[test]
fn test_parse_lazy_string_limit_counts_raw_bytes_before_unescape() {
    // 4 raw content bytes (`\n\n`) unescape to 2 chars; the limit applies to the raw form.
    assert_security_violation(parser_with(tight_string).parse_lazy(br#""\n\n""#));
}

#[test]
fn test_parse_lazy_default_limits_reject_oversized_array() {
    let mut input = Vec::with_capacity(2_200_005);
    input.push(b'[');
    for _ in 0..1_000_001 {
        input.extend_from_slice(b"1,");
    }
    input.extend_from_slice(b"1]");
    assert_security_violation(ZeroCopyParser::new().parse_lazy(&input));
}

#[test]
fn test_parse_lazy_default_limits_reject_oversized_object() {
    let mut input = String::from("{");
    for i in 0..10_001 {
        if i > 0 {
            input.push(',');
        }
        input.push_str(&format!("\"k{i}\":1"));
    }
    input.push('}');
    assert_security_violation(ZeroCopyParser::new().parse_lazy(input.as_bytes()));
}

fn zero_max_object_keys(c: &mut SecurityConfig) {
    c.json.max_object_keys = 0;
}
fn zero_max_array_length(c: &mut SecurityConfig) {
    c.json.max_array_length = 0;
}
fn zero_max_pool_size(c: &mut SecurityConfig) {
    c.buffers.max_pool_size = 0;
}
fn zero_buffer_ttl(c: &mut SecurityConfig) {
    c.buffers.buffer_ttl_secs = 0;
}
fn zero_buffers_per_bucket(c: &mut SecurityConfig) {
    c.buffers.max_buffers_per_bucket = 0;
}
fn zero_concurrent_connections(c: &mut SecurityConfig) {
    c.network.max_concurrent_connections = 0;
}
fn zero_connection_timeout(c: &mut SecurityConfig) {
    c.network.connection_timeout_secs = 0;
}
fn zero_requests_per_second(c: &mut SecurityConfig) {
    c.network.max_requests_per_second = 0;
}
fn zero_window_duration(c: &mut SecurityConfig) {
    c.network.rate_limiting.window_duration_secs = 0;
}
fn zero_connections_per_ip(c: &mut SecurityConfig) {
    c.network.rate_limiting.max_connections_per_ip = 0;
}
fn zero_streams_per_session(c: &mut SecurityConfig) {
    c.sessions.max_streams_per_session = 0;
}
fn zero_session_timeout(c: &mut SecurityConfig) {
    c.sessions.session_timeout_secs = 0;
}

#[test]
fn test_validate_zero_field_rejected_as_must_be_positive() {
    let cases: [(&str, Mutator); 12] = [
        ("json.max_object_keys", zero_max_object_keys),
        ("json.max_array_length", zero_max_array_length),
        ("buffers.max_pool_size", zero_max_pool_size),
        ("buffers.buffer_ttl_secs", zero_buffer_ttl),
        ("buffers.max_buffers_per_bucket", zero_buffers_per_bucket),
        (
            "network.max_concurrent_connections",
            zero_concurrent_connections,
        ),
        ("network.connection_timeout_secs", zero_connection_timeout),
        ("network.max_requests_per_second", zero_requests_per_second),
        ("rate_limiting.window_duration_secs", zero_window_duration),
        (
            "rate_limiting.max_connections_per_ip",
            zero_connections_per_ip,
        ),
        ("sessions.max_streams_per_session", zero_streams_per_session),
        ("sessions.session_timeout_secs", zero_session_timeout),
    ];
    for (name, mutate) in cases {
        let mut config = SecurityConfig::default();
        mutate(&mut config);
        assert!(
            matches!(config.validate(), Err(ConfigError::MustBePositive { .. })),
            "{name} = 0 must be rejected as MustBePositive"
        );
    }
}

#[test]
fn test_validate_buffer_larger_than_total_memory_rejected() {
    let mut config = SecurityConfig::default();
    config.buffers.max_buffer_size = config.buffers.max_total_memory * 2;
    assert!(matches!(
        config.validate(),
        Err(ConfigError::InconsistentBounds { .. })
    ));
}

#[test]
fn test_validate_websocket_frame_larger_than_json_input_rejected() {
    let mut config = SecurityConfig::default();
    config.network.max_websocket_frame_size = config.json.max_input_size * 2;
    assert!(matches!(
        config.validate(),
        Err(ConfigError::InconsistentBounds { .. })
    ));
}

#[test]
fn test_validate_session_data_larger_than_total_memory_rejected() {
    let mut config = SecurityConfig::default();
    config.sessions.max_session_data_size = usize::MAX;
    assert!(matches!(
        config.validate(),
        Err(ConfigError::InconsistentBounds { .. })
    ));
}

#[test]
fn test_validate_default_and_presets_accepted() {
    for (name, config) in [
        ("default", SecurityConfig::default()),
        ("high_throughput", SecurityConfig::high_throughput()),
        ("low_memory", SecurityConfig::low_memory()),
        ("development", SecurityConfig::development()),
    ] {
        assert!(config.validate().is_ok(), "{name} preset must stay valid");
    }
}

#[test]
fn test_alloc_aligned_zero_size_rejected_with_invalid_input() {
    // SAFETY: a zero-size request must be rejected before reaching the global allocator.
    let result = unsafe { aligned_allocator().alloc_aligned(0, 16) };
    assert!(matches!(result, Err(DomainError::InvalidInput(_))));
}
