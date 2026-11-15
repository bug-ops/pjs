//! Acceptance tests for #605: conversions between `TrainedDictionary` and `ZstdDictionary`.
#![cfg(all(feature = "compression", not(target_arch = "wasm32")))]

use pjson_rs::compression::zstd::{MAX_DICT_SIZE, ZstdDictionary};
use pjson_rs::domain::value_objects::{MAX_TRAINED_DICTIONARY_BYTES, TrainedDictionary};

const ZSTD_MAGIC: [u8; 4] = [0x37, 0xA4, 0x30, 0xEC];

fn magic_blob(extra: usize) -> Vec<u8> {
    let mut bytes = ZSTD_MAGIC.to_vec();
    bytes.extend(std::iter::repeat_n(0u8, extra));
    bytes
}

#[test]
fn test_max_dict_size_equals_domain_limit() {
    assert_eq!(MAX_DICT_SIZE, MAX_TRAINED_DICTIONARY_BYTES);
}

#[test]
fn test_zstd_dictionary_try_from_trained_rejects_bad_magic() {
    let trained = TrainedDictionary::from_bytes(vec![0, 1, 2, 3, 4]).expect("valid trained");
    assert!(ZstdDictionary::try_from(&trained).is_err());
}

#[test]
fn test_zstd_dictionary_try_from_trained_accepts_magic_blob() {
    let trained = TrainedDictionary::from_bytes(magic_blob(16)).expect("valid trained");
    let zstd = ZstdDictionary::try_from(&trained).expect("magic present");
    assert_eq!(zstd.as_bytes(), trained.as_bytes());
}

#[test]
fn test_trained_from_zstd_dictionary_preserves_bytes() {
    let zstd = ZstdDictionary::from_bytes(magic_blob(32)).expect("valid zstd dictionary");
    let expected = zstd.as_bytes().to_vec();
    let trained = TrainedDictionary::from(zstd);
    assert_eq!(trained.as_bytes(), expected.as_slice());
}
