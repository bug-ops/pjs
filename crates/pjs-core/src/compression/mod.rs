//! Schema-based compression for PJS protocol
//!
//! Implements intelligent compression strategies based on JSON schema analysis
//! to optimize bandwidth usage while maintaining streaming capabilities.

pub mod secure;

use crate::domain::{DomainError, DomainResult};
use serde_json::{Value as JsonValue, json};
use std::collections::HashMap;

/// Configuration constants for compression algorithms
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Minimum array length for pattern analysis
    pub min_array_length: usize,
    /// Minimum string length for dictionary inclusion
    pub min_string_length: usize,
    /// Minimum frequency for dictionary inclusion
    pub min_frequency_count: u32,
    /// Minimum compression potential for UUID patterns
    pub uuid_compression_potential: f32,
    /// Threshold score for string dictionary compression
    pub string_dict_threshold: f32,
    /// Threshold score for delta compression
    pub delta_threshold: f32,
    /// Minimum delta potential for numeric compression
    pub min_delta_potential: f32,
    /// Threshold for run-length compression
    pub run_length_threshold: f32,
    /// Minimum compression potential for pattern selection
    pub min_compression_potential: f32,
    /// Minimum array size for numeric sequence analysis
    pub min_numeric_sequence_size: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            min_array_length: 2,
            min_string_length: 3,
            min_frequency_count: 1,
            uuid_compression_potential: 0.3,
            string_dict_threshold: 50.0,
            delta_threshold: 30.0,
            min_delta_potential: 0.3,
            run_length_threshold: 20.0,
            min_compression_potential: 0.4,
            min_numeric_sequence_size: 3,
        }
    }
}

/// Compression strategy based on schema analysis
#[derive(Debug, Clone, PartialEq)]
pub enum CompressionStrategy {
    /// No compression applied
    None,
    /// Dictionary-based compression for repeating string patterns
    Dictionary { dictionary: HashMap<String, u16> },
    /// Delta encoding for numeric sequences
    Delta { base_values: HashMap<String, f64> },
    /// Run-length encoding for repeated values
    RunLength,
    /// Hybrid approach combining multiple strategies
    Hybrid {
        string_dict: HashMap<String, u16>,
        numeric_deltas: HashMap<String, f64>,
    },
}

/// Schema analyzer for determining optimal compression strategy
#[derive(Debug, Clone)]
pub struct SchemaAnalyzer {
    /// Pattern frequency analysis
    patterns: HashMap<String, PatternInfo>,
    /// Numeric field analysis
    numeric_fields: HashMap<String, NumericStats>,
    /// String repetition analysis
    string_repetitions: HashMap<String, u32>,
    /// Configuration for compression algorithms
    config: CompressionConfig,
}

#[derive(Debug, Clone)]
struct PatternInfo {
    frequency: u32,
    #[allow(dead_code)] // Future: used for compression ratio calculation
    total_size: usize,
    compression_potential: f32,
}

#[derive(Debug, Clone)]
struct NumericStats {
    values: Vec<f64>,
    delta_potential: f32,
    base_value: f64,
}

impl SchemaAnalyzer {
    /// Create new schema analyzer
    pub fn new() -> Self {
        Self {
            patterns: HashMap::new(),
            numeric_fields: HashMap::new(),
            string_repetitions: HashMap::new(),
            config: CompressionConfig::default(),
        }
    }

    /// Create new schema analyzer with custom configuration
    pub fn with_config(config: CompressionConfig) -> Self {
        Self {
            patterns: HashMap::new(),
            numeric_fields: HashMap::new(),
            string_repetitions: HashMap::new(),
            config,
        }
    }

    /// Analyze JSON data to determine optimal compression strategy
    pub fn analyze(&mut self, data: &JsonValue) -> DomainResult<CompressionStrategy> {
        // Reset analysis state
        self.patterns.clear();
        self.numeric_fields.clear();
        self.string_repetitions.clear();

        // Perform deep analysis
        self.analyze_recursive(data, "")?;

        // Determine best strategy based on analysis
        self.determine_strategy()
    }

    /// Analyze data recursively
    fn analyze_recursive(&mut self, value: &JsonValue, path: &str) -> DomainResult<()> {
        match value {
            JsonValue::Object(obj) => {
                for (key, val) in obj {
                    let field_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    self.analyze_recursive(val, &field_path)?;
                }
            }
            JsonValue::Array(arr) => {
                // Analyze array patterns
                if arr.len() > self.config.min_array_length {
                    self.analyze_array_patterns(arr, path)?;
                }
                for (idx, item) in arr.iter().enumerate() {
                    let item_path = format!("{path}[{idx}]");
                    self.analyze_recursive(item, &item_path)?;
                }
            }
            JsonValue::String(s) => {
                self.analyze_string_pattern(s, path);
            }
            JsonValue::Number(n) => {
                if let Some(f) = n.as_f64() {
                    self.analyze_numeric_pattern(f, path);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Analyze array for repeating patterns
    fn analyze_array_patterns(&mut self, arr: &[JsonValue], path: &str) -> DomainResult<()> {
        // Check for repeating object structures
        if let Some(JsonValue::Object(first)) = arr.first() {
            let structure_key = format!("array_structure:{path}");
            let field_names: Vec<&str> = first.keys().map(|k| k.as_str()).collect();
            let pattern = field_names.join(",");

            // Count how many objects share this structure
            let matching_count = arr
                .iter()
                .filter_map(|v| v.as_object())
                .filter(|obj| {
                    let obj_fields: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                    obj_fields.join(",") == pattern
                })
                .count();

            if matching_count > self.config.min_frequency_count as usize {
                let info = PatternInfo {
                    frequency: matching_count as u32,
                    total_size: pattern.len() * matching_count,
                    compression_potential: (matching_count as f32 - 1.0) / matching_count as f32,
                };
                self.patterns.insert(structure_key, info);
            }
        }

        // Check for repeating primitive values
        if arr.len() > 2 {
            let mut value_counts = HashMap::new();
            for value in arr {
                let key = match value {
                    JsonValue::String(s) => format!("string:{s}"),
                    JsonValue::Number(n) => format!("number:{n}"),
                    JsonValue::Bool(b) => format!("bool:{b}"),
                    _ => continue,
                };
                *value_counts.entry(key).or_insert(0) += 1;
            }

            for (value_key, count) in value_counts {
                if count > self.config.min_frequency_count {
                    let info = PatternInfo {
                        frequency: count,
                        total_size: value_key.len() * count as usize,
                        compression_potential: (count as f32 - 1.0) / count as f32,
                    };
                    self.patterns
                        .insert(format!("array_value:{path}:{value_key}"), info);
                }
            }
        }

        Ok(())
    }

    /// Analyze string for repetition patterns
    fn analyze_string_pattern(&mut self, s: &str, _path: &str) {
        // Track string repetitions across different paths
        *self.string_repetitions.entry(s.to_string()).or_insert(0) += 1;

        // Analyze common prefixes/suffixes for URLs, IDs, etc.
        if s.len() > 10 {
            // Check for URL patterns
            if s.starts_with("http://") || s.starts_with("https://") {
                let prefix = if s.starts_with("https://") {
                    "https://"
                } else {
                    "http://"
                };
                self.patterns
                    .entry(format!("url_prefix:{prefix}"))
                    .or_insert(PatternInfo {
                        frequency: 0,
                        total_size: 0,
                        compression_potential: 0.0,
                    })
                    .frequency += 1;
            }

            // Check for ID patterns (UUID-like)
            if s.len() == 36 && s.chars().filter(|&c| c == '-').count() == 4 {
                self.patterns
                    .entry("uuid_pattern".to_string())
                    .or_insert(PatternInfo {
                        frequency: 0,
                        total_size: 36,
                        compression_potential: self.config.uuid_compression_potential,
                    })
                    .frequency += 1;
            }
        }
    }

    /// Analyze numeric patterns for delta compression
    fn analyze_numeric_pattern(&mut self, value: f64, path: &str) {
        self.numeric_fields
            .entry(path.to_string())
            .or_insert_with(|| NumericStats {
                values: Vec::new(),
                delta_potential: 0.0,
                base_value: value,
            })
            .values
            .push(value);
    }

    /// Determine optimal compression strategy based on analysis
    fn determine_strategy(&mut self) -> DomainResult<CompressionStrategy> {
        // Calculate compression potentials
        let mut string_dict_score = 0.0;
        let mut delta_score = 0.0;

        // Analyze string repetition potential
        let mut string_dict = HashMap::new();
        let mut dict_index = 0u16;

        for (string, count) in &self.string_repetitions {
            if *count > self.config.min_frequency_count
                && string.len() > self.config.min_string_length
            {
                string_dict_score += (*count as f32 - 1.0) * string.len() as f32;
                string_dict.insert(string.clone(), dict_index);
                dict_index += 1;
            }
        }

        // Analyze numeric delta potential
        let mut numeric_deltas = HashMap::new();

        for (path, stats) in &mut self.numeric_fields {
            if stats.values.len() > 2 {
                // Calculate variance to determine delta effectiveness
                stats
                    .values
                    .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

                let deltas: Vec<f64> = stats
                    .values
                    .windows(2)
                    .map(|window| window[1] - window[0])
                    .collect();

                if !deltas.is_empty() {
                    let avg_delta = deltas.iter().sum::<f64>() / deltas.len() as f64;
                    let delta_variance =
                        deltas.iter().map(|d| (d - avg_delta).powi(2)).sum::<f64>()
                            / deltas.len() as f64;

                    // Low variance suggests good delta compression potential
                    stats.delta_potential = 1.0 / (1.0 + delta_variance as f32);

                    if stats.delta_potential > self.config.min_delta_potential {
                        delta_score += stats.delta_potential * stats.values.len() as f32;
                        numeric_deltas.insert(path.clone(), stats.base_value);
                    }
                }
            }
        }

        // Choose strategy based on scores
        match (
            string_dict_score > self.config.string_dict_threshold,
            delta_score > self.config.delta_threshold,
        ) {
            (true, true) => Ok(CompressionStrategy::Hybrid {
                string_dict,
                numeric_deltas,
            }),
            (true, false) => Ok(CompressionStrategy::Dictionary {
                dictionary: string_dict,
            }),
            (false, true) => Ok(CompressionStrategy::Delta {
                base_values: numeric_deltas,
            }),
            (false, false) => {
                // Check for run-length potential
                let run_length_score = self
                    .patterns
                    .values()
                    .filter(|p| p.compression_potential > self.config.min_compression_potential)
                    .map(|p| p.frequency as f32 * p.compression_potential)
                    .sum::<f32>();

                if run_length_score > self.config.run_length_threshold {
                    Ok(CompressionStrategy::RunLength)
                } else {
                    Ok(CompressionStrategy::None)
                }
            }
        }
    }
}

/// Schema-aware compressor
#[derive(Debug, Clone)]
pub struct SchemaCompressor {
    strategy: CompressionStrategy,
    analyzer: SchemaAnalyzer,
    config: CompressionConfig,
}

impl SchemaCompressor {
    /// Create new compressor with automatic strategy detection
    pub fn new() -> Self {
        let config = CompressionConfig::default();
        Self {
            strategy: CompressionStrategy::None,
            analyzer: SchemaAnalyzer::with_config(config.clone()),
            config,
        }
    }

    /// Create compressor with specific strategy
    pub fn with_strategy(strategy: CompressionStrategy) -> Self {
        let config = CompressionConfig::default();
        Self {
            strategy,
            analyzer: SchemaAnalyzer::with_config(config.clone()),
            config,
        }
    }

    /// Create compressor with custom configuration
    pub fn with_config(config: CompressionConfig) -> Self {
        Self {
            strategy: CompressionStrategy::None,
            analyzer: SchemaAnalyzer::with_config(config.clone()),
            config,
        }
    }

    /// Analyze data and update compression strategy
    pub fn analyze_and_optimize(&mut self, data: &JsonValue) -> DomainResult<&CompressionStrategy> {
        self.strategy = self.analyzer.analyze(data)?;
        Ok(&self.strategy)
    }

    /// Compress JSON data according to current strategy
    pub fn compress(&self, data: &JsonValue) -> DomainResult<CompressedData> {
        match &self.strategy {
            CompressionStrategy::None => Ok(CompressedData {
                strategy: self.strategy.clone(),
                compressed_size: serde_json::to_string(data)
                    .map_err(|e| {
                        DomainError::CompressionError(format!("JSON serialization failed: {e}"))
                    })?
                    .len(),
                data: data.clone(),
                compression_metadata: HashMap::new(),
            }),

            CompressionStrategy::Dictionary { dictionary } => {
                self.compress_with_dictionary(data, dictionary)
            }

            CompressionStrategy::Delta { base_values } => {
                self.compress_with_delta(data, base_values)
            }

            CompressionStrategy::RunLength => self.compress_with_run_length(data),

            CompressionStrategy::Hybrid {
                string_dict,
                numeric_deltas,
            } => self.compress_hybrid(data, string_dict, numeric_deltas),
        }
    }

    /// Dictionary-based compression
    fn compress_with_dictionary(
        &self,
        data: &JsonValue,
        dictionary: &HashMap<String, u16>,
    ) -> DomainResult<CompressedData> {
        let mut metadata = HashMap::new();

        // Store dictionary for decompression
        for (string, index) in dictionary {
            metadata.insert(format!("dict_{index}"), JsonValue::String(string.clone()));
        }

        // Replace strings with dictionary indices
        let compressed = self.replace_strings_with_indices(data, dictionary)?;
        let compressed_size = serde_json::to_string(&compressed)
            .map_err(|e| DomainError::CompressionError(format!("JSON serialization failed: {e}")))?
            .len();

        Ok(CompressedData {
            strategy: self.strategy.clone(),
            compressed_size,
            data: compressed,
            compression_metadata: metadata,
        })
    }

    /// Delta compression for numeric sequences
    fn compress_with_delta(
        &self,
        data: &JsonValue,
        base_values: &HashMap<String, f64>,
    ) -> DomainResult<CompressedData> {
        let mut metadata = HashMap::new();

        // Store base values
        for (path, base) in base_values {
            metadata.insert(
                format!("base_{path}"),
                JsonValue::Number(serde_json::Number::from_f64(*base).unwrap()),
            );
        }

        // Apply delta compression
        let compressed = self.apply_delta_compression(data, base_values)?;
        let compressed_size = serde_json::to_string(&compressed)
            .map_err(|e| DomainError::CompressionError(format!("JSON serialization failed: {e}")))?
            .len();

        Ok(CompressedData {
            strategy: self.strategy.clone(),
            compressed_size,
            data: compressed,
            compression_metadata: metadata,
        })
    }

    /// Run-length encoding compression
    fn compress_with_run_length(&self, data: &JsonValue) -> DomainResult<CompressedData> {
        let compressed = self.apply_run_length_encoding(data)?;
        let compressed_size = serde_json::to_string(&compressed)
            .map_err(|e| DomainError::CompressionError(format!("JSON serialization failed: {e}")))?
            .len();

        Ok(CompressedData {
            strategy: self.strategy.clone(),
            compressed_size,
            data: compressed,
            compression_metadata: HashMap::new(),
        })
    }

    /// Apply run-length encoding to arrays with repeated values
    fn apply_run_length_encoding(&self, data: &JsonValue) -> DomainResult<JsonValue> {
        match data {
            JsonValue::Object(obj) => {
                let mut compressed_obj = serde_json::Map::new();
                for (key, value) in obj {
                    compressed_obj.insert(key.clone(), self.apply_run_length_encoding(value)?);
                }
                Ok(JsonValue::Object(compressed_obj))
            }
            JsonValue::Array(arr) if arr.len() > 2 => {
                // Apply run-length encoding to array
                let mut compressed_runs = Vec::new();
                let mut current_value = None;
                let mut run_count = 0;

                for item in arr {
                    if Some(item) == current_value.as_ref() {
                        run_count += 1;
                    } else {
                        // Save previous run if it exists
                        if let Some(value) = current_value {
                            if run_count > self.config.min_frequency_count {
                                // Use run-length encoding: [value, count]
                                compressed_runs.push(json!({
                                    "rle_value": value,
                                    "rle_count": run_count
                                }));
                            } else {
                                // Single occurrence, keep as-is
                                compressed_runs.push(value);
                            }
                        }

                        // Start new run
                        current_value = Some(item.clone());
                        run_count = 1;
                    }
                }

                // Handle final run
                if let Some(value) = current_value {
                    if run_count > self.config.min_frequency_count {
                        compressed_runs.push(json!({
                            "rle_value": value,
                            "rle_count": run_count
                        }));
                    } else {
                        compressed_runs.push(value);
                    }
                }

                Ok(JsonValue::Array(compressed_runs))
            }
            JsonValue::Array(arr) => {
                // Array too small for run-length encoding, process recursively
                let compressed_arr: Result<Vec<_>, _> = arr
                    .iter()
                    .map(|item| self.apply_run_length_encoding(item))
                    .collect();
                Ok(JsonValue::Array(compressed_arr?))
            }
            _ => Ok(data.clone()),
        }
    }

    /// Hybrid compression combining multiple strategies
    fn compress_hybrid(
        &self,
        data: &JsonValue,
        string_dict: &HashMap<String, u16>,
        numeric_deltas: &HashMap<String, f64>,
    ) -> DomainResult<CompressedData> {
        let mut metadata = HashMap::new();

        // Add dictionary metadata
        for (string, index) in string_dict {
            metadata.insert(format!("dict_{index}"), JsonValue::String(string.clone()));
        }

        // Add delta base values
        for (path, base) in numeric_deltas {
            metadata.insert(
                format!("base_{path}"),
                JsonValue::Number(serde_json::Number::from_f64(*base).unwrap()),
            );
        }

        // Apply both compression strategies
        let dict_compressed = self.replace_strings_with_indices(data, string_dict)?;
        let final_compressed = self.apply_delta_compression(&dict_compressed, numeric_deltas)?;

        let compressed_size = serde_json::to_string(&final_compressed)
            .map_err(|e| DomainError::CompressionError(format!("JSON serialization failed: {e}")))?
            .len();

        Ok(CompressedData {
            strategy: self.strategy.clone(),
            compressed_size,
            data: final_compressed,
            compression_metadata: metadata,
        })
    }

    /// Replace strings with dictionary indices
    #[allow(clippy::only_used_in_recursion)]
    fn replace_strings_with_indices(
        &self,
        data: &JsonValue,
        dictionary: &HashMap<String, u16>,
    ) -> DomainResult<JsonValue> {
        match data {
            JsonValue::Object(obj) => {
                let mut compressed_obj = serde_json::Map::new();
                for (key, value) in obj {
                    compressed_obj.insert(
                        key.clone(),
                        self.replace_strings_with_indices(value, dictionary)?,
                    );
                }
                Ok(JsonValue::Object(compressed_obj))
            }
            JsonValue::Array(arr) => {
                let compressed_arr: Result<Vec<_>, _> = arr
                    .iter()
                    .map(|item| self.replace_strings_with_indices(item, dictionary))
                    .collect();
                Ok(JsonValue::Array(compressed_arr?))
            }
            JsonValue::String(s) => {
                if let Some(&index) = dictionary.get(s) {
                    Ok(JsonValue::Number(serde_json::Number::from(index)))
                } else {
                    Ok(data.clone())
                }
            }
            _ => Ok(data.clone()),
        }
    }

    /// Apply delta compression to numeric sequences in arrays
    fn apply_delta_compression(
        &self,
        data: &JsonValue,
        base_values: &HashMap<String, f64>,
    ) -> DomainResult<JsonValue> {
        self.apply_delta_recursive(data, "", base_values)
    }

    /// Recursively apply delta compression to JSON structure
    fn apply_delta_recursive(
        &self,
        data: &JsonValue,
        path: &str,
        base_values: &HashMap<String, f64>,
    ) -> DomainResult<JsonValue> {
        match data {
            JsonValue::Object(obj) => {
                let mut compressed_obj = serde_json::Map::new();
                for (key, value) in obj {
                    let field_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    compressed_obj.insert(
                        key.clone(),
                        self.apply_delta_recursive(value, &field_path, base_values)?,
                    );
                }
                Ok(JsonValue::Object(compressed_obj))
            }
            JsonValue::Array(arr) if arr.len() > 2 => {
                // Check if this array contains numeric sequences that can be delta-compressed
                if self.is_numeric_sequence(arr) {
                    self.compress_numeric_array_with_delta(arr, path, base_values)
                } else {
                    // Process array elements recursively
                    let compressed_arr: Result<Vec<_>, _> = arr
                        .iter()
                        .enumerate()
                        .map(|(idx, item)| {
                            let item_path = format!("{path}[{idx}]");
                            self.apply_delta_recursive(item, &item_path, base_values)
                        })
                        .collect();
                    Ok(JsonValue::Array(compressed_arr?))
                }
            }
            JsonValue::Array(arr) => {
                // Array too small for delta compression, process recursively
                let compressed_arr: Result<Vec<_>, _> = arr
                    .iter()
                    .enumerate()
                    .map(|(idx, item)| {
                        let item_path = format!("{path}[{idx}]");
                        self.apply_delta_recursive(item, &item_path, base_values)
                    })
                    .collect();
                Ok(JsonValue::Array(compressed_arr?))
            }
            _ => Ok(data.clone()),
        }
    }

    /// Check if array contains a numeric sequence suitable for delta compression
    fn is_numeric_sequence(&self, arr: &[JsonValue]) -> bool {
        if arr.len() < self.config.min_numeric_sequence_size {
            return false;
        }

        // Check if all elements are numbers
        arr.iter().all(|v| v.is_number())
    }

    /// Apply delta compression to numeric array
    fn compress_numeric_array_with_delta(
        &self,
        arr: &[JsonValue],
        path: &str,
        base_values: &HashMap<String, f64>,
    ) -> DomainResult<JsonValue> {
        let mut compressed_array = Vec::new();

        // Extract numeric values
        let numbers: Vec<f64> = arr.iter().filter_map(|v| v.as_f64()).collect();

        if numbers.is_empty() {
            return Ok(JsonValue::Array(arr.to_vec()));
        }

        // Use base value from analysis or first element as base
        let base_value = base_values.get(path).copied().unwrap_or(numbers[0]);

        // Add metadata for base value
        compressed_array.push(json!({
            "delta_base": base_value,
            "delta_type": "numeric_sequence"
        }));

        // Calculate deltas from base value
        let deltas: Vec<f64> = numbers.iter().map(|&num| num - base_value).collect();

        // Check if delta compression is beneficial
        let original_precision = numbers.iter().map(|n| format!("{n}").len()).sum::<usize>();

        let delta_precision = deltas.iter().map(|d| format!("{d}").len()).sum::<usize>();

        if delta_precision < original_precision {
            // Delta compression is beneficial
            compressed_array.extend(deltas.into_iter().map(JsonValue::from));
        } else {
            // Keep original values
            return Ok(JsonValue::Array(arr.to_vec()));
        }

        Ok(JsonValue::Array(compressed_array))
    }
}

/// Compressed data with metadata
#[derive(Debug, Clone)]
pub struct CompressedData {
    pub strategy: CompressionStrategy,
    pub compressed_size: usize,
    pub data: JsonValue,
    pub compression_metadata: HashMap<String, JsonValue>,
}

impl CompressedData {
    /// Calculate compression ratio
    pub fn compression_ratio(&self, original_size: usize) -> f32 {
        if original_size == 0 {
            return 1.0;
        }
        self.compressed_size as f32 / original_size as f32
    }

    /// Get compression savings in bytes
    pub fn compression_savings(&self, original_size: usize) -> isize {
        original_size as isize - self.compressed_size as isize
    }
}

impl Default for SchemaAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SchemaCompressor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_schema_analyzer_dictionary_potential() {
        let mut analyzer = SchemaAnalyzer::new();

        let data = json!({
            "users": [
                {"name": "John Doe", "role": "admin", "status": "active", "department": "engineering"},
                {"name": "Jane Smith", "role": "admin", "status": "active", "department": "engineering"},
                {"name": "Bob Wilson", "role": "admin", "status": "active", "department": "engineering"},
                {"name": "Alice Brown", "role": "admin", "status": "active", "department": "engineering"},
                {"name": "Charlie Davis", "role": "admin", "status": "active", "department": "engineering"},
                {"name": "Diana Evans", "role": "admin", "status": "active", "department": "engineering"},
                {"name": "Frank Miller", "role": "admin", "status": "active", "department": "engineering"},
                {"name": "Grace Wilson", "role": "admin", "status": "active", "department": "engineering"}
            ]
        });

        let strategy = analyzer.analyze(&data).unwrap();

        // Should detect repeating strings like "admin", "active"
        match strategy {
            CompressionStrategy::Dictionary { .. } | CompressionStrategy::Hybrid { .. } => {
                // Expected outcome
            }
            _ => panic!("Expected dictionary-based compression strategy"),
        }
    }

    #[test]
    fn test_schema_compressor_basic() {
        let compressor = SchemaCompressor::new();

        let data = json!({
            "message": "hello world",
            "count": 42
        });

        let original_size = serde_json::to_string(&data).unwrap().len();
        let compressed = compressor.compress(&data).unwrap();

        assert!(compressed.compressed_size > 0);
        assert!(compressed.compression_ratio(original_size) <= 1.0);
    }

    #[test]
    fn test_dictionary_compression() {
        let mut dictionary = HashMap::new();
        dictionary.insert("active".to_string(), 0);
        dictionary.insert("admin".to_string(), 1);

        let compressor =
            SchemaCompressor::with_strategy(CompressionStrategy::Dictionary { dictionary });

        let data = json!({
            "status": "active",
            "role": "admin",
            "description": "active admin user"
        });

        let result = compressor.compress(&data).unwrap();

        // Verify compression metadata contains dictionary
        assert!(result.compression_metadata.contains_key("dict_0"));
        assert!(result.compression_metadata.contains_key("dict_1"));
    }

    #[test]
    fn test_compression_strategy_selection() {
        let mut analyzer = SchemaAnalyzer::new();

        // Test data with no clear patterns
        let simple_data = json!({
            "unique_field_1": "unique_value_1",
            "unique_field_2": "unique_value_2"
        });

        let strategy = analyzer.analyze(&simple_data).unwrap();
        assert_eq!(strategy, CompressionStrategy::None);
    }

    #[test]
    fn test_numeric_delta_analysis() {
        let mut analyzer = SchemaAnalyzer::new();

        let data = json!({
            "measurements": [
                {"time": 100, "value": 10.0},
                {"time": 101, "value": 10.5},
                {"time": 102, "value": 11.0},
                {"time": 103, "value": 11.5}
            ]
        });

        let _strategy = analyzer.analyze(&data).unwrap();

        // Should detect incremental numeric patterns
        assert!(!analyzer.numeric_fields.is_empty());
    }

    #[test]
    fn test_run_length_encoding() {
        let compressor = SchemaCompressor::with_strategy(CompressionStrategy::RunLength);

        let data = json!({
            "repeated_values": [1, 1, 1, 2, 2, 3, 3, 3, 3]
        });

        let result = compressor.compress(&data).unwrap();

        // Should compress repeated sequences
        assert!(result.compressed_size > 0);

        // Verify RLE format in the compressed data
        let compressed_array = &result.data["repeated_values"];
        assert!(compressed_array.is_array());

        // Should contain RLE objects
        let array = compressed_array.as_array().unwrap();
        let has_rle = array.iter().any(|v| v.get("rle_value").is_some());
        assert!(has_rle);
    }

    #[test]
    fn test_delta_compression() {
        let mut base_values = HashMap::new();
        base_values.insert("sequence".to_string(), 100.0);

        let compressor =
            SchemaCompressor::with_strategy(CompressionStrategy::Delta { base_values });

        let data = json!({
            "sequence": [100.0, 101.0, 102.0, 103.0, 104.0]
        });

        let result = compressor.compress(&data).unwrap();

        // Should apply delta compression
        assert!(result.compressed_size > 0);

        // Verify delta format in the compressed data
        let compressed_array = &result.data["sequence"];
        assert!(compressed_array.is_array());

        // Should contain delta metadata
        let array = compressed_array.as_array().unwrap();
        let has_delta_base = array.iter().any(|v| v.get("delta_base").is_some());
        assert!(has_delta_base);
    }
}
