//! Acceptance tests for #603: per-priority compression totals and wire-size stability.
//!
//! Assumed API: `CompressionStats::per_priority: HashMap<Priority, PriorityTotals>` and
//! `CompressionStats::priority_compression_ratio(Priority) -> Option<f64>`
//! (= sum(output) / sum(input) for that priority; `None` when the priority was never seen).

use pjson_rs::domain::value_objects::Priority;
use pjson_rs::stream::StreamFrame;
use pjson_rs::stream::compression_integration::StreamingCompressor;
use serde_json::json;
use std::collections::HashMap;

fn frame(priority: Priority) -> StreamFrame {
    StreamFrame {
        data: json!({"name":"alice","tags":["alpha","beta","alpha","beta"],"n":[1,2,3,4,5]}),
        priority,
        metadata: HashMap::new(),
    }
}

#[test]
fn test_priority_compression_ratio_unseen_priority_is_none() {
    let compressor = StreamingCompressor::new();
    assert_eq!(
        compressor
            .stats()
            .priority_compression_ratio(Priority::HIGH),
        None
    );
}

#[test]
fn test_priority_compression_ratio_aggregates_over_frames_of_one_priority() {
    let mut compressor = StreamingCompressor::new();
    compressor
        .compress_frame(frame(Priority::HIGH))
        .expect("first");
    compressor
        .compress_frame(frame(Priority::HIGH))
        .expect("second");

    let stats = compressor.stats();
    let expected = stats.total_output_bytes as f64 / stats.total_input_bytes as f64;
    let ratio = stats
        .priority_compression_ratio(Priority::HIGH)
        .expect("priority seen");
    assert!((ratio - expected).abs() < 1e-12, "{ratio} vs {expected}");
}

#[test]
fn test_priority_compression_ratio_priorities_tracked_independently() {
    let mut compressor = StreamingCompressor::new();
    compressor
        .compress_frame(frame(Priority::HIGH))
        .expect("high");
    assert!(
        compressor
            .stats()
            .priority_compression_ratio(Priority::LOW)
            .is_none()
    );
    compressor
        .compress_frame(frame(Priority::LOW))
        .expect("low");
    assert!(
        compressor
            .stats()
            .priority_compression_ratio(Priority::LOW)
            .is_some()
    );
}

#[test]
fn test_compress_frame_wire_size_unchanged_from_baseline() {
    // Golden values measured at HEAD 6b57a49 for this exact input.
    let mut compressor = StreamingCompressor::new();
    for priority in [Priority::HIGH, Priority::HIGH, Priority::LOW] {
        let out = compressor
            .compress_frame(frame(priority))
            .expect("compress");
        assert_eq!(out.compressed_data.compressed_size, 71);
    }
    let stats = compressor.stats();
    assert_eq!(stats.total_input_bytes, 213);
    assert_eq!(stats.total_output_bytes, 213);
}
