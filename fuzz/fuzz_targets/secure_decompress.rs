#![no_main]

use libfuzzer_sys::fuzz_target;
use pjson_rs::compression::secure::{ByteCodec, SecureCompressedData, SecureCompressor};

// TODO(review): ByteCodec::ZstdDict is not covered; it needs a trained dictionary.
fuzz_target!(|data: &[u8]| {
    let Some((&selector, payload)) = data.split_first() else {
        return;
    };
    let codec = match selector % 4 {
        0 => ByteCodec::None,
        1 => ByteCodec::Deflate,
        2 => ByteCodec::Gzip,
        _ => ByteCodec::Brotli,
    };
    let compressed = SecureCompressedData {
        data: payload.to_vec(),
        original_size: payload.len(),
        compression_ratio: 1.0,
        codec: codec.clone(),
    };
    let compressor = SecureCompressor::with_default_security(codec);
    let _ = compressor.decompress_protected(&compressed);
    let _ = compressor.decompress_nested(&compressed, usize::from(selector >> 2));
});
