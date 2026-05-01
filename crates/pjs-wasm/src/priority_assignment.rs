//! Priority assignment for the WebAssembly bindings.
//!
//! This module is a thin adapter on top of
//! [`pjson_rs_domain::services::compute_priority`]. It exists so the
//! WebAssembly transport produces the same priorities as the HTTP transport
//! for the same payload — see issue #242 for the divergence this resolves.
//!
//! [`PriorityConfig`] is the JS-facing customisation surface (populated
//! through [`crate::priority_config::PriorityConfigBuilder`]); the actual
//! algorithm now lives entirely in the domain crate.

use pjson_rs_domain::services::{PriorityHeuristicConfig, compute_priority};
use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority};
use std::collections::HashMap;

/// Configuration surface for the WASM priority assigner.
///
/// Field tier lists are forwarded to [`PriorityHeuristicConfig`] verbatim;
/// matching is case-insensitive on the last path segment, just like the
/// HTTP transport.
#[derive(Debug, Clone)]
pub struct PriorityConfig {
    /// Field names mapped to [`Priority::CRITICAL`].
    pub critical_fields: Vec<String>,
    /// Field names mapped to [`Priority::HIGH`].
    pub high_fields: Vec<String>,
    /// Field names mapped to [`Priority::LOW`]. (JS API: `addLowPattern`).
    pub low_patterns: Vec<String>,
    /// Field names mapped to [`Priority::BACKGROUND`]. (JS API:
    /// `addBackgroundPattern`).
    pub background_patterns: Vec<String>,
    /// Arrays larger than this lose 40 priority points in the heuristic
    /// fallback.
    pub large_array_threshold: usize,
    /// Strings longer than this lose 20 priority points in the heuristic
    /// fallback.
    pub large_string_threshold: usize,
}

impl Default for PriorityConfig {
    fn default() -> Self {
        let domain = PriorityHeuristicConfig::default();
        Self {
            critical_fields: domain.critical_fields.clone(),
            high_fields: domain.high_fields.clone(),
            low_patterns: domain.low_fields.clone(),
            background_patterns: domain.background_fields.clone(),
            large_array_threshold: domain.large_array_threshold,
            large_string_threshold: domain.large_string_threshold,
        }
    }
}

impl PriorityConfig {
    /// Create configuration with the default rule set.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a critical-tier field name.
    pub fn add_critical_field(&mut self, field: String) {
        push_unique(&mut self.critical_fields, field);
    }

    /// Add a high-tier field name.
    pub fn add_high_field(&mut self, field: String) {
        push_unique(&mut self.high_fields, field);
    }

    /// Add a low-tier field name (kept for JS API compatibility — the match is
    /// exact and case-insensitive, not substring).
    pub fn add_low_pattern(&mut self, pattern: String) {
        push_unique(&mut self.low_patterns, pattern);
    }

    /// Add a background-tier field name.
    pub fn add_background_pattern(&mut self, pattern: String) {
        push_unique(&mut self.background_patterns, pattern);
    }

    /// Project this config onto the domain heuristic config so the parity
    /// between transports is preserved by construction.
    pub fn to_heuristic(&self) -> PriorityHeuristicConfig {
        PriorityHeuristicConfig {
            critical_fields: self.critical_fields.clone(),
            high_fields: self.high_fields.clone(),
            low_fields: self.low_patterns.clone(),
            background_fields: self.background_patterns.clone(),
            large_array_threshold: self.large_array_threshold,
            large_string_threshold: self.large_string_threshold,
            ..PriorityHeuristicConfig::default()
        }
    }
}

fn push_unique(dest: &mut Vec<String>, value: String) {
    if !dest.iter().any(|v| v == &value) {
        dest.push(value);
    }
}

/// Priority assignment engine for JSON data.
///
/// Internally it caches a [`PriorityHeuristicConfig`] derived from
/// [`PriorityConfig`]; mutations through `config_mut` rebuild the cache lazily
/// on the next priority call.
#[derive(Debug)]
pub struct PriorityAssigner {
    config: PriorityConfig,
    heuristic: PriorityHeuristicConfig,
}

impl PriorityAssigner {
    /// Create a priority assigner with the default configuration.
    pub fn new() -> Self {
        let config = PriorityConfig::default();
        let heuristic = config.to_heuristic();
        Self { config, heuristic }
    }

    /// Create a priority assigner with a custom configuration.
    pub fn with_config(config: PriorityConfig) -> Self {
        let heuristic = config.to_heuristic();
        Self { config, heuristic }
    }

    /// Borrow the underlying configuration.
    #[allow(dead_code)]
    pub fn config(&self) -> &PriorityConfig {
        &self.config
    }

    /// Mutably borrow the configuration. The cached heuristic config is
    /// rebuilt on the next [`Self::calculate_field_priority`] call.
    #[allow(dead_code)]
    pub fn config_mut(&mut self) -> &mut PriorityConfig {
        &mut self.config
    }

    fn refreshed_heuristic(&self) -> PriorityHeuristicConfig {
        self.config.to_heuristic()
    }

    /// Calculate priority for a single `(path, value)` pair.
    ///
    /// Delegates entirely to
    /// [`pjson_rs_domain::services::compute_priority`].
    pub fn calculate_field_priority(&self, path: &JsonPath, value: &JsonData) -> Priority {
        // The cached heuristic is used when the config has not been mutated
        // since construction; otherwise we project on the fly. The cost of
        // projection is small (vector clones), and `config_mut` is rare on
        // the hot path.
        let derived = self.refreshed_heuristic();
        let cfg = if heuristic_eq(&self.heuristic, &derived) {
            &self.heuristic
        } else {
            &derived
        };
        compute_priority(cfg, path, value)
    }

    /// Extract every field with its priority, walking objects recursively up
    /// to the security depth limit.
    #[allow(dead_code)]
    pub fn extract_prioritized_fields(&self, data: &JsonData) -> Vec<PrioritizedField> {
        self.extract_prioritized_fields_with_limit(data, crate::security::DEFAULT_MAX_DEPTH)
    }

    /// Extract every field with its priority using a custom depth limit.
    pub fn extract_prioritized_fields_with_limit(
        &self,
        data: &JsonData,
        max_depth: usize,
    ) -> Vec<PrioritizedField> {
        let mut fields = Vec::new();
        self.extract_fields_recursive(data, &JsonPath::root(), &mut fields, 0, max_depth);
        fields
    }

    fn extract_fields_recursive(
        &self,
        data: &JsonData,
        current_path: &JsonPath,
        fields: &mut Vec<PrioritizedField>,
        current_depth: usize,
        max_depth: usize,
    ) {
        if current_depth >= max_depth {
            return;
        }

        match data {
            JsonData::Object(map) => {
                for (key, value) in map {
                    if let Ok(field_path) = current_path.append_key(key) {
                        let priority = self.calculate_field_priority(&field_path, value);

                        fields.push(PrioritizedField {
                            path: field_path.clone(),
                            priority,
                            value: value.clone(),
                        });

                        self.extract_fields_recursive(
                            value,
                            &field_path,
                            fields,
                            current_depth + 1,
                            max_depth,
                        );
                    }
                }
            }
            JsonData::Array(arr) => {
                for (index, item) in arr.iter().enumerate() {
                    let indexed_path = current_path.append_index(index);
                    self.extract_fields_recursive(
                        item,
                        &indexed_path,
                        fields,
                        current_depth + 1,
                        max_depth,
                    );
                }
            }
            _ => {}
        }
    }
}

impl Default for PriorityAssigner {
    fn default() -> Self {
        Self::new()
    }
}

fn heuristic_eq(a: &PriorityHeuristicConfig, b: &PriorityHeuristicConfig) -> bool {
    a.critical_fields == b.critical_fields
        && a.high_fields == b.high_fields
        && a.medium_fields == b.medium_fields
        && a.low_fields == b.low_fields
        && a.background_fields == b.background_fields
        && a.large_array_threshold == b.large_array_threshold
        && a.mid_array_threshold == b.mid_array_threshold
        && a.large_string_threshold == b.large_string_threshold
        && a.small_string_threshold == b.small_string_threshold
        && a.large_object_threshold == b.large_object_threshold
}

/// A field paired with its assigned priority and value, returned by
/// [`PriorityAssigner::extract_prioritized_fields`].
#[derive(Debug, Clone)]
pub struct PrioritizedField {
    /// JSON path to the field.
    pub path: JsonPath,
    /// Computed priority for the field.
    pub priority: Priority,
    /// Value at this path (cloned at extraction time).
    pub value: JsonData,
}

impl PrioritizedField {
    /// Construct a prioritized field directly. Mostly useful in tests.
    #[allow(dead_code)]
    pub fn new(path: JsonPath, priority: Priority, value: JsonData) -> Self {
        Self {
            path,
            priority,
            value,
        }
    }
}

/// Group prioritized fields by their priority level.
pub fn group_by_priority(
    fields: Vec<PrioritizedField>,
) -> HashMap<Priority, Vec<PrioritizedField>> {
    let mut groups: HashMap<Priority, Vec<PrioritizedField>> = HashMap::with_capacity(5);

    let avg_fields_per_priority = if fields.len() < 3 {
        fields.len()
    } else {
        fields.len() / 3
    };

    for field in fields {
        groups
            .entry(field.priority)
            .or_insert_with(|| Vec::with_capacity(avg_fields_per_priority))
            .push(field);
    }

    groups
}

/// Sort priorities from highest to lowest.
pub fn sort_priorities(priorities: Vec<Priority>) -> Vec<Priority> {
    let mut sorted = priorities;
    sorted.sort_by(|a, b| b.cmp(a));
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_creation() {
        let config = PriorityConfig::default();
        assert!(config.critical_fields.contains(&"id".to_string()));
        assert!(config.high_fields.contains(&"name".to_string()));
        assert_eq!(config.large_array_threshold, 100);
    }

    #[test]
    fn config_modification() {
        let mut config = PriorityConfig::new();
        config.add_critical_field("custom_id".to_string());
        assert!(config.critical_fields.contains(&"custom_id".to_string()));

        config.add_high_field("display_name".to_string());
        assert!(config.high_fields.contains(&"display_name".to_string()));
    }

    #[test]
    fn field_priority_calculation_matches_domain() {
        let assigner = PriorityAssigner::new();

        let path = JsonPath::new("$.id").expect("valid path");
        let value = JsonData::Integer(123);
        assert_eq!(
            assigner.calculate_field_priority(&path, &value),
            Priority::CRITICAL
        );

        let path = JsonPath::new("$.name").expect("valid path");
        let value = JsonData::String("John".to_string());
        assert_eq!(
            assigner.calculate_field_priority(&path, &value),
            Priority::HIGH
        );

        let path = JsonPath::new("$.reviews").expect("valid path");
        let value = JsonData::Array(vec![]);
        assert_eq!(
            assigner.calculate_field_priority(&path, &value),
            Priority::BACKGROUND
        );
    }

    #[test]
    fn depth_based_priority_uses_domain_fallback() {
        let assigner = PriorityAssigner::new();

        let path = JsonPath::new("$.field").expect("valid path");
        let value = JsonData::String("value".to_string());
        let priority = assigner.calculate_field_priority(&path, &value);
        // depth-1 + short-string = MEDIUM (50) + 20 + 5 = 75 > MEDIUM (50)
        assert!(priority > Priority::MEDIUM);

        let path = JsonPath::new("$.a.b.c.d").expect("valid path");
        let value = JsonData::String("value".to_string());
        let priority = assigner.calculate_field_priority(&path, &value);
        // depth 4 → no boost; short-string → +5 → 55 (still ≥ MEDIUM).
        assert!(priority >= Priority::MEDIUM);

        // Truly deep field (depth > 5) loses 10 points.
        let path = JsonPath::new("$.a.b.c.d.e.f").expect("valid path");
        let value = JsonData::Object(HashMap::new());
        let priority = assigner.calculate_field_priority(&path, &value);
        assert!(priority < Priority::MEDIUM);
    }

    #[test]
    fn config_mut_rebuilds_heuristic_cache() {
        let mut assigner = PriorityAssigner::new();
        assigner
            .config_mut()
            .add_critical_field("user_id".to_string());

        let path = JsonPath::new("$.user_id").expect("valid path");
        let value = JsonData::Integer(1);
        assert_eq!(
            assigner.calculate_field_priority(&path, &value),
            Priority::CRITICAL,
            "mutating the config should be reflected in subsequent priority calls",
        );
    }

    #[test]
    fn extract_prioritized_fields_round_trip() {
        let assigner = PriorityAssigner::new();

        let mut obj = HashMap::new();
        obj.insert("id".to_string(), JsonData::Integer(1));
        obj.insert("name".to_string(), JsonData::String("Test".to_string()));
        let data = JsonData::Object(obj);

        let fields = assigner.extract_prioritized_fields(&data);
        assert_eq!(fields.len(), 2);

        let id_field = fields
            .iter()
            .find(|f| f.path.as_str() == "$.id")
            .expect("id field present");
        assert_eq!(id_field.priority, Priority::CRITICAL);

        let name_field = fields
            .iter()
            .find(|f| f.path.as_str() == "$.name")
            .expect("name field present");
        assert_eq!(name_field.priority, Priority::HIGH);
    }

    #[test]
    fn group_by_priority_buckets_correctly() {
        let fields = vec![
            PrioritizedField::new(
                JsonPath::root(),
                Priority::HIGH,
                JsonData::String("a".to_string()),
            ),
            PrioritizedField::new(
                JsonPath::root(),
                Priority::HIGH,
                JsonData::String("b".to_string()),
            ),
            PrioritizedField::new(
                JsonPath::root(),
                Priority::LOW,
                JsonData::String("c".to_string()),
            ),
        ];

        let groups = group_by_priority(fields);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups.get(&Priority::HIGH).unwrap().len(), 2);
        assert_eq!(groups.get(&Priority::LOW).unwrap().len(), 1);
    }

    #[test]
    fn sort_priorities_descending() {
        let priorities = vec![Priority::LOW, Priority::CRITICAL, Priority::MEDIUM];
        let sorted = sort_priorities(priorities);
        assert_eq!(sorted[0], Priority::CRITICAL);
        assert_eq!(sorted[2], Priority::LOW);
    }
}
