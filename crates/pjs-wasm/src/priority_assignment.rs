//! Priority assignment logic for WASM
//!
//! This module provides WASM-compatible priority assignment for JSON paths.
//! It analyzes JSON structure and assigns priorities based on configurable rules.

use pjs_domain::value_objects::{JsonData, JsonPath, Priority};
use std::collections::HashMap;

/// Priority assignment rules configuration
#[derive(Debug, Clone)]
pub struct PriorityConfig {
    /// Field names that should have critical priority
    pub critical_fields: Vec<String>,
    /// Field names that should have high priority
    pub high_fields: Vec<String>,
    /// Field patterns that should have low priority
    pub low_patterns: Vec<String>,
    /// Field patterns that should have background priority
    pub background_patterns: Vec<String>,
    /// Maximum array size before downgrading priority
    pub large_array_threshold: usize,
    /// Maximum string length before downgrading priority
    pub large_string_threshold: usize,
}

impl Default for PriorityConfig {
    fn default() -> Self {
        Self {
            critical_fields: vec![
                "id".to_string(),
                "uuid".to_string(),
                "status".to_string(),
                "type".to_string(),
                "kind".to_string(),
            ],
            high_fields: vec![
                "name".to_string(),
                "title".to_string(),
                "label".to_string(),
                "email".to_string(),
                "username".to_string(),
            ],
            low_patterns: vec![
                "analytics".to_string(),
                "stats".to_string(),
                "meta".to_string(),
                "metadata".to_string(),
            ],
            background_patterns: vec![
                "reviews".to_string(),
                "comments".to_string(),
                "logs".to_string(),
                "history".to_string(),
            ],
            large_array_threshold: 100,
            large_string_threshold: 1000,
        }
    }
}

impl PriorityConfig {
    /// Create new configuration with default values
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add critical field name
    pub fn add_critical_field(&mut self, field: String) {
        if !self.critical_fields.contains(&field) {
            self.critical_fields.push(field);
        }
    }

    /// Add high priority field name
    pub fn add_high_field(&mut self, field: String) {
        if !self.high_fields.contains(&field) {
            self.high_fields.push(field);
        }
    }

    /// Add low priority pattern
    pub fn add_low_pattern(&mut self, pattern: String) {
        if !self.low_patterns.contains(&pattern) {
            self.low_patterns.push(pattern);
        }
    }

    /// Add background priority pattern
    pub fn add_background_pattern(&mut self, pattern: String) {
        if !self.background_patterns.contains(&pattern) {
            self.background_patterns.push(pattern);
        }
    }
}

/// Priority assignment engine for JSON data
#[derive(Debug)]
pub struct PriorityAssigner {
    config: PriorityConfig,
}

impl PriorityAssigner {
    /// Create new priority assigner with default configuration
    pub fn new() -> Self {
        Self {
            config: PriorityConfig::default(),
        }
    }

    /// Create priority assigner with custom configuration
    pub fn with_config(config: PriorityConfig) -> Self {
        Self { config }
    }

    /// Get reference to configuration
    #[allow(dead_code)]
    pub fn config(&self) -> &PriorityConfig {
        &self.config
    }

    /// Get mutable reference to configuration
    #[allow(dead_code)]
    pub fn config_mut(&mut self) -> &mut PriorityConfig {
        &mut self.config
    }

    /// Calculate priority for a field based on path and value
    pub fn calculate_field_priority(&self, path: &JsonPath, value: &JsonData) -> Priority {
        // Extract field name from path
        let field_name = if let Some(segment) = path.last_segment() {
            match segment {
                pjs_domain::value_objects::PathSegment::Key(key) => Some(key),
                _ => None,
            }
        } else {
            None
        };

        // Check critical fields
        if let Some(name) = &field_name {
            if self.config.critical_fields.iter().any(|f| f == name) {
                return Priority::CRITICAL;
            }

            // Check high priority fields
            if self.config.high_fields.iter().any(|f| f == name) {
                return Priority::HIGH;
            }

            // Check low priority patterns
            if self
                .config
                .low_patterns
                .iter()
                .any(|p| name.contains(p.as_str()))
            {
                return Priority::LOW;
            }

            // Check background patterns
            if self
                .config
                .background_patterns
                .iter()
                .any(|p| name.contains(p.as_str()))
            {
                return Priority::BACKGROUND;
            }
        }

        // Content-based priority
        match value {
            JsonData::Array(arr) if arr.len() > self.config.large_array_threshold => {
                Priority::BACKGROUND
            }
            JsonData::String(s) if s.len() > self.config.large_string_threshold => Priority::LOW,
            JsonData::Object(obj) if obj.contains_key("timestamp") => Priority::MEDIUM,
            _ => {
                // Depth-based priority: shallower = higher priority
                let depth = path.depth();
                match depth {
                    0..=1 => Priority::HIGH,
                    2..=3 => Priority::MEDIUM,
                    _ => Priority::LOW,
                }
            }
        }
    }

    /// Calculate priority for array elements
    #[allow(dead_code)]
    pub fn calculate_array_priority(&self, path: &JsonPath, elements: &[JsonData]) -> Priority {
        // Large arrays get background priority
        if elements.len() > 50 {
            return Priority::BACKGROUND;
        }

        // Check field name patterns
        if let Some(pjs_domain::value_objects::PathSegment::Key(key)) = path.last_segment() {
            if self
                .config
                .background_patterns
                .iter()
                .any(|p| key.contains(p.as_str()))
            {
                return Priority::BACKGROUND;
            }

            if matches!(key.as_str(), "items" | "data" | "results") {
                return Priority::MEDIUM;
            }
        }

        Priority::MEDIUM
    }

    /// Extract all fields with priorities from JSON data
    #[allow(dead_code)]
    pub fn extract_prioritized_fields(&self, data: &JsonData) -> Vec<PrioritizedField> {
        self.extract_prioritized_fields_with_limit(data, crate::security::DEFAULT_MAX_DEPTH)
    }

    /// Extract all fields with priorities from JSON data with custom depth limit
    pub fn extract_prioritized_fields_with_limit(
        &self,
        data: &JsonData,
        max_depth: usize,
    ) -> Vec<PrioritizedField> {
        let mut fields = Vec::new();
        self.extract_fields_recursive(data, &JsonPath::root(), &mut fields, 0, max_depth);
        fields
    }

    /// Recursively extract fields with priorities (with depth tracking)
    fn extract_fields_recursive(
        &self,
        data: &JsonData,
        current_path: &JsonPath,
        fields: &mut Vec<PrioritizedField>,
        current_depth: usize,
        max_depth: usize,
    ) {
        // Security: Check depth limit to prevent stack overflow
        if current_depth >= max_depth {
            return; // Stop recursion at max depth
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

                        // Recursively process nested structures
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
            _ => {
                // Primitive values are handled by their parent object/array
            }
        }
    }
}

impl Default for PriorityAssigner {
    fn default() -> Self {
        Self::new()
    }
}

/// Field with assigned priority
#[derive(Debug, Clone)]
pub struct PrioritizedField {
    pub path: JsonPath,
    pub priority: Priority,
    pub value: JsonData,
}

impl PrioritizedField {
    /// Create new prioritized field
    #[allow(dead_code)]
    pub fn new(path: JsonPath, priority: Priority, value: JsonData) -> Self {
        Self {
            path,
            priority,
            value,
        }
    }
}

/// Group prioritized fields by priority level
pub fn group_by_priority(
    fields: Vec<PrioritizedField>,
) -> HashMap<Priority, Vec<PrioritizedField>> {
    let mut groups: HashMap<Priority, Vec<PrioritizedField>> = HashMap::new();

    for field in fields {
        groups.entry(field.priority).or_default().push(field);
    }

    groups
}

/// Sort priorities from highest to lowest
pub fn sort_priorities(priorities: Vec<Priority>) -> Vec<Priority> {
    let mut sorted = priorities;
    sorted.sort_by(|a, b| b.cmp(a)); // Descending order (highest first)
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_creation() {
        let config = PriorityConfig::default();
        assert!(config.critical_fields.contains(&"id".to_string()));
        assert!(config.high_fields.contains(&"name".to_string()));
        assert_eq!(config.large_array_threshold, 100);
    }

    #[test]
    fn test_config_modification() {
        let mut config = PriorityConfig::new();
        config.add_critical_field("custom_id".to_string());
        assert!(config.critical_fields.contains(&"custom_id".to_string()));

        config.add_high_field("display_name".to_string());
        assert!(config.high_fields.contains(&"display_name".to_string()));
    }

    #[test]
    fn test_field_priority_calculation() {
        let assigner = PriorityAssigner::new();

        // Test critical field
        let path = JsonPath::new("$.id").expect("Failed to create path in test");
        let value = JsonData::Integer(123);
        assert_eq!(
            assigner.calculate_field_priority(&path, &value),
            Priority::CRITICAL
        );

        // Test high priority field
        let path = JsonPath::new("$.name").expect("Failed to create path in test");
        let value = JsonData::String("John".to_string());
        assert_eq!(
            assigner.calculate_field_priority(&path, &value),
            Priority::HIGH
        );

        // Test background pattern
        let path = JsonPath::new("$.reviews").expect("Failed to create path in test");
        let value = JsonData::Array(vec![]);
        assert_eq!(
            assigner.calculate_field_priority(&path, &value),
            Priority::BACKGROUND
        );
    }

    #[test]
    fn test_depth_based_priority() {
        let assigner = PriorityAssigner::new();

        // Shallow field (depth 1)
        let path = JsonPath::new("$.field").expect("Failed to create path in test");
        let value = JsonData::String("value".to_string());
        let priority = assigner.calculate_field_priority(&path, &value);
        assert!(priority >= Priority::MEDIUM);

        // Deep field (depth 4+)
        let path = JsonPath::new("$.a.b.c.d").expect("Failed to create path in test");
        let value = JsonData::String("value".to_string());
        let priority = assigner.calculate_field_priority(&path, &value);
        assert!(priority <= Priority::LOW);
    }

    #[test]
    fn test_large_array_priority() {
        let assigner = PriorityAssigner::new();
        let path = JsonPath::new("$.items").expect("Failed to create path in test");

        // Small array
        let small_arr = vec![JsonData::Integer(1.into()), JsonData::Integer(2.into())];
        assert_eq!(
            assigner.calculate_array_priority(&path, &small_arr),
            Priority::MEDIUM
        );

        // Large array (>50 elements)
        let large_arr: Vec<JsonData> = (0..100).map(|i| JsonData::Integer(i.into())).collect();
        assert_eq!(
            assigner.calculate_array_priority(&path, &large_arr),
            Priority::BACKGROUND
        );
    }

    #[test]
    fn test_extract_prioritized_fields() {
        let assigner = PriorityAssigner::new();

        let mut obj = HashMap::new();
        obj.insert("id".to_string(), JsonData::Integer(1.into()));
        obj.insert("name".to_string(), JsonData::String("Test".to_string()));
        let data = JsonData::Object(obj);

        let fields = assigner.extract_prioritized_fields(&data);
        assert_eq!(fields.len(), 2);

        // Find ID field
        let id_field = fields.iter().find(|f| f.path.as_str() == "$.id");
        assert!(id_field.is_some());
        assert_eq!(id_field.unwrap().priority, Priority::CRITICAL);

        // Find name field
        let name_field = fields.iter().find(|f| f.path.as_str() == "$.name");
        assert!(name_field.is_some());
        assert_eq!(name_field.unwrap().priority, Priority::HIGH);
    }

    #[test]
    fn test_group_by_priority() {
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
    fn test_sort_priorities() {
        let priorities = vec![Priority::LOW, Priority::CRITICAL, Priority::MEDIUM];
        let sorted = sort_priorities(priorities);

        assert_eq!(sorted[0], Priority::CRITICAL);
        assert_eq!(sorted[2], Priority::LOW);
    }
}
