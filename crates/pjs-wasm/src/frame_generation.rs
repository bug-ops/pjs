//! Shared frame-generation algorithm for the WASM bindings.
//!
//! `PjsParser::generate_frames_internal` and `PriorityStream::generate_frames_internal`
//! both build a depth-limited skeleton, extract prioritized fields, group and sort them
//! by priority, and emit patch frames per level above a threshold followed by a
//! completion frame. This module is the single implementation both structs wrap.

use crate::priority_assignment::{PriorityAssigner, group_by_priority, sort_priorities};
use pjson_rs_domain::entities::Frame;
use pjson_rs_domain::entities::frame::FramePatch;
use pjson_rs_domain::value_objects::{JsonData, Priority, StreamId};
use std::collections::HashMap;

/// Build a skeleton structure from `data`: same shape, but with null/empty leaf values.
///
/// Recursion stops at `max_depth`, replacing anything deeper with `JsonData::Null`.
pub(crate) fn build_skeleton(data: &JsonData, current_depth: usize, max_depth: usize) -> JsonData {
    if current_depth >= max_depth {
        return JsonData::Null;
    }

    match data {
        JsonData::Object(map) => {
            let mut skeleton_map = HashMap::with_capacity(map.len());

            for (k, v) in map.iter() {
                let skeleton_value = match v {
                    JsonData::Object(_) => build_skeleton(v, current_depth + 1, max_depth),
                    JsonData::Array(_) => JsonData::Array(vec![]),
                    JsonData::String(_) => JsonData::Null,
                    JsonData::Integer(_) => JsonData::Integer(0),
                    JsonData::Float(_) => JsonData::Float(0.0),
                    JsonData::Bool(_) => JsonData::Bool(false),
                    JsonData::Null => JsonData::Null,
                    _ => JsonData::Null,
                };
                skeleton_map.insert(k.clone(), skeleton_value);
            }

            JsonData::Object(skeleton_map)
        }
        JsonData::Array(_) => JsonData::Array(vec![]),
        _ => JsonData::Null,
    }
}

/// Generate priority-ordered frames for `data`: a skeleton frame, one patch frame per
/// priority level at or above `min_priority`, then a completion frame.
pub(crate) fn generate_frames(
    priority_assigner: &PriorityAssigner,
    data: &JsonData,
    stream_id: StreamId,
    min_priority: Priority,
    max_depth: usize,
) -> Result<Vec<Frame>, String> {
    // Pre-allocate frames Vec with estimated capacity
    // Typical: 1 skeleton + ~2-4 priority groups + 1 complete = ~4-6 frames
    // Conservative estimate to avoid over-allocation
    let mut frames = Vec::with_capacity(6);
    let mut sequence = 0u64;

    // 1. Generate skeleton frame (always first, critical priority)
    let skeleton = build_skeleton(data, 0, max_depth);
    frames.push(Frame::skeleton(stream_id, sequence, skeleton));
    sequence += 1;

    // 2. Extract all fields with priorities (depth-limited)
    let prioritized_fields =
        priority_assigner.extract_prioritized_fields_with_limit(data, max_depth);

    // 3. Group fields by priority level
    let grouped = group_by_priority(prioritized_fields);

    // 4. Get sorted priorities (descending order)
    let mut priorities: Vec<Priority> = grouped.keys().copied().collect();
    priorities = sort_priorities(priorities);

    // 5. Generate patch frames for each priority level (above threshold)
    for priority in priorities {
        if priority < min_priority {
            continue; // Skip priorities below threshold
        }

        if let Some(fields) = grouped.get(&priority) {
            // Pre-allocate patches Vec with exact capacity
            let mut patches = Vec::with_capacity(fields.len());
            for field in fields.iter() {
                patches.push(FramePatch::set(field.path.clone(), field.value.clone()));
            }

            if !patches.is_empty() {
                // Create patch frame
                let frame = Frame::patch(stream_id, sequence, priority, patches)
                    .map_err(|e| format!("Failed to create patch frame: {:?}", e))?;

                frames.push(frame);
                sequence += 1;
            }
        }
    }

    // 6. Add completion frame (always last, critical priority)
    frames.push(Frame::complete(stream_id, sequence, None));

    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PjsParser, PriorityStream};
    use std::collections::HashMap;

    /// Regression test for #433: `PjsParser` and `PriorityStream` must emit
    /// identical frames for identical input now that both delegate to this
    /// module instead of maintaining separate copies of the algorithm.
    #[test]
    fn parser_and_stream_emit_identical_frames() {
        let mut obj = HashMap::new();
        obj.insert("id".to_string(), JsonData::Integer(1));
        obj.insert("name".to_string(), JsonData::String("Alice".to_string()));
        obj.insert("bio".to_string(), JsonData::String("Developer".to_string()));
        obj.insert("logs".to_string(), JsonData::Array(vec![]));
        let data = JsonData::Object(obj);

        let stream_id = StreamId::new();
        let min_priority = Priority::LOW;

        let parser_frames = PjsParser::new()
            .generate_frames_internal(&data, stream_id, min_priority)
            .expect("PjsParser frame generation failed");
        let stream_frames = PriorityStream::new()
            .generate_frames_internal(&data, stream_id, min_priority)
            .expect("PriorityStream frame generation failed");

        assert_eq!(
            parser_frames.len(),
            stream_frames.len(),
            "PjsParser and PriorityStream must emit the same number of frames for identical input"
        );
        for (parser_frame, stream_frame) in parser_frames.iter().zip(stream_frames.iter()) {
            assert_eq!(parser_frame.frame_type(), stream_frame.frame_type());
            assert_eq!(parser_frame.sequence(), stream_frame.sequence());
            assert_eq!(parser_frame.priority(), stream_frame.priority());
            assert_eq!(parser_frame.payload(), stream_frame.payload());
        }
    }
}
