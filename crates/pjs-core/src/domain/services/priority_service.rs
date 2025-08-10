//! Priority calculation and optimization service

use std::collections::HashMap;
use serde_json::Value as JsonValue;
use crate::domain::{
    DomainResult, DomainError,
    value_objects::{Priority, JsonPath},
};

/// Service for computing and optimizing priorities based on business rules
#[derive(Debug, Clone)]
pub struct PriorityService {
    /// Field-based priority rules
    field_rules: HashMap<String, Priority>,
    /// Path-based priority rules (more specific)
    path_rules: HashMap<String, Priority>,
    /// Type-based priority rules
    type_rules: HashMap<String, Priority>,
    /// Default priority for unknown fields
    default_priority: Priority,
}

impl PriorityService {
    /// Create new priority service with default rules
    pub fn new() -> Self {
        let mut service = Self {
            field_rules: HashMap::new(),
            path_rules: HashMap::new(),
            type_rules: HashMap::new(),
            default_priority: Priority::MEDIUM,
        };
        
        // Configure default business rules
        service.add_default_rules();
        service
    }
    
    /// Create priority service with custom default
    pub fn with_default_priority(default_priority: Priority) -> Self {
        let mut service = Self::new();
        service.default_priority = default_priority;
        service
    }
    
    /// Add field-based priority rule
    pub fn add_field_rule(&mut self, field_name: String, priority: Priority) {
        self.field_rules.insert(field_name, priority);
    }
    
    /// Add path-based priority rule (more specific than field rules)
    pub fn add_path_rule(&mut self, path_pattern: String, priority: Priority) {
        self.path_rules.insert(path_pattern, priority);
    }
    
    /// Add type-based priority rule
    pub fn add_type_rule(&mut self, type_name: String, priority: Priority) {
        self.type_rules.insert(type_name, priority);
    }
    
    /// Calculate priority for a specific JSON path and value
    pub fn calculate_priority(&self, path: &JsonPath, value: &JsonValue) -> Priority {
        // 1. Check for exact path match (highest priority)
        if let Some(&priority) = self.path_rules.get(path.as_str()) {
            return priority;
        }
        
        // 2. Check for path pattern matches
        for (pattern, &priority) in &self.path_rules {
            if self.path_matches_pattern(path.as_str(), pattern) {
                return priority;
            }
        }
        
        // 3. Check for field name match
        if let Some(field_name) = self.extract_field_name(path) {
            if let Some(&priority) = self.field_rules.get(&field_name) {
                return priority;
            }
        }
        
        // 4. Check for type-based rules
        let type_name = self.get_value_type_name(value);
        if let Some(&priority) = self.type_rules.get(type_name) {
            return priority;
        }
        
        // 5. Apply heuristic rules
        self.apply_heuristic_priority(path, value)
    }
    
    /// Calculate priorities for all fields in a JSON object
    pub fn calculate_priorities(&self, data: &JsonValue) -> DomainResult<HashMap<JsonPath, Priority>> {
        let mut priorities = HashMap::new();
        self.calculate_priorities_recursive(data, &JsonPath::root(), &mut priorities)?;
        Ok(priorities)
    }
    
    /// Update priority rules based on usage statistics
    pub fn optimize_rules(&mut self, usage_stats: &UsageStatistics) {
        // Increase priority for frequently accessed fields
        for (field, access_count) in &usage_stats.field_access_counts {
            if *access_count > usage_stats.average_access_count * 2 {
                let current = self.field_rules.get(field).copied().unwrap_or(self.default_priority);
                let optimized = current.increase_by(10);
                self.field_rules.insert(field.clone(), optimized);
            }
        }
        
        // Decrease priority for rarely accessed fields
        for (field, access_count) in &usage_stats.field_access_counts {
            if *access_count < usage_stats.average_access_count / 3 {
                let current = self.field_rules.get(field).copied().unwrap_or(self.default_priority);
                let optimized = current.decrease_by(5);
                self.field_rules.insert(field.clone(), optimized);
            }
        }
    }
    
    /// Get priority rules summary
    pub fn get_rules_summary(&self) -> PriorityRulesSummary {
        PriorityRulesSummary {
            field_rule_count: self.field_rules.len(),
            path_rule_count: self.path_rules.len(),
            type_rule_count: self.type_rules.len(),
            default_priority: self.default_priority,
        }
    }
    
    /// Private: Add default business priority rules
    fn add_default_rules(&mut self) {
        // Critical fields - identifiers and status
        self.add_field_rule("id".to_string(), Priority::CRITICAL);
        self.add_field_rule("uuid".to_string(), Priority::CRITICAL);
        self.add_field_rule("status".to_string(), Priority::CRITICAL);
        self.add_field_rule("state".to_string(), Priority::CRITICAL);
        self.add_field_rule("error".to_string(), Priority::CRITICAL);
        
        // High priority - user-visible content
        self.add_field_rule("name".to_string(), Priority::HIGH);
        self.add_field_rule("title".to_string(), Priority::HIGH);
        self.add_field_rule("label".to_string(), Priority::HIGH);
        self.add_field_rule("description".to_string(), Priority::HIGH);
        self.add_field_rule("message".to_string(), Priority::HIGH);
        
        // Medium priority - regular content
        self.add_field_rule("content".to_string(), Priority::MEDIUM);
        self.add_field_rule("body".to_string(), Priority::MEDIUM);
        self.add_field_rule("value".to_string(), Priority::MEDIUM);
        self.add_field_rule("data".to_string(), Priority::MEDIUM);
        
        // Low priority - metadata
        self.add_field_rule("created_at".to_string(), Priority::LOW);
        self.add_field_rule("updated_at".to_string(), Priority::LOW);
        self.add_field_rule("version".to_string(), Priority::LOW);
        self.add_field_rule("metadata".to_string(), Priority::LOW);
        
        // Background priority - analytics and debug info
        self.add_field_rule("analytics".to_string(), Priority::BACKGROUND);
        self.add_field_rule("debug".to_string(), Priority::BACKGROUND);
        self.add_field_rule("trace".to_string(), Priority::BACKGROUND);
        self.add_field_rule("logs".to_string(), Priority::BACKGROUND);
        
        // Path-based rules (more specific)
        self.add_path_rule("$.error".to_string(), Priority::CRITICAL);
        self.add_path_rule("$.*.id".to_string(), Priority::CRITICAL);
        self.add_path_rule("$.users[*].id".to_string(), Priority::CRITICAL);
        self.add_path_rule("$.*.analytics".to_string(), Priority::BACKGROUND);
        
        // Type-based rules
        self.add_type_rule("array_large".to_string(), Priority::LOW); // Arrays > 100 items
        self.add_type_rule("string_long".to_string(), Priority::LOW);  // Strings > 1000 chars
        self.add_type_rule("object_deep".to_string(), Priority::LOW);  // Objects > 5 levels deep
    }
    
    /// Private: Check if path matches pattern (simple glob-like matching)
    fn path_matches_pattern(&self, path: &str, pattern: &str) -> bool {
        // Simple wildcard matching - could be improved with regex
        if pattern.contains('*') {
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.is_empty() {
                return true;
            }
            
            let mut pos = 0;
            for (i, part) in parts.iter().enumerate() {
                if part.is_empty() {
                    continue;
                }
                
                if i == 0 {
                    // First part must match from start
                    if !path.starts_with(part) {
                        return false;
                    }
                    pos = part.len();
                } else if i == parts.len() - 1 {
                    // Last part must match at end
                    if !path[pos..].ends_with(part) {
                        return false;
                    }
                } else {
                    // Middle parts must exist somewhere after current position
                    if let Some(found) = path[pos..].find(part) {
                        pos += found + part.len();
                    } else {
                        return false;
                    }
                }
            }
            true
        } else {
            path == pattern
        }
    }
    
    /// Private: Extract field name from JSON path
    fn extract_field_name(&self, path: &JsonPath) -> Option<String> {
        if let Some(segment) = path.last_segment() {
            match segment {
                crate::domain::value_objects::PathSegment::Key(key) => Some(key),
                _ => None,
            }
        } else {
            None
        }
    }
    
    /// Private: Get type name for value
    fn get_value_type_name(&self, value: &JsonValue) -> &'static str {
        match value {
            JsonValue::Null => "null",
            JsonValue::Bool(_) => "boolean",
            JsonValue::Number(_) => "number",
            JsonValue::String(s) => {
                if s.len() > 1000 {
                    "string_long"
                } else {
                    "string"
                }
            },
            JsonValue::Array(arr) => {
                if arr.len() > 100 {
                    "array_large"
                } else {
                    "array"
                }
            },
            JsonValue::Object(obj) => {
                if obj.len() > 20 {
                    "object_large"
                } else {
                    "object"
                }
            },
        }
    }
    
    /// Private: Apply heuristic-based priority calculation
    fn apply_heuristic_priority(&self, path: &JsonPath, value: &JsonValue) -> Priority {
        let mut priority = self.default_priority;
        
        // Increase priority for shallow paths (more important)
        let depth = path.depth();
        if depth == 1 {
            priority = priority.increase_by(20);
        } else if depth == 2 {
            priority = priority.increase_by(10);
        } else if depth > 5 {
            priority = priority.decrease_by(10);
        }
        
        // Adjust based on value characteristics
        match value {
            JsonValue::String(s) => {
                if s.len() < 50 {
                    // Short strings are likely important labels
                    priority = priority.increase_by(5);
                }
            },
            JsonValue::Array(arr) => {
                if arr.len() > 10 {
                    // Large arrays are less critical for initial display
                    priority = priority.decrease_by(15);
                }
            },
            JsonValue::Object(obj) => {
                if obj.len() > 10 {
                    // Complex objects are less critical for initial display
                    priority = priority.decrease_by(10);
                }
            },
            _ => {}
        }
        
        priority
    }
    
    /// Private: Recursively calculate priorities
    fn calculate_priorities_recursive(
        &self,
        value: &JsonValue,
        current_path: &JsonPath,
        priorities: &mut HashMap<JsonPath, Priority>,
    ) -> DomainResult<()> {
        // Calculate priority for current path
        let priority = self.calculate_priority(current_path, value);
        priorities.insert(current_path.clone(), priority);
        
        // Recurse into child values
        match value {
            JsonValue::Object(obj) => {
                for (key, child_value) in obj.iter() {
                    let child_path = current_path.append_key(key)?;
                    self.calculate_priorities_recursive(child_value, &child_path, priorities)?;
                }
            },
            JsonValue::Array(arr) => {
                for (index, child_value) in arr.iter().enumerate() {
                    let child_path = current_path.append_index(index);
                    self.calculate_priorities_recursive(child_value, &child_path, priorities)?;
                }
            },
            _ => {
                // Leaf values - already calculated priority above
            }
        }
        
        Ok(())
    }
}

impl Default for PriorityService {
    fn default() -> Self {
        Self::new()
    }
}

/// Usage statistics for priority optimization
#[derive(Debug, Clone)]
pub struct UsageStatistics {
    pub field_access_counts: HashMap<String, u64>,
    pub total_accesses: u64,
    pub average_access_count: u64,
}

/// Summary of priority rules configuration
#[derive(Debug, Clone)]
pub struct PriorityRulesSummary {
    pub field_rule_count: usize,
    pub path_rule_count: usize,
    pub type_rule_count: usize,
    pub default_priority: Priority,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_priority_rules() {
        let service = PriorityService::new();
        let path = JsonPath::new("$.id").unwrap();
        let value = JsonValue::String("123".to_string());
        
        let priority = service.calculate_priority(&path, &value);
        assert_eq!(priority, Priority::CRITICAL);
    }
    
    #[test]
    fn test_path_pattern_matching() {
        let service = PriorityService::new();
        
        // Test wildcard pattern
        assert!(service.path_matches_pattern("$.users[0].id", "$.*.id"));
        assert!(service.path_matches_pattern("$.posts[1].id", "$.*.id"));
        assert!(!service.path_matches_pattern("$.users.name", "$.*.id"));
    }
    
    #[test]
    fn test_custom_rules() {
        let mut service = PriorityService::new();
        service.add_field_rule("custom_field".to_string(), Priority::HIGH);
        
        let path = JsonPath::new("$.custom_field").unwrap();
        let value = JsonValue::String("test".to_string());
        
        let priority = service.calculate_priority(&path, &value);
        assert_eq!(priority, Priority::HIGH);
    }
    
    #[test]
    fn test_heuristic_priority() {
        let service = PriorityService::new();
        
        // Shallow path should have higher priority
        let shallow = JsonPath::new("$.name").unwrap();
        let deep = JsonPath::new("$.user.profile.settings.theme.color").unwrap();
        let value = JsonValue::String("test".to_string());
        
        let shallow_priority = service.calculate_priority(&shallow, &value);
        let deep_priority = service.calculate_priority(&deep, &value);
        
        assert!(shallow_priority > deep_priority);
    }
    
    #[test]
    fn test_calculate_all_priorities() {
        let service = PriorityService::new();
        let data = serde_json::json!({
            "id": 123,
            "name": "Test",
            "details": {
                "description": "A test object",
                "metadata": {
                    "created_at": "2023-01-01"
                }
            }
        });
        
        let priorities = service.calculate_priorities(&data).unwrap();
        
        // Should have priorities for all paths
        assert!(!priorities.is_empty());
        
        // ID should be critical
        let id_path = JsonPath::new("$.id").unwrap();
        assert_eq!(priorities[&id_path], Priority::CRITICAL);
        
        // Name should be high
        let name_path = JsonPath::new("$.name").unwrap();
        assert_eq!(priorities[&name_path], Priority::HIGH);
    }
}