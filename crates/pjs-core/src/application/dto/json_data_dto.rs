//! JsonData Data Transfer Object for API serialization
//!
//! Handles conversion between serde_json::Value (external) and domain JsonData (internal)
//! while keeping domain layer clean of serialization concerns at API boundaries.
//!
//! ## Security Considerations
//!
//! This module relies on parser-layer protections for deeply nested structures.
//! The parser enforces depth limits to prevent stack overflow attacks.
//! Conversions in this module are safe for all valid JsonData instances
//! produced by the parser.

use crate::domain::value_objects::JsonData;
use serde::{Deserialize, Serialize};
use serde_json::Value as SerdeValue;

/// Serializable JSON data representation for API boundaries
///
/// This DTO wraps serde_json::Value for external communication and provides
/// conversion to/from the domain JsonData type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JsonDataDto {
    value: SerdeValue,
}

impl JsonDataDto {
    /// Create from serde_json::Value
    pub fn new(value: SerdeValue) -> Self {
        Self { value }
    }

    /// Get inner serde_json::Value
    pub fn into_inner(self) -> SerdeValue {
        self.value
    }

    /// Get reference to inner value
    pub fn as_value(&self) -> &SerdeValue {
        &self.value
    }
}

impl From<SerdeValue> for JsonDataDto {
    fn from(value: SerdeValue) -> Self {
        Self { value }
    }
}

impl From<JsonDataDto> for SerdeValue {
    fn from(dto: JsonDataDto) -> Self {
        dto.value
    }
}

impl From<JsonDataDto> for JsonData {
    fn from(dto: JsonDataDto) -> Self {
        // Leverages existing From<serde_json::Value> in JsonData
        JsonData::from(dto.value)
    }
}

impl From<&JsonDataDto> for JsonData {
    fn from(dto: &JsonDataDto) -> Self {
        JsonData::from(dto.value.clone())
    }
}

impl From<JsonData> for JsonDataDto {
    fn from(data: JsonData) -> Self {
        Self {
            value: convert_domain_to_serde(&data),
        }
    }
}

/// Convert domain JsonData to serde_json::Value (reverse direction)
///
/// ## Edge Case Handling
///
/// Special float values are converted as follows:
/// - `NaN` → `Null` (serde_json::Number cannot represent NaN)
/// - `Infinity` → `Null` (serde_json::Number cannot represent Infinity)
/// - `NEG_INFINITY` → `Null` (serde_json::Number cannot represent -Infinity)
///
/// This is a safe fallback as JSON specification does not define these values.
fn convert_domain_to_serde(data: &JsonData) -> SerdeValue {
    match data {
        JsonData::Null => SerdeValue::Null,
        JsonData::Bool(b) => SerdeValue::Bool(*b),
        JsonData::Integer(i) => SerdeValue::Number((*i).into()),
        JsonData::Float(f) => serde_json::Number::from_f64(*f)
            .map(SerdeValue::Number)
            .unwrap_or(SerdeValue::Null),
        JsonData::String(s) => SerdeValue::String(s.clone()),
        JsonData::Array(arr) => {
            SerdeValue::Array(arr.iter().map(convert_domain_to_serde).collect())
        }
        JsonData::Object(map) => {
            let obj: serde_json::Map<String, SerdeValue> = map
                .iter()
                .map(|(k, v)| (k.clone(), convert_domain_to_serde(v)))
                .collect();
            SerdeValue::Object(obj)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_data_dto_roundtrip() {
        let original = json!({
            "name": "test",
            "count": 42,
            "active": true,
            "items": [1, 2, 3],
            "nested": {
                "value": null
            }
        });

        let dto = JsonDataDto::from(original.clone());
        let domain: JsonData = dto.clone().into();
        let back: JsonDataDto = domain.into();

        assert_eq!(dto.as_value(), back.as_value());
    }

    #[test]
    fn test_null_conversion() {
        let dto = JsonDataDto::from(SerdeValue::Null);
        let domain: JsonData = dto.into();
        assert!(matches!(domain, JsonData::Null));
    }

    #[test]
    fn test_number_conversion() {
        let int_dto = JsonDataDto::from(json!(42));
        let int_domain: JsonData = int_dto.into();
        assert!(matches!(int_domain, JsonData::Integer(42)));

        let float_dto = JsonDataDto::from(json!(2.5));
        let float_domain: JsonData = float_dto.into();
        if let JsonData::Float(f) = float_domain {
            assert!((f - 2.5).abs() < 0.001);
        } else {
            panic!("Expected Float");
        }
    }

    #[test]
    fn test_serde_serialization() {
        let dto = JsonDataDto::from(json!({"key": "value"}));
        let serialized = serde_json::to_string(&dto).unwrap();
        let deserialized: JsonDataDto = serde_json::from_str(&serialized).unwrap();
        assert_eq!(dto, deserialized);
    }

    #[test]
    fn test_nan_infinity_conversion() {
        // NaN -> Null fallback
        let nan_domain = JsonData::Float(f64::NAN);
        let nan_dto: JsonDataDto = nan_domain.into();
        assert_eq!(nan_dto.as_value(), &SerdeValue::Null);

        // Infinity -> Null fallback
        let inf_domain = JsonData::Float(f64::INFINITY);
        let inf_dto: JsonDataDto = inf_domain.into();
        assert_eq!(inf_dto.as_value(), &SerdeValue::Null);

        // NEG_INFINITY -> Null fallback
        let neg_inf_domain = JsonData::Float(f64::NEG_INFINITY);
        let neg_inf_dto: JsonDataDto = neg_inf_domain.into();
        assert_eq!(neg_inf_dto.as_value(), &SerdeValue::Null);
    }

    #[test]
    fn test_empty_collections() {
        // Empty array
        let empty_array_dto = JsonDataDto::from(json!([]));
        let empty_array_domain: JsonData = empty_array_dto.clone().into();
        let back: JsonDataDto = empty_array_domain.into();
        assert_eq!(empty_array_dto.as_value(), back.as_value());
        assert!(matches!(
            JsonData::from(empty_array_dto),
            JsonData::Array(arr) if arr.is_empty()
        ));

        // Empty object
        let empty_obj_dto = JsonDataDto::from(json!({}));
        let empty_obj_domain: JsonData = empty_obj_dto.clone().into();
        let back: JsonDataDto = empty_obj_domain.into();
        assert_eq!(empty_obj_dto.as_value(), back.as_value());
        assert!(matches!(
            JsonData::from(empty_obj_dto),
            JsonData::Object(obj) if obj.is_empty()
        ));
    }

    #[test]
    fn test_reference_conversion() {
        let dto = JsonDataDto::from(json!({"ref_test": "value"}));

        // Test From<&JsonDataDto> for JsonData
        let domain_from_ref: JsonData = (&dto).into();
        let domain_from_owned: JsonData = dto.clone().into();

        // Both conversions should produce equivalent JsonData
        assert_eq!(
            format!("{:?}", domain_from_ref),
            format!("{:?}", domain_from_owned)
        );
    }

    #[test]
    fn test_number_boundaries() {
        // i64::MAX
        let max_i64 = i64::MAX;
        let max_dto = JsonDataDto::from(json!(max_i64));
        let max_domain: JsonData = max_dto.into();
        assert!(matches!(max_domain, JsonData::Integer(i) if i == max_i64));

        // i64::MIN
        let min_i64 = i64::MIN;
        let min_dto = JsonDataDto::from(json!(min_i64));
        let min_domain: JsonData = min_dto.into();
        assert!(matches!(min_domain, JsonData::Integer(i) if i == min_i64));

        // Large u64 beyond i64::MAX (represented as float in JsonData)
        let large_u64 = u64::MAX;
        let large_dto = JsonDataDto::from(json!(large_u64));
        let large_domain: JsonData = large_dto.into();
        // JsonData converts unsigned integers beyond i64::MAX to Float
        assert!(matches!(large_domain, JsonData::Float(_)));
    }

    #[test]
    fn test_unicode_strings() {
        // Unicode characters
        let unicode_dto = JsonDataDto::from(json!("Hello, 世界"));
        let unicode_domain: JsonData = unicode_dto.clone().into();
        let back: JsonDataDto = unicode_domain.into();
        assert_eq!(unicode_dto.as_value(), back.as_value());

        // Emoji
        let emoji_dto = JsonDataDto::from(json!("🦀 Rust"));
        let emoji_domain: JsonData = emoji_dto.clone().into();
        let back: JsonDataDto = emoji_domain.into();
        assert_eq!(emoji_dto.as_value(), back.as_value());

        // Special characters
        let special_dto = JsonDataDto::from(json!("tab:\t newline:\n quote:\" backslash:\\"));
        let special_domain: JsonData = special_dto.clone().into();
        let back: JsonDataDto = special_domain.into();
        assert_eq!(special_dto.as_value(), back.as_value());
    }
}
