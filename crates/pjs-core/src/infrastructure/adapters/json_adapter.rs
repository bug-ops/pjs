//! JSON Data Adapter
//!
//! Provides conversion between domain JsonData and infrastructure serde_json::Value
//! to maintain Clean Architecture boundaries.

use crate::domain::value_objects::JsonData;
use serde_json::Value as SerdeValue;
use std::collections::HashMap;

/// Adapter for converting between domain JsonData and infrastructure JSON
pub struct JsonAdapter;

impl JsonAdapter {
    /// Convert domain JsonData to serde_json::Value for infrastructure layer
    pub fn to_serde_value(data: &JsonData) -> SerdeValue {
        match data {
            JsonData::Null => SerdeValue::Null,
            JsonData::Bool(b) => SerdeValue::Bool(*b),
            JsonData::Integer(i) => SerdeValue::Number(serde_json::Number::from(*i)),
            JsonData::Float(f) => SerdeValue::Number(
                serde_json::Number::from_f64(*f).unwrap_or_else(|| serde_json::Number::from(0)),
            ),
            JsonData::String(s) => SerdeValue::String(s.clone()),
            JsonData::Array(arr) => {
                let values: Vec<SerdeValue> = arr.iter().map(Self::to_serde_value).collect();
                SerdeValue::Array(values)
            }
            JsonData::Object(obj) => {
                let mut map = serde_json::Map::new();
                for (key, value) in obj {
                    map.insert(key.clone(), Self::to_serde_value(value));
                }
                SerdeValue::Object(map)
            }
        }
    }

    /// Convert serde_json::Value to domain JsonData
    pub fn from_serde_value(value: &SerdeValue) -> JsonData {
        match value {
            SerdeValue::Null => JsonData::Null,
            SerdeValue::Bool(b) => JsonData::Bool(*b),
            SerdeValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    JsonData::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    JsonData::Float(f)
                } else {
                    JsonData::Integer(0) // Fallback for edge cases
                }
            }
            SerdeValue::String(s) => JsonData::String(s.clone()),
            SerdeValue::Array(arr) => {
                let values: Vec<JsonData> = arr.iter().map(Self::from_serde_value).collect();
                JsonData::Array(values)
            }
            SerdeValue::Object(obj) => {
                let mut map = HashMap::new();
                for (key, value) in obj {
                    map.insert(key.clone(), Self::from_serde_value(value));
                }
                JsonData::Object(map)
            }
        }
    }

    /// Convert JSON string to domain JsonData
    pub fn parse_json_string(json_str: &str) -> Result<JsonData, serde_json::Error> {
        let serde_value: SerdeValue = serde_json::from_str(json_str)?;
        Ok(Self::from_serde_value(&serde_value))
    }

    /// Convert domain JsonData to JSON string
    pub fn to_json_string(data: &JsonData) -> Result<String, serde_json::Error> {
        let serde_value = Self::to_serde_value(data);
        serde_json::to_string(&serde_value)
    }

    /// Convert domain JsonData to pretty JSON string
    pub fn to_json_string_pretty(data: &JsonData) -> Result<String, serde_json::Error> {
        let serde_value = Self::to_serde_value(data);
        serde_json::to_string_pretty(&serde_value)
    }

    /// Merge two JsonData objects (right takes precedence)
    pub fn merge(left: JsonData, right: JsonData) -> JsonData {
        match (left, right) {
            (JsonData::Object(mut left_obj), JsonData::Object(right_obj)) => {
                for (key, value) in right_obj {
                    if let Some(existing) = left_obj.get(&key).cloned() {
                        left_obj.insert(key, Self::merge(existing, value));
                    } else {
                        left_obj.insert(key, value);
                    }
                }
                JsonData::Object(left_obj)
            }
            (_, right) => right, // Right takes precedence for non-objects
        }
    }

    /// Deep clone JsonData
    pub fn deep_clone(data: &JsonData) -> JsonData {
        data.clone() // JsonData implements Clone
    }

    /// Validate that JsonData structure is valid for PJS protocol
    pub fn validate_pjs_structure(data: &JsonData) -> Result<(), String> {
        match data {
            JsonData::Object(_) => Ok(()), // Objects are valid root types
            JsonData::Array(_) => Ok(()),  // Arrays are valid root types
            _ => Err("PJS root data must be object or array".to_string()),
        }
    }

    /// Extract all leaf paths from JsonData for priority analysis
    pub fn extract_leaf_paths(data: &JsonData) -> Vec<String> {
        let mut paths = Vec::new();
        Self::extract_paths_recursive(data, String::new(), &mut paths);
        paths
    }

    fn extract_paths_recursive(data: &JsonData, current_path: String, paths: &mut Vec<String>) {
        match data {
            JsonData::Object(obj) => {
                if obj.is_empty() {
                    paths.push(current_path);
                } else {
                    for (key, value) in obj {
                        let new_path = if current_path.is_empty() {
                            key.clone()
                        } else {
                            format!("{current_path}.{key}")
                        };
                        Self::extract_paths_recursive(value, new_path, paths);
                    }
                }
            }
            JsonData::Array(arr) => {
                if arr.is_empty() {
                    paths.push(current_path);
                } else {
                    for (index, value) in arr.iter().enumerate() {
                        let new_path = if current_path.is_empty() {
                            format!("[{index}]")
                        } else {
                            format!("{current_path}[{index}]")
                        };
                        Self::extract_paths_recursive(value, new_path, paths);
                    }
                }
            }
            _ => {
                // Leaf node
                paths.push(current_path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_conversion_primitives() {
        let json_data = JsonData::string("hello");
        let serde_value = JsonAdapter::to_serde_value(&json_data);
        let converted_back = JsonAdapter::from_serde_value(&serde_value);

        assert_eq!(json_data, converted_back);
    }

    #[test]
    fn test_serde_conversion_complex() {
        let mut obj = HashMap::new();
        obj.insert("name".to_string(), JsonData::string("John"));
        obj.insert("age".to_string(), JsonData::integer(30));
        obj.insert("active".to_string(), JsonData::bool(true));

        let json_data = JsonData::object(obj);
        let serde_value = JsonAdapter::to_serde_value(&json_data);
        let converted_back = JsonAdapter::from_serde_value(&serde_value);

        assert_eq!(json_data, converted_back);
    }

    #[test]
    fn test_json_string_parsing() {
        let json_str = r#"{"name":"John","age":30,"active":true}"#;
        let parsed = JsonAdapter::parse_json_string(json_str).unwrap();

        assert_eq!(parsed.get_path("name").unwrap().as_str(), Some("John"));
        assert_eq!(parsed.get_path("age").unwrap().as_f64(), Some(30.0));
        assert_eq!(parsed.get_path("active").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn test_json_string_generation() {
        let mut obj = HashMap::new();
        obj.insert("name".to_string(), JsonData::string("John"));
        obj.insert("age".to_string(), JsonData::integer(30));

        let json_data = JsonData::object(obj);
        let json_str = JsonAdapter::to_json_string(&json_data).unwrap();

        assert!(json_str.contains("John"));
        assert!(json_str.contains("30"));
    }

    #[test]
    fn test_merge_objects() {
        let mut left = HashMap::new();
        left.insert("name".to_string(), JsonData::string("John"));
        left.insert("age".to_string(), JsonData::integer(25));

        let mut right = HashMap::new();
        right.insert("age".to_string(), JsonData::integer(30));
        right.insert("city".to_string(), JsonData::string("NYC"));

        let merged = JsonAdapter::merge(JsonData::object(left), JsonData::object(right));

        assert_eq!(merged.get_path("name").unwrap().as_str(), Some("John"));
        assert_eq!(merged.get_path("age").unwrap().as_i64(), Some(30)); // Right wins
        assert_eq!(merged.get_path("city").unwrap().as_str(), Some("NYC"));
    }

    #[test]
    fn test_extract_leaf_paths() {
        let mut user = HashMap::new();
        user.insert("name".to_string(), JsonData::string("John"));
        user.insert("age".to_string(), JsonData::integer(30));

        let mut profile = HashMap::new();
        profile.insert("bio".to_string(), JsonData::string("Developer"));
        user.insert("profile".to_string(), JsonData::object(profile));

        let root = JsonData::object(
            [("user".to_string(), JsonData::object(user))]
                .into_iter()
                .collect(),
        );

        let paths = JsonAdapter::extract_leaf_paths(&root);

        assert!(paths.contains(&"user.name".to_string()));
        assert!(paths.contains(&"user.age".to_string()));
        assert!(paths.contains(&"user.profile.bio".to_string()));
    }

    #[test]
    fn test_validate_pjs_structure() {
        // Valid structures
        assert!(JsonAdapter::validate_pjs_structure(&JsonData::object(HashMap::new())).is_ok());
        assert!(JsonAdapter::validate_pjs_structure(&JsonData::array(vec![])).is_ok());

        // Invalid structures
        assert!(JsonAdapter::validate_pjs_structure(&JsonData::string("test")).is_err());
        assert!(JsonAdapter::validate_pjs_structure(&JsonData::integer(42)).is_err());
    }
}
