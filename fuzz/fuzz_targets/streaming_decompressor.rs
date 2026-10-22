#![no_main]

use std::collections::HashMap;

use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use pjson_rs::compression::{CompressedData, CompressionStrategy};
use pjson_rs::domain::value_objects::Priority;
use pjson_rs::stream::{
    CompressedFrame, DecompressionMetadata, StreamFrame, StreamingDecompressor,
};

fn strategy(u: &mut Unstructured<'_>) -> Result<CompressionStrategy> {
    Ok(match u.int_in_range(0..=4)? {
        0 => CompressionStrategy::None,
        1 => CompressionStrategy::Dictionary {
            dictionary: HashMap::new(),
        },
        2 => CompressionStrategy::Delta,
        3 => CompressionStrategy::RunLength,
        _ => CompressionStrategy::Hybrid {
            string_dict: HashMap::new(),
        },
    })
}

fn frame(mut u: Unstructured<'_>) -> Result<CompressedFrame> {
    let strategy = strategy(&mut u)?;
    let dictionary_map: HashMap<u16, String> = u.arbitrary()?;
    let delta_bases: HashMap<String, f64> = u.arbitrary()?;
    let json = u.take_rest();
    let data = serde_json::from_slice(json)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(json).into_owned()));
    Ok(CompressedFrame {
        frame: StreamFrame {
            data: serde_json::Value::Null,
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: strategy.clone(),
            compressed_size: 0,
            data,
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy,
            dictionary_map,
            delta_bases,
        },
    })
}

fuzz_target!(|data: &[u8]| {
    if let Ok(frame) = frame(Unstructured::new(data)) {
        let _ = StreamingDecompressor::new().decompress_frame(frame);
    }
});
