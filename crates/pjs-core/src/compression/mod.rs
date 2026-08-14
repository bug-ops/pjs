//! Schema-based compression for PJS protocol
//!
//! Implements intelligent compression strategies based on JSON schema analysis
//! to optimize bandwidth usage while maintaining streaming capabilities.

pub mod secure;

#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
pub mod zstd;

use crate::config::ConfigError;
use crate::domain::{DomainError, DomainResult};
use serde_json::{Value as JsonValue, json};
use std::collections::HashMap;

/// Sentinel byte (ASCII DEL, `\u{7F}`) marking a dictionary-substituted string.
///
/// Serializes as exactly one raw byte in JSON text (no escaping required per
/// RFC 8259) and effectively never leads real-world text data, which is why
/// it was chosen over a structural wrapper: it keeps a substitution
/// self-describing in the string itself, with no positional metadata needed
/// to reverse it (see issue #333).
pub(crate) const DICT_SENTINEL: char = '\u{7F}';

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
    /// Minimum net wire-byte saving required to select
    /// [`CompressionStrategy::Dictionary`] (or the dictionary half of
    /// [`CompressionStrategy::Hybrid`]). The net saving is computed per
    /// candidate string as `gain - cost`, summed across all kept entries and
    /// reduced by a fixed metadata envelope, using the same size accounting
    /// the reported `compressed_size` uses. This makes dictionary selection a
    /// *modelled* net-positive decision, not a guarantee: see the known
    /// imprecisions below, which can make an accepted payload net-negative on
    /// adversarial input. The wire-byte *report* (`compressed_size` and
    /// everything derived from it) is unaffected and always measured, never
    /// modelled (see issue #333).
    ///
    /// For a string of length `L` repeated `c` times with per-occurrence
    /// marker overhead `m = 1 + decimal_digits(index)` and per-entry
    /// dictionary-array cost `L + 3` (the string, its quotes, one
    /// separator), an entry only pays off once `c*(L-m) > L+3`, i.e.
    /// `L > (c*m + 3) / (c - 1)`. The smallest achievable `m` is `2`
    /// (index `0` is one decimal digit), so for `c = 2` that means
    /// `L > 7`. `"active"` (`L = 6, c = 2`) fails this per-entry gate
    /// (gain `8` < cost `9`) and is pruned before `min_net_savings` is
    /// consulted; a payload whose only repeated string is `"active"`
    /// yields an empty dictionary and [`CompressionStrategy::None`]. Even
    /// one kept entry rarely clears the floor alone: at `c = 2` it needs
    /// `L >= 27` to reach the default `min_net_savings` of `10` after the
    /// envelope.
    ///
    /// Known imprecisions, both of which shift the modelled net toward being
    /// optimistic (a real payload can save fewer bytes than modelled, never
    /// more): this accounting does not model JSON string escaping inside
    /// dictionary strings (e.g. embedded quotes or backslashes), and it does
    /// not model the 1-byte-per-instance cost of escaping a payload string
    /// that legitimately starts with the sentinel byte (see
    /// `substitute_dictionary_strings`). Both are symmetric across ordinary
    /// payloads and shift the byte count only slightly, but a payload
    /// engineered to maximize sentinel-led strings can make a selected
    /// `Dictionary` strategy net-negative in the real, measured report.
    pub min_net_savings: usize,
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
            min_net_savings: 10,
            delta_threshold: 30.0,
            min_delta_potential: 0.3,
            run_length_threshold: 20.0,
            min_compression_potential: 0.4,
            min_numeric_sequence_size: 3,
        }
    }
}

impl CompressionConfig {
    /// Validate compression configuration invariants.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::InconsistentBounds`] when a potential/ratio
    /// field (`uuid_compression_potential`, `min_delta_potential`,
    /// `min_compression_potential`) is outside `0.0..=1.0`, or when a
    /// threshold field (`delta_threshold`, `run_length_threshold`) is
    /// negative or non-finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::compression::CompressionConfig;
    ///
    /// CompressionConfig::default().validate().expect("defaults are valid");
    /// ```
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (value, message) in [
            (
                self.uuid_compression_potential,
                "uuid_compression_potential must be in 0.0..=1.0",
            ),
            (
                self.min_delta_potential,
                "min_delta_potential must be in 0.0..=1.0",
            ),
            (
                self.min_compression_potential,
                "min_compression_potential must be in 0.0..=1.0",
            ),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(ConfigError::InconsistentBounds {
                    section: "compression",
                    message,
                });
            }
        }

        for (value, message) in [
            (
                self.delta_threshold,
                "delta_threshold must be finite and non-negative",
            ),
            (
                self.run_length_threshold,
                "run_length_threshold must be finite and non-negative",
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(ConfigError::InconsistentBounds {
                    section: "compression",
                    message,
                });
            }
        }

        Ok(())
    }
}

/// Compression strategy based on schema analysis
#[derive(Debug, Clone, PartialEq)]
pub enum CompressionStrategy {
    /// No compression applied
    None,
    /// Dictionary-based compression for repeating string patterns
    Dictionary {
        /// Mapping from frequent string to assigned dictionary index.
        dictionary: HashMap<String, u16>,
    },
    /// Delta encoding for numeric sequences
    Delta {
        /// Per-field base value subtracted before delta encoding.
        base_values: HashMap<String, f64>,
    },
    /// Run-length encoding for repeated values
    RunLength,
    /// Hybrid approach combining multiple strategies
    Hybrid {
        /// Dictionary used for the string-replacement pass.
        string_dict: HashMap<String, u16>,
        /// Per-field base values used for the delta-encoding pass.
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
        let mut delta_score = 0.0;

        // Build the dictionary against a real net wire-byte savings model instead of a
        // proxy ratio/floor pair (see issue #333).
        let (string_dict, dict_net_savings) =
            build_dictionary(&self.string_repetitions, &self.config);
        let string_dict_selected =
            !string_dict.is_empty() && dict_net_savings >= self.config.min_net_savings as i64;

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
            string_dict_selected,
            delta_score >= self.config.delta_threshold,
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

                if run_length_score >= self.config.run_length_threshold {
                    Ok(CompressionStrategy::RunLength)
                } else {
                    Ok(CompressionStrategy::None)
                }
            }
        }
    }
}

/// Number of base-10 digits in `n`'s decimal representation (`0` has 1 digit).
fn decimal_digits(n: u16) -> usize {
    n.to_string().len()
}

/// Build the pruned dictionary and its modelled net wire-byte saving for a set of candidate
/// string repetitions, using the same cost model [`wire_size`] measures on the wire.
///
/// Candidates are sorted descending by `count * len` (the strings with the largest raw payoff
/// get the smallest indices, which minimizes marker overhead where it matters most) with a
/// lexicographic tie-break on the string itself, so dictionary construction is fully
/// deterministic across runs on identical input (see issue #333 M6).
///
/// An entry is kept only when its modelled `gain` (bytes saved by replacing every occurrence
/// with a sentinel marker) exceeds its modelled `cost` (the string's own transmission cost in
/// the `"dict"` metadata array). The returned net saving sums `gain - cost` across all kept
/// entries and subtracts the fixed `"dict":[]` envelope once, if any entry was kept.
fn build_dictionary(
    repetitions: &HashMap<String, u32>,
    config: &CompressionConfig,
) -> (HashMap<String, u16>, i64) {
    let mut candidates: Vec<(&String, u32)> = repetitions
        .iter()
        .filter_map(|(s, &count)| {
            (count > config.min_frequency_count && s.len() > config.min_string_length)
                .then_some((s, count))
        })
        .collect();
    candidates.sort_by(|(s1, c1), (s2, c2)| {
        let payoff1 = *c1 as usize * s1.len();
        let payoff2 = *c2 as usize * s2.len();
        payoff2.cmp(&payoff1).then_with(|| s1.cmp(s2))
    });

    let mut dictionary = HashMap::new();
    let mut net: i64 = 0;
    let mut index: u16 = 0;
    for (s, count) in candidates {
        // The dictionary index is a u16: once the index space (0..u16::MAX) is exhausted,
        // stop dictionarying further candidates instead of overflowing on the increment
        // below (issue #333 C3 — a debug panic, or on release a silent index wraparound
        // that collapses two entries into the same "dict" array slot and corrupts decode).
        if index == u16::MAX {
            break;
        }
        let marker_len = 1 + decimal_digits(index);
        let gain = count as i64 * (s.len() as i64 - marker_len as i64);
        let cost = s.len() as i64 + 3;
        if gain > cost {
            net += gain - cost;
            dictionary.insert(s.clone(), index);
            index += 1;
        }
    }
    if !dictionary.is_empty() {
        net -= 10; // `{"dict":[]}` envelope
    }
    (dictionary, net)
}

/// Compute the total wire-transmitted size of a compressed payload: the serialized `data` plus
/// its side-channel `metadata`, when present.
///
/// `CompressedData::compressed_size` and everything derived from it
/// (`compression_ratio`, `compression_savings`,
/// [`crate::stream::compression_integration::CompressionStats::bytes_saved`]) are always
/// measured this way — the report is never a model estimate, so it can never claim a false
/// saving even when the *selection* model used elsewhere (see [`build_dictionary`]) is wrong.
fn wire_size(data: &JsonValue, metadata: &HashMap<String, JsonValue>) -> DomainResult<usize> {
    let mut size = serde_json::to_string(data)
        .map_err(|e| DomainError::CompressionError(format!("JSON serialization failed: {e}")))?
        .len();
    if !metadata.is_empty() {
        size += serde_json::to_string(metadata)
            .map_err(|e| DomainError::CompressionError(format!("JSON serialization failed: {e}")))?
            .len();
    }
    Ok(size)
}

/// Build the wire-format dictionary metadata: an index-ordered JSON array of strings, where
/// array position `i` holds the string assigned dictionary index `i`.
///
/// Any index not present in `dictionary` (a caller contract violation — indices are expected to
/// be exactly `0..dictionary.len()`) is degraded to an empty string slot rather than panicking.
fn dictionary_metadata(dictionary: &HashMap<String, u16>) -> JsonValue {
    let mut ordered: Vec<Option<&str>> = vec![None; dictionary.len()];
    for (s, &i) in dictionary {
        if let Some(slot) = ordered.get_mut(i as usize) {
            *slot = Some(s.as_str());
        }
    }
    JsonValue::Array(
        ordered
            .into_iter()
            .map(|s| JsonValue::String(s.unwrap_or_default().to_string()))
            .collect(),
    )
}

/// Encode dictionary substitutions as sentinel-escaped string markers.
///
/// Per string value `s`:
/// - if `s` is a dictionary key, emit `\u{7F}<index>` (a marker)
/// - else if `s` already starts with `\u{7F}`, emit `\u{7F}` + `s` (escape)
/// - else emit `s` unchanged
///
/// This makes a substitution self-describing in the string itself, so decoding needs no
/// positional metadata — closing both the number/index collision (issue #333 C1) and the
/// path-string collision (issue #333 C2) by construction rather than by narrowing.
fn substitute_dictionary_strings(data: &JsonValue, dictionary: &HashMap<String, u16>) -> JsonValue {
    match data {
        JsonValue::Object(obj) => {
            let mut out = serde_json::Map::with_capacity(obj.len());
            for (key, value) in obj {
                out.insert(
                    key.clone(),
                    substitute_dictionary_strings(value, dictionary),
                );
            }
            JsonValue::Object(out)
        }
        JsonValue::Array(arr) => JsonValue::Array(
            arr.iter()
                .map(|v| substitute_dictionary_strings(v, dictionary))
                .collect(),
        ),
        JsonValue::String(s) => {
            if let Some(&index) = dictionary.get(s) {
                JsonValue::String(format!("{DICT_SENTINEL}{index}"))
            } else if s.starts_with(DICT_SENTINEL) {
                JsonValue::String(format!("{DICT_SENTINEL}{s}"))
            } else {
                data.clone()
            }
        }
        _ => data.clone(),
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
            CompressionStrategy::None => {
                let metadata = HashMap::new();
                Ok(CompressedData {
                    strategy: self.strategy.clone(),
                    compressed_size: wire_size(data, &metadata)?,
                    data: data.clone(),
                    compression_metadata: metadata,
                })
            }

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
        metadata.insert("dict".to_string(), dictionary_metadata(dictionary));

        let compressed = substitute_dictionary_strings(data, dictionary);
        let compressed_size = wire_size(&compressed, &metadata)?;

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
            let number = serde_json::Number::from_f64(*base).ok_or_else(|| {
                DomainError::CompressionError(format!(
                    "delta base value for path '{path}' is non-finite (NaN or Infinity); cannot compress"
                ))
            })?;
            metadata.insert(format!("base_{path}"), JsonValue::Number(number));
        }

        // Apply delta compression
        let compressed = self.apply_delta_compression(data, base_values)?;
        let compressed_size = wire_size(&compressed, &metadata)?;

        Ok(CompressedData {
            strategy: self.strategy.clone(),
            compressed_size,
            data: compressed,
            compression_metadata: metadata,
        })
    }

    /// Run-length encoding compression
    fn compress_with_run_length(&self, data: &JsonValue) -> DomainResult<CompressedData> {
        let metadata = HashMap::new();
        let compressed = self.apply_run_length_encoding(data)?;
        let compressed_size = wire_size(&compressed, &metadata)?;

        Ok(CompressedData {
            strategy: self.strategy.clone(),
            compressed_size,
            data: compressed,
            compression_metadata: metadata,
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
        metadata.insert("dict".to_string(), dictionary_metadata(string_dict));

        // Add delta base values
        for (path, base) in numeric_deltas {
            let number = serde_json::Number::from_f64(*base).ok_or_else(|| {
                DomainError::CompressionError(format!(
                    "delta base value for path '{path}' is non-finite (NaN or Infinity); cannot compress"
                ))
            })?;
            metadata.insert(format!("base_{path}"), JsonValue::Number(number));
        }

        // Apply both compression strategies: dictionary substitution first, then delta.
        let dict_compressed = substitute_dictionary_strings(data, string_dict);
        let final_compressed = self.apply_delta_compression(&dict_compressed, numeric_deltas)?;

        let compressed_size = wire_size(&final_compressed, &metadata)?;

        Ok(CompressedData {
            strategy: self.strategy.clone(),
            compressed_size,
            data: final_compressed,
            compression_metadata: metadata,
        })
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
    /// Strategy that produced the compressed payload.
    pub strategy: CompressionStrategy,
    /// Total wire-transmitted size, in bytes: `data` after JSON serialization plus
    /// `compression_metadata` when non-empty. This is always a measured value, never a model
    /// estimate — see the `wire_size` helper in this module.
    pub compressed_size: usize,
    /// JSON payload after compression has been applied.
    pub data: JsonValue,
    /// Side-channel metadata required for decompression (dictionaries, base values, etc.).
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
    fn test_schema_analyzer_realistic_ecommerce_payload() {
        // Regression test for issue #333: a realistic ~423-byte payload with moderate
        // repetition ("Electronics"/"Apple"/"available" x3 each) that nets a genuine positive
        // wire-byte saving under honest `wire_size` accounting, not just a favorable ratio.
        let mut analyzer = SchemaAnalyzer::new();

        let data = json!({
            "products": [
                {"id": 1001, "name": "MacBook Pro", "category": "Electronics", "status": "available", "brand": "Apple", "price": 2399.99},
                {"id": 1002, "name": "iPhone 15", "category": "Electronics", "status": "available", "brand": "Apple", "price": 999.99},
                {"id": 1003, "name": "AirPods Pro", "category": "Electronics", "status": "available", "brand": "Apple", "price": 249.99}
            ],
            "store": {"name": "Tech Store", "status": "operational", "location": "San Francisco"}
        });

        let strategy = analyzer.analyze(&data).unwrap();

        match &strategy {
            CompressionStrategy::Dictionary { .. } | CompressionStrategy::Hybrid { .. } => {}
            other => panic!("Expected dictionary-based compression strategy, got {other:?}"),
        }

        let original_size = serde_json::to_string(&data).unwrap().len();
        let compressed = SchemaCompressor::with_strategy(strategy)
            .compress(&data)
            .unwrap();
        assert!(
            compressed.compression_savings(original_size) > 0,
            "expected genuine positive wire-byte savings, got {}",
            compressed.compression_savings(original_size)
        );
    }

    #[test]
    fn test_schema_analyzer_realistic_api_response_payload() {
        // Regression test for issue #333: a realistic API response with genuine field-level
        // repetition (5 users, "status" x4 and "role" x4) large enough to net a real
        // wire-byte saving once dictionary overhead is honestly accounted for — a smaller,
        // 3-user version of this payload with short enum strings ("active"/"user" x2 each)
        // cannot clear even a 2-byte-per-instance marker overhead and correctly stays `None`.
        let mut analyzer = SchemaAnalyzer::new();

        let data = json!({
            "status": "success",
            "data": {
                "users": [
                    {"id": "user_001", "email": "alice@example.com", "status": "subscription_active", "role": "standard_user", "created_at": "2024-01-01T00:00:00Z", "last_login": "2024-01-15T10:30:00Z"},
                    {"id": "user_002", "email": "bob@example.com", "status": "subscription_active", "role": "standard_user", "created_at": "2024-01-02T00:00:00Z", "last_login": "2024-01-15T09:15:00Z"},
                    {"id": "user_003", "email": "charlie@example.com", "status": "subscription_active", "role": "standard_user", "created_at": "2024-01-03T00:00:00Z", "last_login": "2024-01-10T14:22:00Z"},
                    {"id": "user_004", "email": "dave@example.com", "status": "subscription_active", "role": "administrator", "created_at": "2024-01-04T00:00:00Z", "last_login": "2024-01-14T11:05:00Z"},
                    {"id": "user_005", "email": "erin@example.com", "status": "subscription_inactive", "role": "standard_user", "created_at": "2024-01-05T00:00:00Z", "last_login": "2024-01-09T08:40:00Z"}
                ]
            },
            "pagination": {"page": 1, "per_page": 25, "total_pages": 4, "total_items": 89},
            "meta": {"request_id": "req_12345", "timestamp": "2024-01-15T10:30:15Z", "version": "v1.2.3"}
        });

        let strategy = analyzer.analyze(&data).unwrap();

        match &strategy {
            CompressionStrategy::Dictionary { .. } | CompressionStrategy::Hybrid { .. } => {}
            other => panic!("Expected dictionary-based compression strategy, got {other:?}"),
        }

        let original_size = serde_json::to_string(&data).unwrap().len();
        let compressed = SchemaCompressor::with_strategy(strategy)
            .compress(&data)
            .unwrap();
        assert!(
            compressed.compression_savings(original_size) > 0,
            "expected genuine positive wire-byte savings, got {}",
            compressed.compression_savings(original_size)
        );
    }

    #[test]
    fn test_schema_analyzer_no_repetition_stays_none() {
        // Payloads with no meaningful string repetition must still resolve to
        // `CompressionStrategy::None` after normalizing the threshold — the
        // fix must not zero out the threshold and trigger unconditionally.
        let mut analyzer = SchemaAnalyzer::new();

        let data = json!({
            "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
            "name": "Unique Product Name Alpha",
            "description": "A completely unique description of this particular item with no repeats",
            "vendor": "Acme Corporation International",
            "location": "Building 12, Warehouse Section D",
            "notes": "Handled with care during transit process"
        });

        let strategy = analyzer.analyze(&data).unwrap();
        assert_eq!(strategy, CompressionStrategy::None);
    }

    #[test]
    fn test_schema_analyzer_tiny_duplicate_stays_none_below_savings_floor() {
        // Regression test for issue #333's S3/net-benefit-gate finding: a tiny payload with
        // one duplicated short string ("hello" x2) models a net wire-byte loss once dictionary
        // overhead is accounted for (gain 6 < cost 8), so `build_dictionary` prunes it and
        // `min_net_savings` correctly rejects `Dictionary` for this payload.
        let mut analyzer = SchemaAnalyzer::new();

        let data = json!({"a": "hello", "b": "hello", "c": "world"});

        let strategy = analyzer.analyze(&data).unwrap();
        assert_eq!(strategy, CompressionStrategy::None);
    }

    #[test]
    fn test_schema_analyzer_long_repeated_string_selects_dictionary_and_shrinks() {
        // Net-benefit gate, positive case: a >=12-char string repeated 3 times models a
        // comfortably positive net wire-byte saving (gain 54 - cost 23 - envelope 10 = 21
        // here), clearing the default `min_net_savings` floor of 10.
        let mut analyzer = SchemaAnalyzer::new();

        let data = json!({
            "a": "premium_subscription",
            "b": "premium_subscription",
            "c": "premium_subscription",
            "d": "unique"
        });

        let strategy = analyzer.analyze(&data).unwrap();
        let dictionary = match &strategy {
            CompressionStrategy::Dictionary { dictionary } => dictionary,
            other => panic!("Expected Dictionary strategy, got {other:?}"),
        };

        let original_size = serde_json::to_string(&data).unwrap().len();
        let compressed = SchemaCompressor::with_strategy(CompressionStrategy::Dictionary {
            dictionary: dictionary.clone(),
        })
        .compress(&data)
        .unwrap();
        assert!(compressed.compression_savings(original_size) > 0);
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

        // Verify compression metadata contains the index-ordered dictionary array.
        assert_eq!(
            result.compression_metadata.get("dict"),
            Some(&json!(["active", "admin"]))
        );
    }

    #[test]
    fn test_dictionary_compression_never_produces_numbers_from_substitution() {
        // Regression test for issue #333's C1 finding: a bare dictionary index used to be
        // encoded as a JsonValue::Number, indistinguishable from a genuine payload integer.
        // Sentinel-escaped string markers make this structurally impossible: "count" holds the
        // same raw value (0) that "active"'s dictionary index encodes as, but it's untouched
        // because only JSON strings are ever substitution candidates.
        let mut dictionary = HashMap::new();
        dictionary.insert("active".to_string(), 0);

        let compressor =
            SchemaCompressor::with_strategy(CompressionStrategy::Dictionary { dictionary });

        let data = json!({
            "status": "active",
            "count": 0
        });

        let result = compressor.compress(&data).unwrap();

        assert_eq!(result.data, json!({"status": "\u{7F}0", "count": 0}));
    }

    #[test]
    fn test_dictionary_sentinel_escaping_encode_shape() {
        // Encode-side half of the sentinel-marker injectivity proof: a payload containing
        // strings that legitimately start with the sentinel byte — including one that mimics
        // a real marker's exact shape ("\u{7F}0") — must be escaped with exactly one extra
        // leading sentinel, distinct from a genuine dictionary marker's single sentinel.
        // The full round trip (encode + decode via the public streaming API) is covered by
        // `test_dictionary_sentinel_escaping_round_trips_losslessly` in the integration tests.
        let mut dictionary = HashMap::new();
        dictionary.insert("greeting".to_string(), 0);

        let data = json!({
            "a": "\u{7F}foo",
            "b": "\u{7F}\u{7F}bar",
            "c": "\u{7F}0",
            "d": "greeting"
        });

        let substituted = substitute_dictionary_strings(&data, &dictionary);
        assert_eq!(
            substituted,
            json!({
                "a": "\u{7F}\u{7F}foo",
                "b": "\u{7F}\u{7F}\u{7F}bar",
                "c": "\u{7F}\u{7F}0",
                "d": "\u{7F}0"
            })
        );
    }

    #[test]
    fn test_compressed_size_matches_wire_bytes_for_every_strategy() {
        // Size-honesty regression test for issue #333 S5: `compressed_size` must equal the
        // actual serialized data plus metadata bytes for every strategy, never a model
        // estimate.
        fn expected_wire_size(data: &JsonValue, metadata: &HashMap<String, JsonValue>) -> usize {
            let mut size = serde_json::to_string(data).unwrap().len();
            if !metadata.is_empty() {
                size += serde_json::to_string(metadata).unwrap().len();
            }
            size
        }

        let data = json!({
            "status": "active",
            "count": 3,
            "sequence": [1.0, 2.0, 3.0],
            "repeated": [1, 1, 1, 2, 2]
        });

        let mut dictionary = HashMap::new();
        dictionary.insert("active".to_string(), 0);
        let mut base_values = HashMap::new();
        base_values.insert("sequence".to_string(), 1.0);

        for strategy in [
            CompressionStrategy::None,
            CompressionStrategy::Dictionary {
                dictionary: dictionary.clone(),
            },
            CompressionStrategy::Delta {
                base_values: base_values.clone(),
            },
            CompressionStrategy::RunLength,
            CompressionStrategy::Hybrid {
                string_dict: dictionary.clone(),
                numeric_deltas: base_values.clone(),
            },
        ] {
            let compressor = SchemaCompressor::with_strategy(strategy);
            let result = compressor.compress(&data).unwrap();
            assert_eq!(
                result.compressed_size,
                expected_wire_size(&result.data, &result.compression_metadata),
                "strategy {:?} mismatched wire size",
                result.strategy
            );
        }
    }

    #[test]
    fn test_build_dictionary_caps_index_at_u16_max_without_overflow() {
        // Regression test for issue #333 C3: the dictionary index is a `u16`. Before the fix,
        // more than `u16::MAX` kept entries panicked on the index increment in debug builds
        // and silently wrapped to duplicate indices in release builds, collapsing distinct
        // dictionary entries into the same "dict" array slot and corrupting decode with no
        // error raised. Every candidate below is deliberately long enough (`L = 20`) and
        // repeated enough (`c = 2`) to individually clear the per-entry `gain > cost` gate
        // regardless of the marker length at any index up to `u16::MAX`, so all of them are
        // kept candidates — the count of *kept* entries is what the index bounds, not the
        // count of candidates offered.
        let mut repetitions = HashMap::new();
        for i in 0..(u16::MAX as u32 + 2) {
            repetitions.insert(format!("padding_string_{i:05}"), 2);
        }

        let (dictionary, _net) = build_dictionary(&repetitions, &CompressionConfig::default());

        assert!(
            dictionary.len() <= u16::MAX as usize,
            "dictionary must never exceed the u16 index space, got {} entries",
            dictionary.len()
        );

        let distinct_indices: std::collections::HashSet<u16> =
            dictionary.values().copied().collect();
        assert_eq!(
            distinct_indices.len(),
            dictionary.len(),
            "every dictionary entry must have a unique index — a mismatch here means indices \
             wrapped and collided"
        );
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

    #[test]
    fn test_delta_compression_rejects_nan_base() {
        let mut base_values = HashMap::new();
        base_values.insert("sequence".to_string(), f64::NAN);

        let compressor =
            SchemaCompressor::with_strategy(CompressionStrategy::Delta { base_values });

        let data = json!({ "sequence": [1.0, 2.0, 3.0] });

        let err = compressor
            .compress(&data)
            .expect_err("expected error for NaN base");
        match err {
            DomainError::CompressionError(msg) => {
                assert!(msg.contains("non-finite"), "unexpected message: {msg}");
                assert!(msg.contains("sequence"), "expected path in message: {msg}");
            }
            other => panic!("expected CompressionError, got {other:?}"),
        }
    }

    #[test]
    fn test_delta_compression_rejects_infinity_base() {
        let mut base_values = HashMap::new();
        base_values.insert("sequence".to_string(), f64::INFINITY);

        let compressor =
            SchemaCompressor::with_strategy(CompressionStrategy::Delta { base_values });

        let data = json!({ "sequence": [1.0, 2.0, 3.0] });

        let err = compressor
            .compress(&data)
            .expect_err("expected error for Infinity base");
        assert!(matches!(err, DomainError::CompressionError(_)));
    }

    #[test]
    fn test_hybrid_compression_rejects_nan_base() {
        let string_dict = HashMap::new();
        let mut numeric_deltas = HashMap::new();
        numeric_deltas.insert("sequence".to_string(), f64::NEG_INFINITY);

        let compressor = SchemaCompressor::with_strategy(CompressionStrategy::Hybrid {
            string_dict,
            numeric_deltas,
        });

        let data = json!({ "sequence": [1.0, 2.0, 3.0] });

        let err = compressor
            .compress(&data)
            .expect_err("expected error for non-finite base");
        match err {
            DomainError::CompressionError(msg) => {
                assert!(msg.contains("non-finite"), "unexpected message: {msg}");
            }
            other => panic!("expected CompressionError, got {other:?}"),
        }
    }
}
