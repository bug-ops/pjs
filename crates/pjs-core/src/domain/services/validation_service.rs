//! Schema validation domain service
//!
//! Provides core validation logic for JSON data against schemas.
//! This is a domain service as it contains business logic that doesn't
//! naturally fit into a value object or entity.

use std::collections::HashSet;

use crate::domain::value_objects::{
    JsonData, Schema, SchemaValidationError, SchemaValidationResult,
};

#[cfg(feature = "schema-validation")]
use {dashmap::DashMap, std::sync::Arc};

/// Schema validation service
///
/// Validates JSON data against schema definitions following a subset of
/// JSON Schema specification. Designed for high-performance validation
/// in streaming scenarios.
///
/// # Design Philosophy
/// - Zero allocation validation where possible
/// - Early exit on validation failures for performance
/// - Detailed error messages with full path context
/// - Composable validators for complex schemas
///
/// # Examples
/// ```
/// # use pjson_rs::domain::services::ValidationService;
/// # use pjson_rs::domain::value_objects::{Schema, JsonData};
/// let validator = ValidationService::new();
/// let schema = Schema::integer(Some(0), Some(100));
/// let data = JsonData::Integer(50);
///
/// assert!(validator.validate(&data, &schema, "/value").is_ok());
/// ```
pub struct ValidationService {
    /// Maximum validation depth to prevent stack overflow
    max_depth: usize,
    /// Maximum accepted length (in bytes) for a schema's regex `pattern`,
    /// checked before compilation so an oversized pattern is rejected
    /// without ever being compiled or cached.
    #[cfg(feature = "schema-validation")]
    max_pattern_length: usize,
    /// Per-instance cache of compiled patterns, keyed by pattern source.
    ///
    /// Scoped to the service instance (rather than a process-wide
    /// `static`) so cached regexes are reclaimed when the service is
    /// dropped, and bounded by [`Self::MAX_REGEX_CACHE_ENTRIES`] so it
    /// cannot grow without limit for the service's lifetime either.
    #[cfg(feature = "schema-validation")]
    regex_cache: DashMap<String, Arc<regex::Regex>>,
}

impl ValidationService {
    /// Maximum default validation depth
    const DEFAULT_MAX_DEPTH: usize = 32;

    /// Default maximum length (in bytes) for a schema's regex `pattern`.
    #[cfg(feature = "schema-validation")]
    const DEFAULT_MAX_PATTERN_LENGTH: usize = 1024;

    /// Hard ceiling on [`Self::max_pattern_length`], regardless of what a
    /// caller requests via [`Self::with_max_pattern_length`].
    ///
    /// The pattern text is echoed into `InvalidPattern`/`PatternMismatch`
    /// error values, so `max_pattern_length` is also an anti-amplification
    /// bound, not just a compile-cost knob. Clamping to this ceiling means
    /// raising the configured limit can't fully disable that bound (e.g.
    /// `with_max_pattern_length(usize::MAX)` still caps at this value).
    #[cfg(feature = "schema-validation")]
    const MAX_PATTERN_LENGTH_CEILING: usize = 8 * 1024; // 8 KiB

    /// Maximum number of compiled patterns retained in [`Self::regex_cache`]
    /// before it is flushed. Combined with [`Self::REGEX_SIZE_LIMIT`], this
    /// bounds a single thread's view of the cache to roughly
    /// `MAX_REGEX_CACHE_ENTRIES * REGEX_SIZE_LIMIT` = 64 MiB.
    ///
    /// This does **not** bound total process memory on its own: `regex`'s
    /// lazy DFA cache ([`Self::REGEX_DFA_SIZE_LIMIT`]) is a per-thread pool
    /// held inside each compiled `Regex`, so the real worst case scales
    /// additionally with the number of distinct threads that have matched
    /// against each cached pattern — roughly
    /// `entries * (REGEX_SIZE_LIMIT + threads_touched * REGEX_DFA_SIZE_LIMIT)`,
    /// i.e. ≈320 MiB at 64 threads (down from the unbounded, process-lifetime
    /// `static` this cache replaced).
    #[cfg(feature = "schema-validation")]
    const MAX_REGEX_CACHE_ENTRIES: usize = 64;

    /// Maximum compiled program size accepted for a single regex.
    ///
    /// This is one tenth of `regex`'s crate-default `size_limit` (10 MiB),
    /// chosen to be close to the practical floor: ordinary patterns like a bare
    /// `^\d{4}-\d{2}-\d{2}$` date or `^(\d{1,3}\.){3}\d{1,3}$` IPv4 sit right
    /// at 64 KiB, and a plain ISO-8601 timestamp pattern needs 128 KiB, so a
    /// meaningfully smaller limit rejects realistic schema patterns rather
    /// than pathological ones. The memory bound is instead taken from
    /// [`Self::MAX_REGEX_CACHE_ENTRIES`] — see that constant's doc for how
    /// this combines into the cache's overall memory bound, including the
    /// per-thread DFA caveat.
    ///
    /// **Documented limitation**: patterns needing a Unicode character
    /// class under bounded repetition (e.g. `^\p{L}{1,64}$`, ~4 MiB;
    /// `^\p{L}{1,255}$`, 16 MiB) still exceed this limit and are rejected
    /// with [`SchemaValidationError::InvalidPattern`]. Accepting those would
    /// require a limit close to `regex`'s own default (~10 MiB), which
    /// reopens most of the memory-bound gap this fix exists to close; this
    /// is treated as an accepted trade-off, not a bug.
    #[cfg(feature = "schema-validation")]
    const REGEX_SIZE_LIMIT: usize = 1024 * 1024; // 1 MiB

    /// Maximum DFA cache size accepted for a single regex, per thread that
    /// matches against it. See [`Self::MAX_REGEX_CACHE_ENTRIES`].
    #[cfg(feature = "schema-validation")]
    const REGEX_DFA_SIZE_LIMIT: usize = 64 * 1024; // 64 KiB

    /// Create a new validation service with default configuration
    pub fn new() -> Self {
        Self {
            max_depth: Self::DEFAULT_MAX_DEPTH,
            #[cfg(feature = "schema-validation")]
            max_pattern_length: Self::DEFAULT_MAX_PATTERN_LENGTH,
            #[cfg(feature = "schema-validation")]
            regex_cache: DashMap::new(),
        }
    }

    /// Set the maximum nested validation depth.
    ///
    /// # Arguments
    /// * `max_depth` - Maximum nested validation depth
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Set the maximum accepted length (in bytes) for a schema's regex
    /// `pattern`. Patterns longer than this are rejected with
    /// [`SchemaValidationError::PatternTooLong`] before compilation.
    ///
    /// Clamped to a hard, non-configurable ceiling (8 KiB) regardless of
    /// the requested value, since the pattern text is echoed into error
    /// values — this bound can't be raised away to disable that
    /// anti-amplification guard.
    #[cfg(feature = "schema-validation")]
    pub fn with_max_pattern_length(mut self, max_pattern_length: usize) -> Self {
        self.max_pattern_length = max_pattern_length.min(Self::MAX_PATTERN_LENGTH_CEILING);
        self
    }

    /// Validate JSON data against a schema
    ///
    /// Performs comprehensive validation of JSON data against the provided schema,
    /// including type checking, constraint validation, and nested structure validation.
    ///
    /// # Arguments
    /// * `data` - JSON data to validate
    /// * `schema` - Schema to validate against
    /// * `path` - Current JSON path for error reporting
    ///
    /// # Returns
    /// `Ok(())` if validation succeeds, error with details if validation fails
    ///
    /// # Errors
    /// Returns `SchemaValidationError` with context when validation fails
    pub fn validate(
        &self,
        data: &JsonData,
        schema: &Schema,
        path: &str,
    ) -> SchemaValidationResult<()> {
        self.validate_with_depth(data, schema, path, 0)
    }

    /// Internal validation with depth tracking
    fn validate_with_depth(
        &self,
        data: &JsonData,
        schema: &Schema,
        path: &str,
        depth: usize,
    ) -> SchemaValidationResult<()> {
        // Prevent stack overflow from deeply nested structures
        if depth > self.max_depth {
            return Err(SchemaValidationError::TypeMismatch {
                path: path.to_string(),
                expected: "maximum depth not exceeded".to_string(),
                actual: format!("depth {depth} exceeds maximum {}", self.max_depth),
            });
        }

        match schema {
            Schema::Any => Ok(()),
            Schema::Null => self.validate_null(data, path),
            Schema::Boolean => self.validate_boolean(data, path),
            Schema::Integer { minimum, maximum } => {
                self.validate_integer(data, path, *minimum, *maximum)
            }
            Schema::Number { minimum, maximum } => {
                self.validate_number(data, path, *minimum, *maximum)
            }
            Schema::String {
                min_length,
                max_length,
                pattern,
                allowed_values,
            } => self.validate_string(
                data,
                path,
                *min_length,
                *max_length,
                pattern,
                allowed_values,
            ),
            Schema::Array {
                items,
                min_items,
                max_items,
                unique_items,
            } => self.validate_array(
                data,
                path,
                items,
                *min_items,
                *max_items,
                *unique_items,
                depth,
            ),
            Schema::Object {
                properties,
                required,
                additional_properties,
            } => self.validate_object(
                data,
                path,
                properties,
                required,
                *additional_properties,
                depth,
            ),
            Schema::OneOf { schemas } => self.validate_one_of(data, path, schemas, depth),
            Schema::AllOf { schemas } => self.validate_all_of(data, path, schemas, depth),
            _ => Err(SchemaValidationError::TypeMismatch {
                path: path.to_string(),
                expected: "known schema variant".to_string(),
                actual: "unknown".to_string(),
            }),
        }
    }

    fn validate_null(&self, data: &JsonData, path: &str) -> SchemaValidationResult<()> {
        match data {
            JsonData::Null => Ok(()),
            _ => Err(SchemaValidationError::TypeMismatch {
                path: path.to_string(),
                expected: "null".to_string(),
                actual: Self::get_type_name(data).to_string(),
            }),
        }
    }

    fn validate_boolean(&self, data: &JsonData, path: &str) -> SchemaValidationResult<()> {
        match data {
            JsonData::Bool(_) => Ok(()),
            _ => Err(SchemaValidationError::TypeMismatch {
                path: path.to_string(),
                expected: "boolean".to_string(),
                actual: Self::get_type_name(data).to_string(),
            }),
        }
    }

    fn get_type_name(data: &JsonData) -> &'static str {
        match data {
            JsonData::Null => "null",
            JsonData::Bool(_) => "boolean",
            JsonData::Integer(_) => "integer",
            JsonData::Float(_) => "number",
            JsonData::String(_) => "string",
            JsonData::Array(_) => "array",
            JsonData::Object(_) => "object",
            _ => "unknown",
        }
    }

    fn validate_integer(
        &self,
        data: &JsonData,
        path: &str,
        minimum: Option<i64>,
        maximum: Option<i64>,
    ) -> SchemaValidationResult<()> {
        let value = match data {
            JsonData::Integer(v) => *v,
            _ => {
                return Err(SchemaValidationError::TypeMismatch {
                    path: path.to_string(),
                    expected: "integer".to_string(),
                    actual: Self::get_type_name(data).to_string(),
                });
            }
        };

        if let Some(min) = minimum
            && value < min
        {
            return Err(SchemaValidationError::OutOfRange {
                path: path.to_string(),
                value: value.to_string(),
                min: min.to_string(),
                max: maximum.map_or("∞".to_string(), |m| m.to_string()),
            });
        }

        if let Some(max) = maximum
            && value > max
        {
            return Err(SchemaValidationError::OutOfRange {
                path: path.to_string(),
                value: value.to_string(),
                min: minimum.map_or("-∞".to_string(), |m| m.to_string()),
                max: max.to_string(),
            });
        }

        Ok(())
    }

    fn validate_number(
        &self,
        data: &JsonData,
        path: &str,
        minimum: Option<f64>,
        maximum: Option<f64>,
    ) -> SchemaValidationResult<()> {
        let value = match data {
            JsonData::Float(v) => *v,
            JsonData::Integer(v) => *v as f64,
            _ => {
                return Err(SchemaValidationError::TypeMismatch {
                    path: path.to_string(),
                    expected: "number".to_string(),
                    actual: Self::get_type_name(data).to_string(),
                });
            }
        };

        // Validate that the number is finite (not NaN or Infinity)
        if value.is_nan() || value.is_infinite() {
            return Err(SchemaValidationError::TypeMismatch {
                path: path.to_string(),
                expected: "finite number".to_string(),
                actual: format!("{}", value),
            });
        }

        if let Some(min) = minimum
            && value < min
        {
            return Err(SchemaValidationError::OutOfRange {
                path: path.to_string(),
                value: value.to_string(),
                min: min.to_string(),
                max: maximum.map_or("∞".to_string(), |m| m.to_string()),
            });
        }

        if let Some(max) = maximum
            && value > max
        {
            return Err(SchemaValidationError::OutOfRange {
                path: path.to_string(),
                value: value.to_string(),
                min: minimum.map_or("-∞".to_string(), |m| m.to_string()),
                max: max.to_string(),
            });
        }

        Ok(())
    }

    fn validate_string(
        &self,
        data: &JsonData,
        path: &str,
        min_length: Option<usize>,
        max_length: Option<usize>,
        pattern: &Option<String>,
        allowed_values: &Option<smallvec::SmallVec<[String; 8]>>,
    ) -> SchemaValidationResult<()> {
        let value = match data {
            JsonData::String(s) => s,
            _ => {
                return Err(SchemaValidationError::TypeMismatch {
                    path: path.to_string(),
                    expected: "string".to_string(),
                    actual: Self::get_type_name(data).to_string(),
                });
            }
        };

        let len = value.chars().count();

        if let Some(min) = min_length
            && len < min
        {
            return Err(SchemaValidationError::StringLengthConstraint {
                path: path.to_string(),
                actual: len,
                min,
                max: max_length.unwrap_or(usize::MAX),
            });
        }

        if let Some(max) = max_length
            && len > max
        {
            return Err(SchemaValidationError::StringLengthConstraint {
                path: path.to_string(),
                actual: len,
                min: min_length.unwrap_or(0),
                max,
            });
        }

        if let Some(allowed) = allowed_values
            && !allowed.iter().any(|v| v.as_str() == value)
        {
            return Err(SchemaValidationError::InvalidEnumValue {
                path: path.to_string(),
                value: value.clone(),
            });
        }

        #[cfg(feature = "schema-validation")]
        if let Some(pat) = pattern {
            // Reject oversized patterns before compiling or caching them.
            // This also bounds how much pattern text can ever reach
            // `InvalidPattern`/`PatternMismatch` below, so a multi-megabyte
            // attacker-controlled pattern can't be re-amplified into error
            // values or logs.
            if pat.len() > self.max_pattern_length {
                return Err(SchemaValidationError::PatternTooLong {
                    path: path.to_string(),
                    length: pat.len(),
                    max: self.max_pattern_length,
                });
            }

            if self.regex_cache.len() >= Self::MAX_REGEX_CACHE_ENTRIES {
                // Generation-flush eviction: len()-then-clear is not atomic
                // under dashmap, so concurrent inserters can transiently
                // overshoot this cap by roughly the number of racing
                // threads. That overshoot is bounded and acceptable, not a
                // correctness bug.
                self.regex_cache.clear();
            }

            let re: Arc<regex::Regex> = {
                let guard = self.regex_cache.entry(pat.clone()).or_try_insert_with(|| {
                    regex::RegexBuilder::new(pat)
                        .size_limit(Self::REGEX_SIZE_LIMIT)
                        .dfa_size_limit(Self::REGEX_DFA_SIZE_LIMIT)
                        .build()
                        .map(Arc::new)
                        .map_err(|e| SchemaValidationError::InvalidPattern {
                            path: path.to_string(),
                            pattern: pat.clone(),
                            reason: e.to_string(),
                        })
                })?;
                // Clone the Arc and let the dashmap shard guard drop here,
                // instead of holding it across `is_match` below — matching
                // regexes against large input strings would otherwise
                // serialize all requests hashing to the same shard.
                guard.value().clone()
            };

            if !re.is_match(value) {
                return Err(SchemaValidationError::PatternMismatch {
                    path: path.to_string(),
                    value: value.clone(),
                    pattern: pat.clone(),
                });
            }
        }

        #[cfg(not(feature = "schema-validation"))]
        let _ = pattern;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_array(
        &self,
        data: &JsonData,
        path: &str,
        items: &Option<Box<Schema>>,
        min_items: Option<usize>,
        max_items: Option<usize>,
        unique_items: bool,
        depth: usize,
    ) -> SchemaValidationResult<()> {
        let arr = match data {
            JsonData::Array(a) => a,
            _ => {
                return Err(SchemaValidationError::TypeMismatch {
                    path: path.to_string(),
                    expected: "array".to_string(),
                    actual: Self::get_type_name(data).to_string(),
                });
            }
        };

        let len = arr.len();

        if let Some(min) = min_items
            && len < min
        {
            return Err(SchemaValidationError::ArraySizeConstraint {
                path: path.to_string(),
                actual: len,
                min,
                max: max_items.unwrap_or(usize::MAX),
            });
        }

        if let Some(max) = max_items
            && len > max
        {
            return Err(SchemaValidationError::ArraySizeConstraint {
                path: path.to_string(),
                actual: len,
                min: min_items.unwrap_or(0),
                max,
            });
        }

        if unique_items {
            let mut seen = HashSet::with_capacity(arr.len());
            for item in arr {
                // Use JsonData's Hash implementation directly for efficient uniqueness check
                if !seen.insert(item) {
                    return Err(SchemaValidationError::DuplicateItems {
                        path: path.to_string(),
                    });
                }
            }
        }

        if let Some(item_schema) = items {
            // Pre-allocate path buffer to avoid repeated allocations
            let mut path_buffer = String::with_capacity(path.len() + 16);
            for (i, item) in arr.iter().enumerate() {
                path_buffer.clear();
                path_buffer.push_str(path);
                path_buffer.push('[');
                use std::fmt::Write;
                write!(&mut path_buffer, "{}", i).unwrap();
                path_buffer.push(']');

                self.validate_with_depth(item, item_schema, &path_buffer, depth + 1)?;
            }
        }

        Ok(())
    }

    fn validate_object(
        &self,
        data: &JsonData,
        path: &str,
        properties: &std::collections::HashMap<String, Schema>,
        required: &[String],
        additional_properties: bool,
        depth: usize,
    ) -> SchemaValidationResult<()> {
        let obj = match data {
            JsonData::Object(o) => o,
            _ => {
                return Err(SchemaValidationError::TypeMismatch {
                    path: path.to_string(),
                    expected: "object".to_string(),
                    actual: Self::get_type_name(data).to_string(),
                });
            }
        };

        // Check required fields
        for field in required {
            if !obj.contains_key(field) {
                return Err(SchemaValidationError::MissingRequired {
                    path: path.to_string(),
                    field: field.clone(),
                });
            }
        }

        // Validate defined properties
        let mut path_buffer = String::with_capacity(path.len() + 32);
        for (key, value) in obj {
            if let Some(prop_schema) = properties.get(key) {
                path_buffer.clear();
                path_buffer.push_str(path);
                path_buffer.push('/');
                path_buffer.push_str(key);
                self.validate_with_depth(value, prop_schema, &path_buffer, depth + 1)?;
            } else if !additional_properties {
                return Err(SchemaValidationError::AdditionalPropertyNotAllowed {
                    path: path.to_string(),
                    property: key.clone(),
                });
            }
        }

        Ok(())
    }

    fn validate_one_of(
        &self,
        data: &JsonData,
        path: &str,
        schemas: &[Box<Schema>],
        depth: usize,
    ) -> SchemaValidationResult<()> {
        let mut match_count = 0;

        for schema in schemas {
            if self
                .validate_with_depth(data, schema, path, depth + 1)
                .is_ok()
            {
                match_count += 1;
                // Early exit: if we found 2 matches, we know it's invalid
                if match_count > 1 {
                    return Err(SchemaValidationError::NoMatchingOneOf {
                        path: path.to_string(),
                    });
                }
            }
        }

        if match_count == 1 {
            Ok(())
        } else {
            Err(SchemaValidationError::NoMatchingOneOf {
                path: path.to_string(),
            })
        }
    }

    fn validate_all_of(
        &self,
        data: &JsonData,
        path: &str,
        schemas: &[Box<Schema>],
        depth: usize,
    ) -> SchemaValidationResult<()> {
        let mut failures = Vec::new();

        for (i, schema) in schemas.iter().enumerate() {
            if self
                .validate_with_depth(data, schema, path, depth + 1)
                .is_err()
            {
                failures.push(i);
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(SchemaValidationError::AllOfFailure {
                path: path.to_string(),
                failures: failures
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
        }
    }
}

impl Default for ValidationService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_validate_null() {
        let validator = ValidationService::new();
        let schema = Schema::Null;
        let data = JsonData::Null;

        assert!(validator.validate(&data, &schema, "/").is_ok());

        let invalid = JsonData::Integer(42);
        assert!(validator.validate(&invalid, &schema, "/").is_err());
    }

    #[test]
    fn test_validate_boolean() {
        let validator = ValidationService::new();
        let schema = Schema::Boolean;

        assert!(
            validator
                .validate(&JsonData::Bool(true), &schema, "/flag")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Bool(false), &schema, "/flag")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(1), &schema, "/flag")
                .is_err()
        );
    }

    #[test]
    fn test_validate_integer_range() {
        let validator = ValidationService::new();
        let schema = Schema::integer(Some(0), Some(100));

        assert!(
            validator
                .validate(&JsonData::Integer(50), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(0), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(100), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(-1), &schema, "/value")
                .is_err()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(101), &schema, "/value")
                .is_err()
        );
    }

    #[test]
    fn test_validate_string_length() {
        let validator = ValidationService::new();
        let schema = Schema::string(Some(2), Some(10));

        assert!(
            validator
                .validate(&JsonData::String("hello".to_string()), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("hi".to_string()), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("0123456789".to_string()), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("a".to_string()), &schema, "/")
                .is_err()
        );
        assert!(
            validator
                .validate(&JsonData::String("12345678901".to_string()), &schema, "/")
                .is_err()
        );
    }

    #[test]
    fn test_validate_array() {
        let validator = ValidationService::new();
        let schema = Schema::Array {
            items: Some(Box::new(Schema::integer(Some(0), None))),
            min_items: Some(1),
            max_items: Some(5),
            unique_items: false,
        };

        let valid = JsonData::Array(vec![JsonData::Integer(1), JsonData::Integer(2)]);
        assert!(validator.validate(&valid, &schema, "/items").is_ok());

        let empty = JsonData::Array(vec![]);
        assert!(validator.validate(&empty, &schema, "/items").is_err());

        let invalid_item = JsonData::Array(vec![JsonData::Integer(-1)]);
        assert!(
            validator
                .validate(&invalid_item, &schema, "/items")
                .is_err()
        );
    }

    #[test]
    fn test_validate_object() {
        let validator = ValidationService::new();
        let mut properties = HashMap::new();
        properties.insert("id".to_string(), Schema::integer(Some(1), None));
        properties.insert("name".to_string(), Schema::string(Some(1), Some(100)));

        let schema = Schema::object(properties, vec!["id".to_string()]);

        let mut valid_obj = HashMap::new();
        valid_obj.insert("id".to_string(), JsonData::Integer(42));
        valid_obj.insert("name".to_string(), JsonData::String("test".to_string()));

        let valid = JsonData::Object(valid_obj);
        assert!(validator.validate(&valid, &schema, "/user").is_ok());

        let mut missing_required = HashMap::new();
        missing_required.insert("name".to_string(), JsonData::String("test".to_string()));
        let invalid = JsonData::Object(missing_required);
        assert!(validator.validate(&invalid, &schema, "/user").is_err());
    }

    #[test]
    fn test_validate_number() {
        let validator = ValidationService::new();
        let schema = Schema::number(Some(0.0), Some(100.0));

        assert!(
            validator
                .validate(&JsonData::Float(50.0), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(50), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Float(0.0), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Float(100.0), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Float(-0.1), &schema, "/value")
                .is_err()
        );
        assert!(
            validator
                .validate(&JsonData::Float(100.1), &schema, "/value")
                .is_err()
        );
    }

    #[test]
    fn test_validate_number_nan_infinity() {
        let validator = ValidationService::new();
        let schema = Schema::number(Some(0.0), Some(100.0));

        // NaN should be rejected
        let result = validator.validate(&JsonData::Float(f64::NAN), &schema, "/value");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaValidationError::TypeMismatch { .. }));

        // Infinity should be rejected
        let result = validator.validate(&JsonData::Float(f64::INFINITY), &schema, "/value");
        assert!(result.is_err());

        // Negative infinity should be rejected
        let result = validator.validate(&JsonData::Float(f64::NEG_INFINITY), &schema, "/value");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_string_enum_values() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let allowed_values = Some(SmallVec::from_vec(vec![
            String::from("red"),
            String::from("green"),
            String::from("blue"),
        ]));

        let schema = Schema::String {
            min_length: None,
            max_length: None,
            pattern: None,
            allowed_values,
        };

        // Valid enum values
        assert!(
            validator
                .validate(&JsonData::String("red".to_string()), &schema, "/color")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("green".to_string()), &schema, "/color")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("blue".to_string()), &schema, "/color")
                .is_ok()
        );

        // Invalid enum value
        let result = validator.validate(&JsonData::String("yellow".to_string()), &schema, "/color");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            SchemaValidationError::InvalidEnumValue { .. }
        ));
    }

    #[test]
    fn test_validate_array_unique_items() {
        let validator = ValidationService::new();
        let schema = Schema::Array {
            items: Some(Box::new(Schema::integer(None, None))),
            min_items: None,
            max_items: None,
            unique_items: true,
        };

        // Valid: all unique items
        let unique = JsonData::Array(vec![
            JsonData::Integer(1),
            JsonData::Integer(2),
            JsonData::Integer(3),
        ]);
        assert!(validator.validate(&unique, &schema, "/items").is_ok());

        // Invalid: duplicate items
        let duplicates = JsonData::Array(vec![
            JsonData::Integer(1),
            JsonData::Integer(2),
            JsonData::Integer(1),
        ]);
        let result = validator.validate(&duplicates, &schema, "/items");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaValidationError::DuplicateItems { .. }));
    }

    #[test]
    fn test_validate_array_min_max_items() {
        let validator = ValidationService::new();
        let schema = Schema::Array {
            items: None,
            min_items: Some(2),
            max_items: Some(4),
            unique_items: false,
        };

        // Valid: within range
        let valid = JsonData::Array(vec![JsonData::Integer(1), JsonData::Integer(2)]);
        assert!(validator.validate(&valid, &schema, "/items").is_ok());

        // Invalid: too few items
        let too_few = JsonData::Array(vec![JsonData::Integer(1)]);
        let result = validator.validate(&too_few, &schema, "/items");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            SchemaValidationError::ArraySizeConstraint { .. }
        ));

        // Invalid: too many items
        let too_many = JsonData::Array(vec![
            JsonData::Integer(1),
            JsonData::Integer(2),
            JsonData::Integer(3),
            JsonData::Integer(4),
            JsonData::Integer(5),
        ]);
        let result = validator.validate(&too_many, &schema, "/items");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SchemaValidationError::ArraySizeConstraint { .. }
        ));
    }

    #[test]
    fn test_validate_object_additional_properties() {
        let validator = ValidationService::new();
        let mut properties = HashMap::new();
        properties.insert("name".to_string(), Schema::string(Some(1), Some(100)));

        // Schema disallows additional properties
        let schema = Schema::Object {
            properties: properties.clone(),
            required: vec![],
            additional_properties: false,
        };

        let mut valid_obj = HashMap::new();
        valid_obj.insert("name".to_string(), JsonData::String("test".to_string()));

        // Valid: no additional properties
        let valid = JsonData::Object(valid_obj.clone());
        assert!(validator.validate(&valid, &schema, "/obj").is_ok());

        // Invalid: has additional property
        let mut invalid_obj = valid_obj;
        invalid_obj.insert("extra".to_string(), JsonData::Integer(42));
        let invalid = JsonData::Object(invalid_obj);
        let result = validator.validate(&invalid, &schema, "/obj");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            SchemaValidationError::AdditionalPropertyNotAllowed { .. }
        ));

        // Schema allows additional properties
        let schema_allow = Schema::Object {
            properties,
            required: vec![],
            additional_properties: true,
        };

        let mut obj_with_extra = HashMap::new();
        obj_with_extra.insert("name".to_string(), JsonData::String("test".to_string()));
        obj_with_extra.insert("extra".to_string(), JsonData::Integer(42));
        let with_extra = JsonData::Object(obj_with_extra);
        assert!(
            validator
                .validate(&with_extra, &schema_allow, "/obj")
                .is_ok()
        );
    }

    #[test]
    fn test_validate_one_of_single_match() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let schema = Schema::OneOf {
            schemas: SmallVec::from_vec(vec![
                Box::new(Schema::string(Some(1), None)),
                Box::new(Schema::integer(Some(0), None)),
            ]),
        };

        // Valid: matches exactly one schema (string)
        assert!(
            validator
                .validate(&JsonData::String("test".to_string()), &schema, "/value")
                .is_ok()
        );

        // Valid: matches exactly one schema (integer)
        assert!(
            validator
                .validate(&JsonData::Integer(42), &schema, "/value")
                .is_ok()
        );
    }

    #[test]
    fn test_validate_one_of_no_match() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let schema = Schema::OneOf {
            schemas: SmallVec::from_vec(vec![
                Box::new(Schema::string(Some(5), None)),    // min length 5
                Box::new(Schema::integer(Some(100), None)), // min 100
            ]),
        };

        // Invalid: matches no schemas (string too short, not an integer)
        let result = validator.validate(&JsonData::String("hi".to_string()), &schema, "/value");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SchemaValidationError::NoMatchingOneOf { .. }
        ));

        // Invalid: matches no schemas (integer too small, not a string)
        let result = validator.validate(&JsonData::Integer(50), &schema, "/value");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_one_of_multiple_matches() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let schema = Schema::OneOf {
            schemas: SmallVec::from_vec(vec![
                Box::new(Schema::integer(None, None)), // matches any integer
                Box::new(Schema::integer(Some(0), Some(100))), // matches integers 0-100
            ]),
        };

        // Invalid: matches both schemas (ambiguous)
        let result = validator.validate(&JsonData::Integer(50), &schema, "/value");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SchemaValidationError::NoMatchingOneOf { .. }
        ));
    }

    #[test]
    fn test_validate_all_of_success() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let schema = Schema::AllOf {
            schemas: SmallVec::from_vec(vec![
                Box::new(Schema::integer(Some(0), None)),   // >= 0
                Box::new(Schema::integer(None, Some(100))), // <= 100
            ]),
        };

        // Valid: matches all schemas
        assert!(
            validator
                .validate(&JsonData::Integer(50), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(0), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(100), &schema, "/value")
                .is_ok()
        );
    }

    #[test]
    fn test_validate_all_of_failure() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let schema = Schema::AllOf {
            schemas: SmallVec::from_vec(vec![
                Box::new(Schema::integer(Some(0), None)),   // >= 0
                Box::new(Schema::integer(None, Some(100))), // <= 100
            ]),
        };

        // Invalid: fails first constraint
        let result = validator.validate(&JsonData::Integer(-1), &schema, "/value");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaValidationError::AllOfFailure { .. }));

        // Invalid: fails second constraint
        let result = validator.validate(&JsonData::Integer(101), &schema, "/value");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SchemaValidationError::AllOfFailure { .. }
        ));
    }

    #[test]
    fn test_validate_max_depth_exceeded() {
        let validator = ValidationService::new().with_max_depth(5);

        // Create nested structure that exceeds max depth
        fn create_nested(depth: usize) -> JsonData {
            if depth == 0 {
                JsonData::Integer(42)
            } else {
                let mut obj = HashMap::new();
                obj.insert("nested".to_string(), create_nested(depth - 1));
                JsonData::Object(obj)
            }
        }

        fn create_nested_schema(depth: usize) -> Schema {
            if depth == 0 {
                Schema::integer(None, None)
            } else {
                Schema::Object {
                    properties: [("nested".to_string(), create_nested_schema(depth - 1))]
                        .into_iter()
                        .collect(),
                    required: vec![],
                    additional_properties: false,
                }
            }
        }

        let data = create_nested(10);
        let schema = create_nested_schema(10);

        // Should fail due to depth limit
        let result = validator.validate(&data, &schema, "/deep");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaValidationError::TypeMismatch { .. }));
    }

    #[test]
    fn test_validate_any_schema() {
        let validator = ValidationService::new();
        let schema = Schema::Any;

        // Any schema accepts all types
        assert!(validator.validate(&JsonData::Null, &schema, "/").is_ok());
        assert!(
            validator
                .validate(&JsonData::Bool(true), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(42), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Float(std::f64::consts::PI), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("test".to_string()), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Array(vec![]), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Object(HashMap::new()), &schema, "/")
                .is_ok()
        );
    }

    #[test]
    fn test_validate_type_mismatches() {
        let validator = ValidationService::new();

        // Test all type mismatches
        let test_cases = vec![
            (Schema::Null, JsonData::Integer(42), "null"),
            (
                Schema::Boolean,
                JsonData::String("true".to_string()),
                "boolean",
            ),
            (
                Schema::integer(None, None),
                JsonData::String("42".to_string()),
                "integer",
            ),
            (
                Schema::number(None, None),
                JsonData::String("3.14".to_string()),
                "number",
            ),
            (Schema::string(None, None), JsonData::Integer(42), "string"),
            (
                Schema::Array {
                    items: None,
                    min_items: None,
                    max_items: None,
                    unique_items: false,
                },
                JsonData::Integer(42),
                "array",
            ),
            (
                Schema::Object {
                    properties: HashMap::new(),
                    required: vec![],
                    additional_properties: true,
                },
                JsonData::Integer(42),
                "object",
            ),
        ];

        for (schema, data, expected_type) in test_cases {
            let result = validator.validate(&data, &schema, "/test");
            assert!(result.is_err(), "Expected error for {expected_type}");
            let err = result.unwrap_err();
            assert!(
                matches!(err, SchemaValidationError::TypeMismatch { .. }),
                "Expected TypeMismatch for {expected_type}"
            );
        }
    }

    #[test]
    fn test_default_validation_service() {
        let default = ValidationService::default();
        let created = ValidationService::new();

        // Both should have same max_depth
        let schema = Schema::integer(None, None);
        let data = JsonData::Integer(42);

        assert!(default.validate(&data, &schema, "/").is_ok());
        assert!(created.validate(&data, &schema, "/").is_ok());
    }

    #[test]
    #[cfg(feature = "schema-validation")]
    fn test_validate_string_pattern() {
        let validator = ValidationService::new();
        let schema = Schema::String {
            min_length: None,
            max_length: None,
            pattern: Some("^[a-z]+$".to_string()),
            allowed_values: None,
        };

        assert!(
            validator
                .validate(&JsonData::String("hello".to_string()), &schema, "/name")
                .is_ok()
        );

        let result = validator.validate(&JsonData::String("12345".to_string()), &schema, "/name");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SchemaValidationError::PatternMismatch { .. }
        ));
    }

    #[test]
    #[cfg(feature = "schema-validation")]
    fn test_validate_string_invalid_regex_pattern() {
        let validator = ValidationService::new();
        let schema = Schema::String {
            min_length: None,
            max_length: None,
            pattern: Some("(unclosed".to_string()),
            allowed_values: None,
        };

        let result = validator.validate(&JsonData::String("hello".to_string()), &schema, "$");
        assert!(
            matches!(result, Err(SchemaValidationError::InvalidPattern { .. })),
            "expected InvalidPattern for a syntactically invalid regex, got: {:?}",
            result
        );
    }

    #[test]
    #[cfg(feature = "schema-validation")]
    fn test_validate_string_pattern_too_long_rejected() {
        let validator = ValidationService::new().with_max_pattern_length(8);
        // Deliberately an *invalid* regex (unbalanced parens): if the
        // length check ran after compilation instead of before, this
        // would surface as `InvalidPattern`, not `PatternTooLong` — this
        // test would then pass for the wrong reason with a merely
        // oversized-but-valid pattern like `"a".repeat(9)`.
        let schema = Schema::String {
            min_length: None,
            max_length: None,
            pattern: Some("(".repeat(9)),
            allowed_values: None,
        };

        let result = validator.validate(&JsonData::String("aaa".to_string()), &schema, "/name");
        assert!(matches!(
            result,
            Err(SchemaValidationError::PatternTooLong {
                length: 9,
                max: 8,
                ..
            })
        ));
    }

    #[test]
    #[cfg(feature = "schema-validation")]
    fn test_validate_string_pattern_at_max_length_is_accepted() {
        // Boundary: exactly `max_pattern_length` bytes must be accepted
        // (the check is `len > max`, not `len >= max`); a regression to
        // `>=` would reject this and only this test would catch it.
        let validator = ValidationService::new().with_max_pattern_length(8);
        let schema = Schema::String {
            min_length: None,
            max_length: None,
            pattern: Some("a".repeat(8)),
            allowed_values: None,
        };

        let result =
            validator.validate(&JsonData::String("aaaaaaaa".to_string()), &schema, "/name");
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(feature = "schema-validation")]
    fn test_regex_cache_stays_bounded_across_many_patterns() {
        let validator = ValidationService::new();

        // Compile far more distinct patterns than the cache cap, checking
        // it never grows past the bound (allowing slack for the
        // documented non-atomic flush, which is single-threaded-safe here).
        for i in 0..(ValidationService::MAX_REGEX_CACHE_ENTRIES * 3) {
            let schema = Schema::String {
                min_length: None,
                max_length: None,
                pattern: Some(format!("^pattern-{i}$")),
                allowed_values: None,
            };
            let data = JsonData::String(format!("pattern-{i}"));
            assert!(validator.validate(&data, &schema, "/value").is_ok());
            assert!(validator.regex_cache.len() <= ValidationService::MAX_REGEX_CACHE_ENTRIES);
        }
    }

    #[test]
    #[cfg(feature = "schema-validation")]
    fn test_valid_pattern_still_matches_after_cache_flush() {
        let validator = ValidationService::new();
        let schema = Schema::String {
            min_length: None,
            max_length: None,
            pattern: Some("^[a-z]+$".to_string()),
            allowed_values: None,
        };

        assert!(
            validator
                .validate(&JsonData::String("hello".to_string()), &schema, "/name")
                .is_ok()
        );

        // Force a flush, then re-validate the same pattern: it must be
        // recompiled transparently and still match.
        validator.regex_cache.clear();

        assert!(
            validator
                .validate(&JsonData::String("world".to_string()), &schema, "/name")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("123".to_string()), &schema, "/name")
                .is_err()
        );
    }

    #[test]
    #[cfg(feature = "schema-validation")]
    fn test_realistic_pattern_corpus_compiles_under_configured_limits() {
        // Regression guard for the `REGEX_SIZE_LIMIT` DoS-hardening
        // trade-off: nothing else in this suite compiles a non-trivial
        // schema `pattern`, so a size limit tight enough to reject
        // ordinary, non-pathological patterns (e.g. a bare ISO-8601
        // timestamp, which needs ~128 KiB) could regress silently. Each
        // entry pairs a realistic pattern with a value that must match it
        // under the service's default configuration.
        let validator = ValidationService::new();
        let corpus: &[(&str, &str, &str)] = &[
            (
                "email",
                r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$",
                "user@example.com",
            ),
            (
                "uuid",
                r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
                "550e8400-e29b-41d4-a716-446655440000",
            ),
            (
                "iso8601",
                r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$",
                "2024-01-15T12:00:00Z",
            ),
            ("date", r"^\d{4}-\d{2}-\d{2}$", "2024-01-15"),
            ("ipv4", r"^(\d{1,3}\.){3}\d{1,3}$", "192.168.1.1"),
            ("hex-color", r"^#[0-9a-fA-F]{6}$", "#1a2b3c"),
            ("slug", r"^[a-z0-9-]{1,64}$", "hello-world-123"),
            (
                "semver",
                r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*)?$",
                "1.2.3-beta",
            ),
            (
                "jwt-ish",
                r"^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$",
                "abc123.def456.ghi789",
            ),
            // Measured at exactly 1 MiB (regex 1.13.1): pins
            // `REGEX_SIZE_LIMIT` precisely, so this test is a tripwire for
            // *any* future reduction, not just one below ~128 KiB (the
            // tightest of the patterns above, `iso8601`).
            ("any-char-1000", r"^[\s\S]{1,1000}$", "x"),
        ];

        for (name, pattern, value) in corpus {
            let schema = Schema::String {
                min_length: None,
                max_length: None,
                pattern: Some((*pattern).to_string()),
                allowed_values: None,
            };
            let result =
                validator.validate(&JsonData::String((*value).to_string()), &schema, "/value");
            assert!(
                result.is_ok(),
                "pattern {name:?} ({pattern}) should compile and match {value:?} under the \
                 configured REGEX_SIZE_LIMIT, got: {result:?}"
            );
        }
    }
}
