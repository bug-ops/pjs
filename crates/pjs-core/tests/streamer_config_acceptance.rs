//! Acceptance tests for #760 (track A): `StreamerConfig` keeps only
//! `max_patch_size`, a `NonZeroUsize`, so `chunks(0)` cannot be constructed.

use std::num::NonZeroUsize;

use pjson_rs::stream::priority::{PriorityStreamFrame, PriorityStreamer, StreamerConfig};
use serde_json::{Value, json};

fn batch_sizes(streamer: &PriorityStreamer, document: &Value) -> Vec<usize> {
    streamer
        .analyze(document)
        .expect("plan")
        .frames()
        .filter_map(|frame| match frame {
            PriorityStreamFrame::Patch { patches, .. } => Some(patches.len()),
            _ => None,
        })
        .collect()
}

/// All five fields share one priority, so a batch is split only by the size limit.
fn flat_document() -> Value {
    json!({"a": 1, "b": 2, "c": 3, "d": 4, "e": 5})
}

#[test]
fn streamer_config_is_built_from_max_patch_size_alone() {
    // An exhaustive literal: compiles only while `max_patch_size` is the single field.
    let config = StreamerConfig {
        max_patch_size: NonZeroUsize::new(2).expect("non-zero"),
    };

    assert_eq!(config.max_patch_size.get(), 2);
}

#[test]
fn streamer_config_default_keeps_the_batch_limit_of_100() {
    assert_eq!(StreamerConfig::default().max_patch_size.get(), 100);
}

#[test]
fn with_config_splits_a_priority_group_at_max_patch_size() {
    let streamer = PriorityStreamer::with_config(StreamerConfig {
        max_patch_size: NonZeroUsize::new(2).expect("non-zero"),
    });

    assert_eq!(batch_sizes(&streamer, &flat_document()), vec![2, 2, 1]);
}

#[test]
fn with_config_of_one_emits_a_frame_per_patch() {
    let streamer = PriorityStreamer::with_config(StreamerConfig {
        max_patch_size: NonZeroUsize::MIN,
    });

    assert_eq!(batch_sizes(&streamer, &flat_document()), vec![1; 5]);
}
