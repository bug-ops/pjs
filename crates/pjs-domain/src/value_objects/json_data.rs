//! Domain-specific JSON data value object
//!
//! Provides a Clean Architecture compliant representation of JSON data
//! without depending on external serialization libraries in the domain layer.

use crate::{DomainError, DomainResult};
use serde::de::{Deserialize, Deserializer, Error as DeError, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use std::collections::HashMap;
use std::fmt;

/// Domain-specific representation of JSON-like data
/// This replaces serde_json::Value to maintain Clean Architecture principles
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub enum JsonData {
    #[default]
    /// Null value
    Null,
    /// Boolean value
    Bool(bool),
    /// Integer value
    Integer(i64),
    /// Float value (stored as f64 for simplicity)
    Float(f64),
    /// String value
    String(String),
    /// Array of JsonData values
    Array(Vec<JsonData>),
    /// Object with string keys and JsonData values
    Object(HashMap<String, JsonData>),
}

impl JsonData {
    /// Create a new null value
    pub fn null() -> Self {
        Self::Null
    }

    /// Create a new boolean value
    pub fn bool(value: bool) -> Self {
        Self::Bool(value)
    }

    /// Create a new integer value
    pub fn integer(value: i64) -> Self {
        Self::Integer(value)
    }

    /// Create a new float value.
    ///
    /// Returns `Err` when `value` is NaN or infinite. JSON (RFC 8259 §6) does not
    /// allow non-finite numbers, so a `JsonData` containing one could never be
    /// serialized to valid JSON.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs_domain::value_objects::JsonData;
    ///
    /// assert!(JsonData::float(3.14).is_ok());
    /// assert!(JsonData::float(f64::NAN).is_err());
    /// assert!(JsonData::float(f64::INFINITY).is_err());
    /// ```
    pub fn float(value: f64) -> DomainResult<Self> {
        if value.is_nan() || value.is_infinite() {
            return Err(DomainError::InvalidInput(
                "JSON does not support NaN or infinite float values (RFC 8259 §6)".to_string(),
            ));
        }
        Ok(Self::Float(value))
    }

    /// Create a new string value
    pub fn string<S: Into<String>>(value: S) -> Self {
        Self::String(value.into())
    }

    /// Create a new array value
    pub fn array(values: Vec<JsonData>) -> Self {
        Self::Array(values)
    }

    /// Create a new object value
    pub fn object(values: HashMap<String, JsonData>) -> Self {
        Self::Object(values)
    }

    /// Check if value is null
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Check if value is boolean
    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    /// Check if value is integer
    pub fn is_integer(&self) -> bool {
        matches!(self, Self::Integer(_))
    }

    /// Check if value is float
    pub fn is_float(&self) -> bool {
        matches!(self, Self::Float(_))
    }

    /// Check if value is number (integer or float)
    pub fn is_number(&self) -> bool {
        matches!(self, Self::Integer(_) | Self::Float(_))
    }

    /// Check if value is string
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    /// Check if value is array
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    /// Check if value is object
    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    /// Get boolean value if this is a boolean
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Get integer value if this is an integer
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Get float value if this is a float
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Get string value if this is a string
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Get array value if this is an array
    pub fn as_array(&self) -> Option<&Vec<JsonData>> {
        match self {
            Self::Array(arr) => Some(arr),
            _ => None,
        }
    }

    /// Get mutable array value if this is an array
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<JsonData>> {
        match self {
            Self::Array(arr) => Some(arr),
            _ => None,
        }
    }

    /// Get object value if this is an object
    pub fn as_object(&self) -> Option<&HashMap<String, JsonData>> {
        match self {
            Self::Object(obj) => Some(obj),
            _ => None,
        }
    }

    /// Get mutable object value if this is an object
    pub fn as_object_mut(&mut self) -> Option<&mut HashMap<String, JsonData>> {
        match self {
            Self::Object(obj) => Some(obj),
            _ => None,
        }
    }

    /// Get value by key (if this is an object)
    pub fn get(&self, key: &str) -> Option<&JsonData> {
        match self {
            Self::Object(obj) => obj.get(key),
            _ => None,
        }
    }

    /// Get nested value by path (dot notation)
    pub fn path(&self, path: &str) -> Option<&JsonData> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = self;

        for part in parts {
            match current {
                Self::Object(obj) => {
                    current = obj.get(part)?;
                }
                _ => return None,
            }
        }

        Some(current)
    }

    /// Set nested value by path (dot notation)
    pub fn set_path(&mut self, path: &str, value: JsonData) -> bool {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return false;
        }

        if parts.len() == 1 {
            if let Self::Object(obj) = self {
                obj.insert(parts[0].to_string(), value);
                return true;
            }
            return false;
        }

        // Navigate to parent and create intermediate objects if needed
        let mut current = self;
        for part in &parts[..parts.len() - 1] {
            match current {
                Self::Object(obj) => {
                    if !obj.contains_key(*part) {
                        obj.insert(part.to_string(), Self::object(HashMap::new()));
                    }
                    current = obj
                        .get_mut(*part)
                        .expect("Key must exist as we just inserted it above");
                }
                _ => return false,
            }
        }

        // Set final value
        if let Self::Object(obj) = current {
            obj.insert(parts[parts.len() - 1].to_string(), value);
            true
        } else {
            false
        }
    }

    /// Estimate memory size in bytes
    pub fn memory_size(&self) -> usize {
        match self {
            Self::Null => 1,
            Self::Bool(_) => 1,
            Self::Integer(_) => 8,
            Self::Float(_) => 8,
            Self::String(s) => s.len() * 2, // UTF-16 estimation
            Self::Array(arr) => 8 + arr.iter().map(|v| v.memory_size()).sum::<usize>(),
            Self::Object(obj) => {
                16 + obj
                    .iter()
                    .map(|(k, v)| k.len() * 2 + v.memory_size())
                    .sum::<usize>()
            }
        }
    }
}

impl fmt::Display for JsonData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Integer(i) => write!(f, "{i}"),
            Self::Float(float_val) => write!(f, "{float_val}"),
            Self::String(s) => write!(f, "\"{s}\""),
            Self::Array(arr) => {
                write!(f, "[")?;
                for (i, item) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            Self::Object(obj) => {
                write!(f, "{{")?;
                for (i, (key, value)) in obj.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "\"{key}\":{value}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl Eq for JsonData {}

impl std::hash::Hash for JsonData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Null => 0u8.hash(state),
            Self::Bool(b) => {
                1u8.hash(state);
                b.hash(state);
            }
            Self::Integer(i) => {
                2u8.hash(state);
                i.hash(state);
            }
            Self::Float(f) => {
                3u8.hash(state);
                // For floats, convert to bits for consistent hashing
                f.to_bits().hash(state);
            }
            Self::String(s) => {
                4u8.hash(state);
                s.hash(state);
            }
            Self::Array(arr) => {
                5u8.hash(state);
                arr.hash(state);
            }
            Self::Object(obj) => {
                6u8.hash(state);
                // HashMap doesn't have deterministic iteration order,
                // so we need to sort keys for consistent hashing
                let mut pairs: Vec<_> = obj.iter().collect();
                pairs.sort_by_key(|(k, _)| *k);
                pairs.hash(state);
            }
        }
    }
}

impl From<bool> for JsonData {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for JsonData {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<String> for JsonData {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for JsonData {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<Vec<JsonData>> for JsonData {
    fn from(value: Vec<JsonData>) -> Self {
        Self::Array(value)
    }
}

impl From<HashMap<String, JsonData>> for JsonData {
    fn from(value: HashMap<String, JsonData>) -> Self {
        Self::Object(value)
    }
}

impl From<serde_json::Value> for JsonData {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    Self::Float(f)
                } else {
                    Self::Float(0.0) // fallback
                }
            }
            serde_json::Value::String(s) => Self::String(s),
            serde_json::Value::Array(arr) => {
                let converted: Vec<JsonData> = arr.into_iter().map(JsonData::from).collect();
                Self::Array(converted)
            }
            serde_json::Value::Object(obj) => {
                let converted: HashMap<String, JsonData> = obj
                    .into_iter()
                    .map(|(k, v)| (k, JsonData::from(v)))
                    .collect();
                Self::Object(converted)
            }
        }
    }
}

/// Serializes [`JsonData`] as a plain JSON value rather than as a Rust enum
/// (which would wrap every non-unit variant in a `{"VariantName": ...}`
/// tag). This keeps the wire representation identical to what a client
/// actually sent, and symmetric with the [`Deserialize`] impl below, which
/// expects plain JSON on the way in.
///
/// # Examples
///
/// ```
/// use pjson_rs_domain::value_objects::JsonData;
/// use std::collections::HashMap;
///
/// let data = JsonData::object(HashMap::from([
///     ("a".to_string(), JsonData::integer(1)),
/// ]));
/// assert_eq!(serde_json::to_string(&data).unwrap(), r#"{"a":1}"#);
/// ```
impl Serialize for JsonData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            JsonData::Null => serializer.serialize_unit(),
            JsonData::Bool(b) => serializer.serialize_bool(*b),
            JsonData::Integer(i) => serializer.serialize_i64(*i),
            JsonData::Float(f) => serializer.serialize_f64(*f),
            JsonData::String(s) => serializer.serialize_str(s),
            JsonData::Array(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len()))?;
                for item in arr {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            JsonData::Object(obj) => {
                let mut map = serializer.serialize_map(Some(obj.len()))?;
                for (key, value) in obj {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

/// Deserializes JSON input directly into [`JsonData`], skipping the
/// intermediate `serde_json::Value` tree that `From<serde_json::Value>`
/// would otherwise require building and then walking a second time.
///
/// The implementation drives a [`Visitor`] through
/// [`Deserializer::deserialize_any`], so it works with any self-describing
/// format (not just `serde_json`) and constructs each [`JsonData`] variant
/// in a single pass over the input.
///
/// # Examples
///
/// ```
/// use pjson_rs_domain::value_objects::JsonData;
///
/// let data: JsonData = serde_json::from_str(r#"{"a": 1, "b": [true, null]}"#).unwrap();
/// assert!(data.is_object());
/// assert_eq!(data.get("a").and_then(JsonData::as_i64), Some(1));
/// ```
impl<'de> Deserialize<'de> for JsonData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonDataVisitor)
    }
}

struct JsonDataVisitor;

impl<'de> Visitor<'de> for JsonDataVisitor {
    type Value = JsonData;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a valid JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(JsonData::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(JsonData::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
        Ok(JsonData::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(JsonData::Integer(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        match i64::try_from(v) {
            Ok(i) => Ok(JsonData::Integer(i)),
            Err(_) => Ok(JsonData::Float(v as f64)),
        }
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        if v.is_nan() || v.is_infinite() {
            return Err(E::custom(
                "JSON does not support NaN or infinite float values (RFC 8259 §6)",
            ));
        }
        Ok(JsonData::Float(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(JsonData::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(JsonData::String(v))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut vec = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(elem) = seq.next_element()? {
            vec.push(elem);
        }
        Ok(JsonData::Array(vec))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut obj = HashMap::with_capacity(map.size_hint().unwrap_or(0));
        while let Some((key, value)) = map.next_entry()? {
            obj.insert(key, value);
        }
        Ok(JsonData::Object(obj))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_data_creation() {
        assert_eq!(JsonData::null(), JsonData::Null);
        assert_eq!(JsonData::bool(true), JsonData::Bool(true));
        assert_eq!(JsonData::float(42.0).unwrap(), JsonData::Float(42.0));
        assert_eq!(
            JsonData::string("hello"),
            JsonData::String("hello".to_string())
        );
    }

    #[test]
    fn test_json_data_type_checks() {
        assert!(JsonData::null().is_null());
        assert!(JsonData::bool(true).is_bool());
        assert!(JsonData::float(42.0).unwrap().is_number());
        assert!(JsonData::string("hello").is_string());
        assert!(JsonData::array(vec![]).is_array());
        assert!(JsonData::object(HashMap::new()).is_object());
    }

    #[test]
    fn test_json_data_conversions() {
        assert_eq!(JsonData::bool(true).as_bool(), Some(true));
        assert_eq!(JsonData::float(42.0).unwrap().as_f64(), Some(42.0));
        assert_eq!(JsonData::integer(42).as_i64(), Some(42));
        assert_eq!(JsonData::string("hello").as_str(), Some("hello"));
    }

    #[test]
    fn test_path_operations() {
        let mut data = JsonData::object(HashMap::new());

        // Set nested path
        assert!(data.set_path("user.name", JsonData::string("John")));
        assert!(data.set_path("user.age", JsonData::integer(30)));

        // Get nested path
        assert_eq!(data.path("user.name").unwrap().as_str(), Some("John"));
        assert_eq!(data.path("user.age").unwrap().as_i64(), Some(30));

        // Non-existent path
        assert!(data.path("user.email").is_none());
    }

    #[test]
    fn test_memory_size() {
        let data = JsonData::object(
            [
                ("name".to_string(), JsonData::string("John")),
                ("age".to_string(), JsonData::integer(30)),
            ]
            .into_iter()
            .collect(),
        );

        assert!(data.memory_size() > 0);
    }

    #[test]
    fn test_display() {
        let data = JsonData::object(
            [
                ("name".to_string(), JsonData::string("John")),
                ("active".to_string(), JsonData::bool(true)),
            ]
            .into_iter()
            .collect(),
        );

        let display = format!("{data}");
        assert!(display.contains("name"));
        assert!(display.contains("John"));
    }

    #[test]
    fn test_deserialize_rejects_nan_float() {
        use serde::de::IntoDeserializer;
        let deserializer: serde::de::value::F64Deserializer<serde::de::value::Error> =
            f64::NAN.into_deserializer();
        let result: Result<JsonData, _> = JsonData::deserialize(deserializer);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_rejects_infinite_float() {
        use serde::de::IntoDeserializer;
        let deserializer: serde::de::value::F64Deserializer<serde::de::value::Error> =
            f64::INFINITY.into_deserializer();
        let result: Result<JsonData, _> = JsonData::deserialize(deserializer);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_malformed_json_errors() {
        let result: Result<JsonData, _> = serde_json::from_str("{not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_large_u64_becomes_float() {
        let data: JsonData = serde_json::from_str(&u64::MAX.to_string()).unwrap();
        assert!(matches!(data, JsonData::Float(_)));
    }

    #[test]
    fn test_deserialize_unicode_string_roundtrip() {
        let original = JsonData::string("Hello, 世界 🦀");
        let json = serde_json::to_string(&original).unwrap();
        let back: JsonData = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn test_deserialize_deeply_nested_roundtrip() {
        let mut data = JsonData::string("leaf");
        for _ in 0..64 {
            data = JsonData::array(vec![data]);
        }
        let json = serde_json::to_string(&data).unwrap();
        let back: JsonData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }
}
