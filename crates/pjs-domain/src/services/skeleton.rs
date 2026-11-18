//! Skeleton transform shared by `Stream` and the WebAssembly bindings.
//!
//! A skeleton keeps the object shape of a document and replaces every leaf with
//! a placeholder, so a client can lay out the result before the values arrive.

use crate::value_objects::JsonData;
use std::collections::HashMap;

/// Build the skeleton of `data`, descending at most `max_depth` object levels.
///
/// The root is at depth 0. Placeholders for object members: string and null
/// become `null`, integer `0`, float `0.0`, bool `false`, array `[]`. An object
/// at depth `max_depth` or deeper becomes `null`, which bounds the recursion for
/// any input. A root scalar becomes `null` and a root array `[]`.
///
/// # Examples
///
/// ```
/// use pjson_rs_domain::services::skeleton;
/// use pjson_rs_domain::value_objects::JsonData;
///
/// let data = JsonData::from(serde_json::json!({"id": 7, "ratio": 1.5, "name": "ada"}));
/// let expected = JsonData::from(serde_json::json!({"id": 0, "ratio": 0.0, "name": null}));
///
/// assert_eq!(skeleton(&data, 64), expected);
/// ```
pub fn skeleton(data: &JsonData, max_depth: usize) -> JsonData {
    skeleton_at(data, 0, max_depth)
}

fn skeleton_at(data: &JsonData, depth: usize, max_depth: usize) -> JsonData {
    if depth >= max_depth {
        return JsonData::Null;
    }
    match data {
        JsonData::Object(map) => JsonData::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), member_placeholder(value, depth, max_depth)))
                .collect::<HashMap<_, _>>(),
        ),
        JsonData::Array(_) => JsonData::Array(Vec::new()),
        _ => JsonData::Null,
    }
}

fn member_placeholder(value: &JsonData, depth: usize, max_depth: usize) -> JsonData {
    match value {
        JsonData::Object(_) => skeleton_at(value, depth + 1, max_depth),
        JsonData::Array(_) => JsonData::Array(Vec::new()),
        JsonData::Integer(_) => JsonData::Integer(0),
        JsonData::Float(_) => JsonData::Float(0.0),
        JsonData::Bool(_) => JsonData::Bool(false),
        JsonData::String(_) | JsonData::Null => JsonData::Null,
    }
}
