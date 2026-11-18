//! Priority heuristic computation shared across all transports.
//!
//! This module owns the single source of truth for converting a JSON path and
//! value into a [`Priority`]. Both the Rust HTTP path (via
//! [`Stream::create_patch_frames`](crate::Stream::create_patch_frames)) and
//! the WebAssembly bindings delegate here so that the same payload yields the
//! same priority regardless of how it is delivered to the client.
//!
//! # Algorithm
//!
//! 1. **Per-call overrides** — case-sensitive lookup of the last path key in
//!    [`PriorityHeuristicConfig::overrides`] short-circuits the heuristic.
//!    This is the hook used by `Stream` for its per-stream `priority_rules`.
//! 2. **Field-name heuristic** — case-insensitive match of the last path key
//!    against the five field lists (`critical`, `high`, `medium`, `low`,
//!    `background`). The first match wins.
//! 3. **Depth + value-shape fallback** — start at [`Priority::MEDIUM`] and
//!    apply a depth boost (shallow paths gain priority, deep paths lose it),
//!    then a value-shape penalty (long strings, big arrays, and large objects
//!    are demoted; short strings get a small bump).
//!
//! # Examples
//!
//! ```
//! use pjson_rs_domain::services::{PriorityHeuristicConfig, ValueShape, compute_priority};
//! use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority};
//!
//! let cfg = PriorityHeuristicConfig::default();
//!
//! // Field-name heuristic: `id` is critical regardless of depth or value.
//! let path = JsonPath::new("$.user.id").unwrap();
//! let value = JsonData::Integer(7);
//! assert_eq!(compute_priority(&cfg, &path, ValueShape::from(&value)), Priority::CRITICAL);
//!
//! // Heuristic fallback: an unknown field at depth 1 with a small string
//! // gets the depth boost (+20) plus the short-string bump (+5).
//! let path = JsonPath::new("$.nickname").unwrap();
//! let value = JsonData::String("ada".to_string());
//! let priority = compute_priority(&cfg, &path, ValueShape::from(&value));
//! assert_eq!(priority.value(), 50 + 20 + 5);
//! ```

use crate::value_objects::{JsonData, JsonPath, PathSegment, Priority};
use std::collections::HashMap;

/// Tunable parameters for the priority heuristic.
///
/// The defaults reproduce the rule set first introduced for the HTTP transport
/// in PR #235. WASM consumers can extend the field lists through the
/// `PriorityConfigBuilder` JS API; per-stream overrides are passed through the
/// `overrides` map.
#[derive(Debug, Clone)]
pub struct PriorityHeuristicConfig {
    /// Field names mapped to [`Priority::CRITICAL`].
    pub critical_fields: Vec<String>,
    /// Field names mapped to [`Priority::HIGH`].
    pub high_fields: Vec<String>,
    /// Field names mapped to [`Priority::MEDIUM`].
    pub medium_fields: Vec<String>,
    /// Field names mapped to [`Priority::LOW`].
    pub low_fields: Vec<String>,
    /// Field names mapped to [`Priority::BACKGROUND`].
    pub background_fields: Vec<String>,
    /// Arrays larger than this lose 40 priority points in the fallback.
    pub large_array_threshold: usize,
    /// Arrays larger than this (but not large) lose 15 priority points.
    pub mid_array_threshold: usize,
    /// Strings longer than this lose 20 priority points in the fallback.
    pub large_string_threshold: usize,
    /// Strings shorter than this gain 5 priority points in the fallback.
    pub small_string_threshold: usize,
    /// Objects with more entries than this lose 10 priority points.
    pub large_object_threshold: usize,
    /// Per-call exact-key overrides (case-sensitive). Wins over heuristics.
    pub overrides: HashMap<String, Priority>,
}

impl Default for PriorityHeuristicConfig {
    fn default() -> Self {
        Self {
            critical_fields: to_strings(&[
                "id", "uuid", "status", "state", "error", "type", "kind",
            ]),
            high_fields: to_strings(&[
                "name",
                "title",
                "label",
                "email",
                "username",
                "description",
                "message",
            ]),
            medium_fields: to_strings(&["content", "body", "value", "data"]),
            low_fields: to_strings(&["created_at", "updated_at", "version", "metadata"]),
            background_fields: to_strings(&[
                "analytics",
                "debug",
                "trace",
                "logs",
                "history",
                "comments",
                "reviews",
            ]),
            large_array_threshold: 100,
            mid_array_threshold: 10,
            large_string_threshold: 1000,
            small_string_threshold: 50,
            large_object_threshold: 10,
            overrides: HashMap::new(),
        }
    }
}

impl PriorityHeuristicConfig {
    /// Create a new config with the default rule set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a field name to the critical tier (case-insensitive at lookup time).
    pub fn add_critical_field(&mut self, field: impl Into<String>) {
        push_unique(&mut self.critical_fields, field.into());
    }

    /// Add a field name to the high tier.
    pub fn add_high_field(&mut self, field: impl Into<String>) {
        push_unique(&mut self.high_fields, field.into());
    }

    /// Add a field name to the medium tier.
    pub fn add_medium_field(&mut self, field: impl Into<String>) {
        push_unique(&mut self.medium_fields, field.into());
    }

    /// Add a field name to the low tier.
    pub fn add_low_field(&mut self, field: impl Into<String>) {
        push_unique(&mut self.low_fields, field.into());
    }

    /// Add a field name to the background tier.
    pub fn add_background_field(&mut self, field: impl Into<String>) {
        push_unique(&mut self.background_fields, field.into());
    }

    /// Insert an exact-key override that bypasses the heuristic entirely.
    pub fn add_override(&mut self, key: impl Into<String>, priority: Priority) {
        self.overrides.insert(key.into(), priority);
    }
}

fn to_strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn push_unique(dest: &mut Vec<String>, value: String) {
    if !dest.iter().any(|v| v == &value) {
        dest.push(value);
    }
}

/// Size-only view of a JSON value, which is all the priority fallback needs.
///
/// Lets callers rank a value without building or cloning a [`JsonData`].
/// String lengths are measured in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueShape {
    /// A string of `len` bytes.
    String {
        /// Length in bytes.
        len: usize,
    },
    /// An array of `len` elements.
    Array {
        /// Element count.
        len: usize,
    },
    /// An object of `len` entries.
    Object {
        /// Entry count.
        len: usize,
    },
    /// Null, boolean, integer or float.
    Scalar,
}

impl From<&JsonData> for ValueShape {
    fn from(value: &JsonData) -> Self {
        match value {
            JsonData::String(s) => Self::String { len: s.len() },
            JsonData::Array(items) => Self::Array { len: items.len() },
            JsonData::Object(entries) => Self::Object { len: entries.len() },
            _ => Self::Scalar,
        }
    }
}

impl From<&serde_json::Value> for ValueShape {
    fn from(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::String(s) => Self::String { len: s.len() },
            serde_json::Value::Array(items) => Self::Array { len: items.len() },
            serde_json::Value::Object(entries) => Self::Object { len: entries.len() },
            _ => Self::Scalar,
        }
    }
}

/// Compute the priority for a single `(path, shape)` pair using the supplied
/// heuristic configuration.
///
/// See the [module-level docs](self) for the algorithm description.
pub fn compute_priority(
    config: &PriorityHeuristicConfig,
    path: &JsonPath,
    shape: ValueShape,
) -> Priority {
    if let Some(PathSegment::Key(key)) = path.last_segment() {
        if let Some(p) = config.overrides.get(key.as_str()) {
            return *p;
        }
        let tiers = [
            (&config.critical_fields, Priority::CRITICAL),
            (&config.high_fields, Priority::HIGH),
            (&config.medium_fields, Priority::MEDIUM),
            (&config.low_fields, Priority::LOW),
            (&config.background_fields, Priority::BACKGROUND),
        ];
        for (list, priority) in tiers {
            if list.iter().any(|f| f.eq_ignore_ascii_case(key)) {
                return priority;
            }
        }
    }

    let mut priority = Priority::MEDIUM;
    match path.depth() {
        0 | 1 => priority = priority.increase_by(20),
        2 => priority = priority.increase_by(10),
        d if d > 5 => priority = priority.decrease_by(10),
        _ => {}
    }

    match shape {
        ValueShape::String { len } if len > config.large_string_threshold => {
            priority = priority.decrease_by(20)
        }
        ValueShape::String { len } if len < config.small_string_threshold => {
            priority = priority.increase_by(5)
        }
        ValueShape::Array { len } if len > config.large_array_threshold => {
            priority = priority.decrease_by(40)
        }
        ValueShape::Array { len } if len > config.mid_array_threshold => {
            priority = priority.decrease_by(15)
        }
        ValueShape::Object { len } if len > config.large_object_threshold => {
            priority = priority.decrease_by(10)
        }
        _ => {}
    }

    priority
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_objects::JsonPath;

    #[test]
    fn critical_field_match_is_case_insensitive() {
        let cfg = PriorityHeuristicConfig::default();
        let path = JsonPath::new("$.User.ID").unwrap();
        let value = JsonData::Integer(1);
        assert_eq!(
            compute_priority(&cfg, &path, ValueShape::from(&value)),
            Priority::CRITICAL
        );
    }

    #[test]
    fn override_wins_over_heuristic() {
        let mut cfg = PriorityHeuristicConfig::default();
        cfg.add_override("id", Priority::LOW);
        let path = JsonPath::new("$.id").unwrap();
        let value = JsonData::Integer(1);
        assert_eq!(
            compute_priority(&cfg, &path, ValueShape::from(&value)),
            Priority::LOW
        );
    }

    #[test]
    fn description_field_is_high() {
        let cfg = PriorityHeuristicConfig::default();
        let path = JsonPath::new("$.description").unwrap();
        let value = JsonData::String("x".to_string());
        assert_eq!(
            compute_priority(&cfg, &path, ValueShape::from(&value)),
            Priority::HIGH
        );
    }

    #[test]
    fn state_and_error_fields_are_critical() {
        let cfg = PriorityHeuristicConfig::default();
        for field in ["state", "error"] {
            let path = JsonPath::new(format!("$.{field}")).unwrap();
            let value = JsonData::String("active".to_string());
            assert_eq!(
                compute_priority(&cfg, &path, ValueShape::from(&value)),
                Priority::CRITICAL,
                "field `{field}` should be CRITICAL",
            );
        }
    }

    #[test]
    fn unknown_field_uses_depth_and_shape() {
        let cfg = PriorityHeuristicConfig::default();
        let path = JsonPath::new("$.unknown").unwrap();
        let small_string = JsonData::String("hi".to_string());
        // MEDIUM (50) + depth-1 boost (20) + short-string bump (5) = 75
        assert_eq!(
            compute_priority(&cfg, &path, ValueShape::from(&small_string)).value(),
            50 + 20 + 5
        );
    }

    #[test]
    fn large_array_demotes_priority() {
        let cfg = PriorityHeuristicConfig::default();
        let path = JsonPath::new("$.unknown").unwrap();
        let big = JsonData::Array((0..200).map(JsonData::Integer).collect());
        // MEDIUM (50) + depth-1 boost (20) - 40 = 30
        assert_eq!(
            compute_priority(&cfg, &path, ValueShape::from(&big)).value(),
            50 + 20 - 40
        );
    }

    #[test]
    fn deep_field_loses_priority() {
        let cfg = PriorityHeuristicConfig::default();
        let path = JsonPath::new("$.a.b.c.d.e.f.g").unwrap();
        let value = JsonData::Bool(true);
        // depth 7 > 5 → -10. MEDIUM (50) - 10 = 40
        assert_eq!(
            compute_priority(&cfg, &path, ValueShape::from(&value)).value(),
            40
        );
    }
}
