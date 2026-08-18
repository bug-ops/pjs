//! Domain-specific JSON data value object
//!
//! Provides a Clean Architecture compliant representation of JSON data
//! without depending on external serialization libraries in the domain layer.

use crate::{DomainError, DomainResult};
use serde::de::{
    Deserialize, DeserializeSeed, Deserializer, Error as DeError, MapAccess, SeqAccess, Visitor,
};
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
        deserializer.deserialize_any(JsonDataVisitor { depth: 0 })
    }
}

/// Maximum container nesting depth accepted when deserializing [`JsonData`].
///
/// `JsonData::deserialize` drives a recursive [`Visitor`] through
/// [`Deserializer::deserialize_any`], so nesting depth in the input maps
/// one-to-one onto stack frames and onto retained per-level preallocations.
/// Without a bound, a few bytes of nested container headers in a
/// length-prefixed self-describing format (MessagePack, CBOR) exhaust the
/// stack (CWE-674) and amplify retained allocation (CWE-789).
///
/// Set to 64, matching `pjson_rs::config::security::JsonLimits`' `max_depth`
/// default. The constant cannot be shared, because the dependency direction
/// is `pjs-core` -> `pjs-domain`, not the reverse; it is `pub` (re-exported at
/// the crate root) so callers configuring their own limits can stay in sync
/// with it rather than duplicating the value blindly. If this value changes,
/// update it alongside the guarding tests in `pjson-rs`'s
/// `config::security::tests` (`test_max_deserialize_depth_matches_domain_guard_defaults`,
/// `test_jiter_config_default_max_depth_matches_domain_guard`) and `pjs-wasm`'s
/// `security::tests::test_default_max_depth_matches_domain_guard`.
///
/// Together with this crate's internal per-collection preallocation cap,
/// this bounds worst-case retained allocation for one `JsonData::deserialize`
/// call to, order of magnitude, `MAX_DESERIALIZE_DEPTH` times that cap —
/// every nesting level can retain up to one level's preallocation while its
/// children are still being read, and `HashMap`'s bucket-table overhead
/// pushes the object case somewhat above that product. This bound is per
/// `deserialize` call, not per process: a server accepting concurrent
/// requests must still bound concurrency or request-body size on top of it.
pub const MAX_DESERIALIZE_DEPTH: usize = 64;

/// Carries the current nesting depth into a nested [`JsonData`] value.
///
/// `Deserialize::deserialize` takes no state, so a nested `next_element()`
/// would restart the visitor at depth 0. `DeserializeSeed` is serde's
/// supported way to thread state through a recursive descent.
struct JsonDataSeed {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for JsonDataSeed {
    type Value = JsonData;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonDataVisitor { depth: self.depth })
    }
}

/// Upper bound, in bytes of `Element`s, on the *element count* a single
/// `size_hint()`-driven preallocation may claim before any element has
/// actually been read from the input.
///
/// This bounds `capacity * size_of::<Element>()`, not the resulting
/// allocation's actual byte size — a `HashMap::with_capacity` call fed this
/// count can still allocate a larger backing table once load-factor and
/// power-of-two bucket rounding are applied (e.g. ~2.65 MiB observed for a
/// 1 MiB-worth-of-elements `HashMap<String, JsonData>` capacity), same as
/// with `serde`'s own equivalent cap.
///
/// Loosely follows the cap `serde`'s own container `Deserialize` impls use
/// via `serde::__private::size_hint::cautious` — that path is not stable
/// public API, so an equivalent bound is reimplemented here. This
/// implementation intentionally diverges for zero-sized `Element`s: `serde`
/// returns 0 (no cap needed, a ZST allocates no memory regardless of count),
/// while this returns `MAX_PREALLOC_BYTES` (harmless for the `Element`
/// types actually used by this module's visitors, both well over one byte).
const MAX_PREALLOC_BYTES: usize = 1024 * 1024;

/// Caps a deserializer-supplied `size_hint()` so a preallocation can never
/// claim more than [`MAX_PREALLOC_BYTES`] worth of `Element`s.
///
/// For length-prefixed self-describing formats (MessagePack, CBOR, etc.)
/// `size_hint()` reflects a value read directly from untrusted input before
/// any element is validated; using it unbounded lets a few bytes of input
/// claim an arbitrarily large allocation (CWE-789).
fn cautious_capacity<Element>(hint: Option<usize>) -> usize {
    match hint {
        Some(hint) => hint.min(MAX_PREALLOC_BYTES / size_of::<Element>().max(1)),
        None => 0,
    }
}

struct JsonDataVisitor {
    depth: usize,
}

impl JsonDataVisitor {
    /// Descends one container level, or rejects once the depth bound is hit.
    fn enter<E: DeError>(&self) -> Result<JsonDataSeed, E> {
        if self.depth >= MAX_DESERIALIZE_DEPTH {
            return Err(E::custom(format_args!(
                "JSON nesting depth exceeds maximum of {MAX_DESERIALIZE_DEPTH}"
            )));
        }
        Ok(JsonDataSeed {
            depth: self.depth + 1,
        })
    }
}

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

    /// Forwards `self` unchanged, so `Option` wrapping does not consume a
    /// depth level — correctly so, since `Some(x)` is not itself a container.
    /// This does mean depth tracking does not cover *every* recursive call
    /// into `deserialize_any`; it is safe only because none of `serde_json`,
    /// `rmp-serde`, or common CBOR deserializers ever call `visit_some` from
    /// `deserialize_any` (self-describing formats decode `Option` via
    /// `visit_none`/direct value visits, not a wrapping `visit_some`).
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
        let seed = self.enter()?;
        let mut vec = Vec::with_capacity(cautious_capacity::<JsonData>(seq.size_hint()));
        while let Some(elem) = seq.next_element_seed(JsonDataSeed { depth: seed.depth })? {
            vec.push(elem);
        }
        Ok(JsonData::Array(vec))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let seed = self.enter()?;
        let mut obj =
            HashMap::with_capacity(cautious_capacity::<(String, JsonData)>(map.size_hint()));
        // next_entry() cannot carry a seed for the value, so key and value are
        // read separately. Keys are String, not JsonData, so they do not recurse.
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value_seed(JsonDataSeed { depth: seed.depth })?;
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

    #[test]
    fn test_cautious_capacity_bounds_hostile_size_hint() {
        // A malicious length-prefixed payload can claim any hint up to
        // usize::MAX; the capped capacity must stay far below that,
        // regardless of what the untrusted hint claims.
        let capped = cautious_capacity::<JsonData>(Some(usize::MAX));
        assert!(capped * size_of::<JsonData>() <= MAX_PREALLOC_BYTES);

        let capped_pair = cautious_capacity::<(String, JsonData)>(Some(usize::MAX));
        assert!(capped_pair * size_of::<(String, JsonData)>() <= MAX_PREALLOC_BYTES);

        // A small, honest hint is never inflated beyond what was asked for.
        assert_eq!(cautious_capacity::<JsonData>(Some(3)), 3);
        assert_eq!(cautious_capacity::<JsonData>(None), 0);
    }

    #[test]
    fn test_cautious_capacity_bounds_mid_range_hostile_size_hint() {
        // usize::MAX overflows `capacity * size_of::<Element>()` and would
        // panic with "capacity overflow" even without a cap, which doesn't
        // exercise the actually-dangerous band: a hint large enough to
        // succeed uncapped (100_000_000 elements * 56 bytes ~= 5.6 GB for
        // `JsonData`) but far more than any legitimate payload needs.
        let hint = 100_000_000;
        let capped = cautious_capacity::<JsonData>(Some(hint));
        assert!(capped < hint);
        assert!(capped * size_of::<JsonData>() <= MAX_PREALLOC_BYTES);
    }

    /// Minimal hand-rolled `SeqAccess` that reports a hostile `size_hint()`
    /// (`usize::MAX`) while only ever yielding `remaining` real elements —
    /// simulates a length-prefixed format (MessagePack/CBOR) lying about
    /// how many elements follow.
    struct HostileSeqAccess {
        remaining: usize,
    }

    impl<'de> SeqAccess<'de> for HostileSeqAccess {
        type Error = serde_json::Error;

        fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
        where
            T: serde::de::DeserializeSeed<'de>,
        {
            if self.remaining == 0 {
                return Ok(None);
            }
            self.remaining -= 1;
            seed.deserialize(serde_json::Value::Null).map(Some)
        }

        fn size_hint(&self) -> Option<usize> {
            Some(usize::MAX)
        }
    }

    struct HostileSeqDeserializer;

    impl<'de> Deserializer<'de> for HostileSeqDeserializer {
        type Error = serde_json::Error;

        fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            visitor.visit_seq(HostileSeqAccess { remaining: 3 })
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map struct enum identifier ignored_any
        }
    }

    #[test]
    fn test_visit_seq_ignores_hostile_size_hint() {
        // If size_hint() were trusted directly, `Vec::with_capacity` would
        // panic with "capacity overflow" (usize::MAX * size_of::<JsonData>()
        // overflows) before reaching this assertion; see
        // test_cautious_capacity_bounds_mid_range_hostile_size_hint for a
        // hint that stays within range and would actually allocate.
        let result: JsonData = JsonData::deserialize(HostileSeqDeserializer).unwrap();
        assert_eq!(
            result,
            JsonData::Array(vec![JsonData::Null, JsonData::Null, JsonData::Null])
        );
    }

    /// Minimal hand-rolled `MapAccess` counterpart to [`HostileSeqAccess`].
    struct HostileMapAccess {
        remaining: usize,
    }

    impl<'de> MapAccess<'de> for HostileMapAccess {
        type Error = serde_json::Error;

        fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
        where
            K: serde::de::DeserializeSeed<'de>,
        {
            if self.remaining == 0 {
                return Ok(None);
            }
            self.remaining -= 1;
            seed.deserialize(serde_json::Value::String(format!("k{}", self.remaining)))
                .map(Some)
        }

        fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
        where
            V: serde::de::DeserializeSeed<'de>,
        {
            seed.deserialize(serde_json::Value::Null)
        }

        fn size_hint(&self) -> Option<usize> {
            Some(usize::MAX)
        }
    }

    struct HostileMapDeserializer;

    impl<'de> Deserializer<'de> for HostileMapDeserializer {
        type Error = serde_json::Error;

        fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            visitor.visit_map(HostileMapAccess { remaining: 2 })
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map struct enum identifier ignored_any
        }
    }

    #[test]
    fn test_visit_map_ignores_hostile_size_hint() {
        let result: JsonData = JsonData::deserialize(HostileMapDeserializer).unwrap();
        assert!(result.is_object());
        assert_eq!(result.as_object().unwrap().len(), 2);
    }

    #[test]
    fn test_serde_json_to_value_roundtrip_primitives() {
        let data = JsonData::string("hello");
        let value = serde_json::to_value(&data).unwrap();
        let back = JsonData::from(value);
        assert_eq!(data, back);
    }

    #[test]
    fn test_serde_json_to_value_roundtrip_complex() {
        let data = JsonData::object(
            [
                ("name".to_string(), JsonData::string("John")),
                ("age".to_string(), JsonData::integer(30)),
                ("active".to_string(), JsonData::bool(true)),
            ]
            .into_iter()
            .collect(),
        );

        let value = serde_json::to_value(&data).unwrap();
        let back = JsonData::from(value);
        assert_eq!(data, back);
    }

    #[test]
    fn test_non_finite_float_serializes_as_json_null() {
        // JsonData::float() rejects NaN/infinite values, so this bypasses
        // the validating constructor directly (only possible inside this
        // crate) to exercise the Serialize impl's non-finite branch, which
        // is the single canonical conversion path now that the divergent
        // JsonAdapter::to_serde_value (NaN/Infinity -> 0) is removed.
        for non_finite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let data = JsonData::Float(non_finite);
            let value = serde_json::to_value(&data).unwrap();
            assert_eq!(value, serde_json::Value::Null);
        }
    }

    /// `levels` nested MessagePack fixarray-of-1 headers wrapping a nil.
    fn nested_msgpack(levels: usize) -> Vec<u8> {
        let mut buf = vec![0x91u8; levels];
        buf.push(0xc0);
        buf
    }

    #[test]
    fn test_deserialize_msgpack_at_max_depth_succeeds() {
        let data: Result<JsonData, _> =
            rmp_serde::from_slice(&nested_msgpack(MAX_DESERIALIZE_DEPTH));
        assert!(data.is_ok(), "{:?}", data.err());
    }

    #[test]
    fn test_deserialize_msgpack_beyond_max_depth_rejected() {
        let err = rmp_serde::from_slice::<JsonData>(&nested_msgpack(MAX_DESERIALIZE_DEPTH + 1))
            .unwrap_err()
            .to_string();
        // Assert on our message, not just is_err(), so the test cannot pass
        // because rmp-serde rejected the input for an unrelated reason.
        assert!(err.contains("nesting depth"), "{err}");
    }

    #[test]
    fn test_deserialize_json_beyond_max_depth_rejected() {
        let levels = MAX_DESERIALIZE_DEPTH + 1;
        let json = format!("{}1{}", "[".repeat(levels), "]".repeat(levels));
        let err = serde_json::from_str::<JsonData>(&json)
            .unwrap_err()
            .to_string();
        assert!(err.contains("nesting depth"), "{err}");
    }

    /// Without the depth guard this aborts the test process with a stack
    /// overflow (verified). nextest runs each test in its own process, so the
    /// abort is reported as a failure rather than taking down the suite.
    ///
    /// 2 MiB gives real margin over the 64 levels of rmp-serde + visitor
    /// frames the guard actually needs (a 512 KiB stack was observed to work
    /// on macOS/arm64 debug builds, but with well under 2x headroom over the
    /// point it started overflowing — too tight to trust across a 3-OS CI
    /// matrix, where MSVC debug frames in particular run larger). 2 MiB is
    /// still far below the ~8 MiB platform default, so a regression that
    /// removed the guard and let this 100 000-level payload recurse
    /// unbounded would still overflow and fail the test.
    #[test]
    fn test_deserialize_extreme_nesting_does_not_overflow_stack() {
        let payload = nested_msgpack(100_000);
        let err = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                rmp_serde::from_slice::<JsonData>(&payload)
                    .unwrap_err()
                    .to_string()
            })
            .expect("thread spawn")
            .join()
            .expect("worker must return an error, not overflow the stack");
        assert!(err.contains("nesting depth"), "{err}");
    }

    /// Nested array32 headers each claiming ~16.7M elements: the depth guard,
    /// not the per-collection cap, is what stops this.
    #[test]
    fn test_deserialize_nested_large_size_hints_rejected_at_depth() {
        let mut buf = Vec::new();
        for _ in 0..200 {
            buf.extend_from_slice(&[0xdd, 0x00, 0xff, 0xff, 0xff]);
        }
        let err = rmp_serde::from_slice::<JsonData>(&buf)
            .unwrap_err()
            .to_string();
        assert!(err.contains("nesting depth"), "{err}");
    }
}
