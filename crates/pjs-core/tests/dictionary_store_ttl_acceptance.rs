//! Acceptance tests for #762 item 3 (track A): a `ttl` too large for `u64`
//! milliseconds means "never expire", it does not wrap into a short ttl.
#![cfg(all(feature = "compression", not(target_arch = "wasm32")))]

use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use pjson_rs::compression::zstd::{MAX_DICT_SIZE, ZstdDictionary};
use pjson_rs::domain::ports::DictionaryStore;
use pjson_rs::infrastructure::repositories::InMemoryDictionaryStore;
use pjson_rs::security::CompressionBombDetector;
use pjson_rs_domain::value_objects::SessionId;

const ZSTD_MAGIC: [u8; 4] = [0x37, 0xA4, 0x30, 0xEC];

// 18_446_744_073_709_552 s in ms is 2^64 + 384: the old `as u64` cast wrapped it to 384 ms.
const WRAPPING_TTL: Duration = Duration::from_secs(18_446_744_073_709_552);

fn store_with_one_session() -> (InMemoryDictionaryStore, SessionId) {
    let store =
        InMemoryDictionaryStore::new(Arc::new(CompressionBombDetector::default()), MAX_DICT_SIZE);
    let mut bytes = ZSTD_MAGIC.to_vec();
    bytes.extend([0u8; 32]);
    let session = SessionId::new();
    store
        .register(
            session,
            ZstdDictionary::from_bytes(bytes).expect("valid dictionary"),
        )
        .expect("registers");
    (store, session)
}

fn is_present(store: &InMemoryDictionaryStore, session: SessionId) -> bool {
    tokio_test::block_on(store.get_dictionary(session))
        .expect("lookup")
        .is_some()
}

#[test]
fn cleanup_expired_with_a_ttl_beyond_u64_millis_evicts_nothing() {
    let (store, session) = store_with_one_session();
    sleep(Duration::from_millis(450));

    store.cleanup_expired(WRAPPING_TTL);
    store.cleanup_expired(Duration::MAX);

    assert!(is_present(&store, session));
}

#[test]
fn cleanup_expired_evicts_a_session_idle_longer_than_a_short_ttl() {
    let (store, session) = store_with_one_session();
    sleep(Duration::from_millis(150));

    store.cleanup_expired(Duration::from_millis(50));

    assert!(!is_present(&store, session));
}

#[test]
fn cleanup_expired_keeps_a_session_within_its_ttl() {
    let (store, session) = store_with_one_session();

    store.cleanup_expired(Duration::from_secs(1800));

    assert!(is_present(&store, session));
}
