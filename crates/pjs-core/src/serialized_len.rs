//! Shared serialized-length counter (stub, #670).

use serde_json::Value;

/// Serialized byte length of `value`, without allocating.
pub(crate) fn json_len(_value: &Value) -> usize {
    todo!("#670: counting writer")
}

/// Serialized byte length of `s` as a JSON string, without allocating.
pub(crate) fn json_str_len(_s: &str) -> usize {
    todo!("#670: counting writer")
}

#[cfg(test)]
mod acceptance;
