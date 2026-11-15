//! Acceptance tests for #605: opaque `TrainedDictionary` value object.
//!
//! Assumed API: `pjson_rs_domain::value_objects::{TrainedDictionary,
//! MAX_TRAINED_DICTIONARY_BYTES}` with `from_bytes(Vec<u8>) -> Result<Self, _>`,
//! `as_bytes()`, `len()`, `is_empty()`.

use pjson_rs_domain::value_objects::{MAX_TRAINED_DICTIONARY_BYTES, TrainedDictionary};

#[test]
fn test_max_trained_dictionary_bytes_is_112_kib() {
    assert_eq!(MAX_TRAINED_DICTIONARY_BYTES, 112 * 1024);
}

#[test]
fn test_from_bytes_empty_rejected() {
    assert!(TrainedDictionary::from_bytes(Vec::new()).is_err());
}

#[test]
fn test_from_bytes_over_limit_rejected() {
    assert!(TrainedDictionary::from_bytes(vec![1u8; MAX_TRAINED_DICTIONARY_BYTES + 1]).is_err());
}

#[test]
fn test_from_bytes_single_byte_accepted() {
    let dictionary = TrainedDictionary::from_bytes(vec![7u8]).expect("1 byte is valid");
    assert_eq!(dictionary.len(), 1);
    assert!(!dictionary.is_empty());
}

#[test]
fn test_from_bytes_at_limit_accepted_and_bytes_preserved() {
    let bytes = vec![9u8; MAX_TRAINED_DICTIONARY_BYTES];
    let dictionary = TrainedDictionary::from_bytes(bytes.clone()).expect("limit is valid");
    assert_eq!(dictionary.as_bytes(), bytes.as_slice());
}

#[test]
fn test_from_bytes_does_not_require_zstd_magic() {
    assert!(TrainedDictionary::from_bytes(vec![0, 1, 2, 3, 4]).is_ok());
}
