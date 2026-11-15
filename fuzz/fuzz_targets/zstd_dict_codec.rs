#![no_main]

use std::sync::{Arc, LazyLock};

use libfuzzer_sys::fuzz_target;
use pjs_fuzz::assert_decode_error;
use pjson_rs::compression::secure::{ByteCodec, SecureCompressedData, SecureCompressor};
use pjson_rs::compression::zstd::{MAX_DICT_SIZE, N_TRAIN, ZstdDictCompressor, ZstdDictionary};

const DECODE_CAP: usize = 1 << 20;

static DICT: LazyLock<Arc<ZstdDictionary>> = LazyLock::new(|| {
    let samples: Vec<Vec<u8>> = (0..N_TRAIN)
        .map(|i| {
            format!(
                r#"{{"id":{i},"name":"item-{i}","value":{},"active":true,"tags":["a{i}","b"]}}"#,
                i * 10
            )
            .into_bytes()
        })
        .collect();
    Arc::new(ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).expect("deterministic training"))
});

fuzz_target!(|data: &[u8]| {
    let Some((&selector, payload)) = data.split_first() else {
        return;
    };
    let dict = &*DICT;
    let codec = ByteCodec::ZstdDict(Arc::clone(dict));
    let compressor = SecureCompressor::with_default_security(codec.clone());

    if selector & 1 == 0 {
        let compressed = SecureCompressedData {
            data: payload.to_vec(),
            original_size: payload.len(),
            compression_ratio: 1.0,
            codec,
        };
        assert_decode_error(compressor.decompress_protected(&compressed));
        assert_decode_error(compressor.decompress_nested(&compressed, usize::from(selector >> 1)));
        assert_decode_error(ZstdDictCompressor::decompress(payload, dict, DECODE_CAP));
    } else {
        let level = i32::from((selector >> 1) % 3) + 1;
        let Ok(packed) = ZstdDictCompressor::compress_with_level(payload, dict, level) else {
            return;
        };
        let compressed = SecureCompressedData {
            compression_ratio: payload.len() as f64 / packed.len().max(1) as f64,
            data: packed,
            original_size: payload.len(),
            codec,
        };
        match compressor.decompress_protected(&compressed) {
            Ok(out) => assert_eq!(out, payload),
            error => assert_decode_error(error),
        }
    }
});
