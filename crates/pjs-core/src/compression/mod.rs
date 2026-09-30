//! Schema-based compression for PJS protocol
//!
//! Implements intelligent compression strategies based on JSON schema analysis
//! to optimize bandwidth usage while maintaining streaming capabilities.

mod savings;
pub mod secure;

#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
pub mod zstd;

use crate::domain::{DomainError, DomainResult};
use savings::{decimal_digits, delta_plan, rle_net_saving, rle_runs, run_saving};
use serde_json::{Value as JsonValue, json};
use std::collections::HashMap;

/// Maximum count of one run-length encoded run, shared by the encoder and the decoder.
pub(crate) const MAX_RLE_COUNT: usize = 100_000;
/// Maximum length of a delta-encoded array including its header element.
pub(crate) const MAX_DELTA_ARRAY_SIZE: usize = 1_000_000;
/// Maximum number of elements one run-length decoded array may expand to.
pub(crate) const MAX_DECOMPRESSED_ELEMENTS: usize = 10_485_760;

/// Exact integer value of a JSON number, `None` for floats and non-numbers.
pub(crate) fn as_integer(value: &JsonValue) -> Option<i128> {
    let n = value.as_number()?;
    n.as_i64()
        .map(i128::from)
        .or_else(|| n.as_u64().map(i128::from))
}

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
    /// Minimum string length for dictionary inclusion
    pub min_string_length: usize,
    /// Minimum frequency for dictionary inclusion (run-length encoding is gated by its saving)
    pub min_frequency_count: u32,
    /// Minimum net wire-byte saving required to select any strategy other than
    /// [`CompressionStrategy::None`]. For dictionaries the net saving is computed per
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
    /// Minimum array length for delta encoding (at least 3 is always required)
    pub min_numeric_sequence_size: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            min_string_length: 3,
            min_frequency_count: 1,
            min_net_savings: 10,
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
    Dictionary {
        /// Mapping from frequent string to assigned dictionary index.
        dictionary: HashMap<String, u16>,
    },
    /// Lossless delta encoding of integer arrays against their per-array minimum
    Delta,
    /// Run-length encoding for repeated values
    RunLength,
    /// Hybrid approach combining multiple strategies
    Hybrid {
        /// Dictionary used for the string-replacement pass.
        string_dict: HashMap<String, u16>,
    },
}

/// Schema analyzer for determining optimal compression strategy
///
/// Every strategy is scored by the exact number of wire bytes the encoder saves on the analyzed
/// payload; the strategy with the largest saving of at least [`CompressionConfig::min_net_savings`]
/// wins.
#[derive(Debug, Clone)]
pub struct SchemaAnalyzer {
    config: CompressionConfig,
}

/// Aggregates collected in a single walk over the analyzed payload.
struct Analysis<'a> {
    strings: HashMap<&'a str, u32>,
    delta_net: usize,
    rle_net: usize,
}

#[derive(Clone, Copy)]
enum Candidate {
    Dictionary,
    Delta,
    RunLength,
    Hybrid,
}

fn signed(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

impl SchemaAnalyzer {
    /// Create new schema analyzer
    pub fn new() -> Self {
        Self {
            config: CompressionConfig::default(),
        }
    }

    /// Create new schema analyzer with custom configuration
    pub fn with_config(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// Analyze JSON data to determine optimal compression strategy
    pub fn analyze(&self, data: &JsonValue) -> DomainResult<CompressionStrategy> {
        let mut analysis = Analysis {
            strings: HashMap::new(),
            delta_net: 0,
            rle_net: 0,
        };
        self.walk(data, true, &mut analysis);
        Ok(self.determine_strategy(analysis))
    }

    /// `rle_reachable` mirrors which arrays `apply_run_length_encoding` visits.
    fn walk<'a>(&self, value: &'a JsonValue, rle_reachable: bool, out: &mut Analysis<'a>) {
        match value {
            JsonValue::Object(obj) => {
                for child in obj.values() {
                    self.walk(child, rle_reachable, out);
                }
            }
            JsonValue::Array(arr) => {
                if let Some(plan) = delta_plan(arr, &self.config) {
                    out.delta_net += plan.saving;
                }
                let is_run_encoded = arr.len() > 2;
                if rle_reachable && is_run_encoded {
                    out.rle_net += rle_net_saving(arr);
                }
                for item in arr {
                    self.walk(item, rle_reachable && !is_run_encoded, out);
                }
            }
            JsonValue::String(s) => {
                *out.strings.entry(s).or_insert(0) += 1;
            }
            _ => {}
        }
    }

    /// Pick the strategy with the largest modelled saving that clears the floor.
    fn determine_strategy(&self, analysis: Analysis<'_>) -> CompressionStrategy {
        let (string_dict, dict_net) = build_dictionary(&analysis.strings, &self.config);
        let delta_net = signed(analysis.delta_net);
        let rle_net = signed(analysis.rle_net);
        let floor = signed(self.config.min_net_savings);
        let clears_floor = |net: i64| (net > 0 && net >= floor).then_some(net);

        let dict_useful = !string_dict.is_empty() && dict_net > 0;
        let hybrid_net = (dict_useful && delta_net > 0).then(|| dict_net + delta_net);
        let candidates = [
            (
                Candidate::Dictionary,
                dict_useful.then_some(dict_net).and_then(clears_floor),
            ),
            (Candidate::Delta, clears_floor(delta_net)),
            (Candidate::RunLength, clears_floor(rle_net)),
            (Candidate::Hybrid, hybrid_net.and_then(clears_floor)),
        ];

        let mut best: Option<(Candidate, i64)> = None;
        for (candidate, net) in candidates {
            if let Some(net) = net
                && best.is_none_or(|(_, best_net)| net > best_net)
            {
                best = Some((candidate, net));
            }
        }

        match best.map(|(candidate, _)| candidate) {
            None => CompressionStrategy::None,
            Some(Candidate::Dictionary) => CompressionStrategy::Dictionary {
                dictionary: string_dict,
            },
            Some(Candidate::Delta) => CompressionStrategy::Delta,
            Some(Candidate::RunLength) => CompressionStrategy::RunLength,
            Some(Candidate::Hybrid) => CompressionStrategy::Hybrid { string_dict },
        }
    }
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
    repetitions: &HashMap<&str, u32>,
    config: &CompressionConfig,
) -> (HashMap<String, u16>, i64) {
    let mut candidates: Vec<(&str, u32)> = repetitions
        .iter()
        .filter_map(|(&s, &count)| {
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
        let marker_len = 1 + decimal_digits(u64::from(index));
        let gain = count as i64 * (s.len() as i64 - marker_len as i64);
        let cost = s.len() as i64 + 3;
        if gain > cost {
            net += gain - cost;
            dictionary.insert(s.to_string(), index);
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

            CompressionStrategy::Delta => self.compress_with_delta(data),

            CompressionStrategy::RunLength => self.compress_with_run_length(data),

            CompressionStrategy::Hybrid { string_dict } => self.compress_hybrid(data, string_dict),
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

    /// Delta compression for integer arrays
    fn compress_with_delta(&self, data: &JsonValue) -> DomainResult<CompressedData> {
        let metadata = HashMap::new();
        let compressed = self.apply_delta_compression(data);
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
        let compressed = self.apply_run_length_encoding(data);
        let compressed_size = wire_size(&compressed, &metadata)?;

        Ok(CompressedData {
            strategy: self.strategy.clone(),
            compressed_size,
            data: compressed,
            compression_metadata: metadata,
        })
    }

    /// Apply run-length encoding to arrays, emitting only runs that shrink the wire size
    fn apply_run_length_encoding(&self, data: &JsonValue) -> JsonValue {
        match data {
            JsonValue::Object(obj) => JsonValue::Object(
                obj.iter()
                    .map(|(key, value)| (key.clone(), self.apply_run_length_encoding(value)))
                    .collect(),
            ),
            JsonValue::Array(arr) if arr.len() > 2 => {
                let Some(runs) = rle_runs(arr) else {
                    return data.clone();
                };
                let mut encoded = Vec::new();
                for (value, count) in runs {
                    if run_saving(value, count).is_some() {
                        encoded.push(json!({ "rle_value": value, "rle_count": count }));
                    } else {
                        encoded.extend(std::iter::repeat_n(value.clone(), count));
                    }
                }
                JsonValue::Array(encoded)
            }
            JsonValue::Array(arr) => JsonValue::Array(
                arr.iter()
                    .map(|item| self.apply_run_length_encoding(item))
                    .collect(),
            ),
            _ => data.clone(),
        }
    }

    /// Hybrid compression combining multiple strategies
    fn compress_hybrid(
        &self,
        data: &JsonValue,
        string_dict: &HashMap<String, u16>,
    ) -> DomainResult<CompressedData> {
        let mut metadata = HashMap::new();
        metadata.insert("dict".to_string(), dictionary_metadata(string_dict));

        // Dictionary substitution first, then delta; the decoder reverses the order.
        let dict_compressed = substitute_dictionary_strings(data, string_dict);
        let final_compressed = self.apply_delta_compression(&dict_compressed);

        let compressed_size = wire_size(&final_compressed, &metadata)?;

        Ok(CompressedData {
            strategy: self.strategy.clone(),
            compressed_size,
            data: final_compressed,
            compression_metadata: metadata,
        })
    }

    /// Recursively delta-encode every profitable integer array
    fn apply_delta_compression(&self, data: &JsonValue) -> JsonValue {
        match data {
            JsonValue::Object(obj) => JsonValue::Object(
                obj.iter()
                    .map(|(key, value)| (key.clone(), self.apply_delta_compression(value)))
                    .collect(),
            ),
            JsonValue::Array(arr) => {
                if let Some(plan) = delta_plan(arr, &self.config)
                    && let Some(deltas) = arr
                        .iter()
                        .map(|value| plan.delta_of(value).map(JsonValue::from))
                        .collect::<Option<Vec<_>>>()
                {
                    let header = json!({
                        "delta_base": plan.base,
                        "delta_type": "numeric_sequence"
                    });
                    let mut encoded = Vec::with_capacity(deltas.len() + 1);
                    encoded.push(header);
                    encoded.extend(deltas);
                    return JsonValue::Array(encoded);
                }
                JsonValue::Array(
                    arr.iter()
                        .map(|item| self.apply_delta_compression(item))
                        .collect(),
                )
            }
            _ => data.clone(),
        }
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
    /// Side-channel metadata required for decompression (the dictionary).
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
        let analyzer = SchemaAnalyzer::new();

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
        let analyzer = SchemaAnalyzer::new();

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
        let analyzer = SchemaAnalyzer::new();

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
        let analyzer = SchemaAnalyzer::new();

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
        let analyzer = SchemaAnalyzer::new();

        let data = json!({"a": "hello", "b": "hello", "c": "world"});

        let strategy = analyzer.analyze(&data).unwrap();
        assert_eq!(strategy, CompressionStrategy::None);
    }

    #[test]
    fn test_schema_analyzer_long_repeated_string_selects_dictionary_and_shrinks() {
        // Net-benefit gate, positive case: a >=12-char string repeated 3 times models a
        // comfortably positive net wire-byte saving (gain 54 - cost 23 - envelope 10 = 21
        // here), clearing the default `min_net_savings` floor of 10.
        let analyzer = SchemaAnalyzer::new();

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
            "sequence": [1000, 1001, 1002, 1003],
            "repeated": [1, 1, 1, 2, 2]
        });

        let mut dictionary = HashMap::new();
        dictionary.insert("active".to_string(), 0);
        for strategy in [
            CompressionStrategy::None,
            CompressionStrategy::Dictionary {
                dictionary: dictionary.clone(),
            },
            CompressionStrategy::Delta,
            CompressionStrategy::RunLength,
            CompressionStrategy::Hybrid {
                string_dict: dictionary.clone(),
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
        let names: Vec<String> = (0..(u16::MAX as u32 + 2))
            .map(|i| format!("padding_string_{i:05}"))
            .collect();
        let repetitions: HashMap<&str, u32> = names.iter().map(|n| (n.as_str(), 2)).collect();

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
        let analyzer = SchemaAnalyzer::new();

        // Test data with no clear patterns
        let simple_data = json!({
            "unique_field_1": "unique_value_1",
            "unique_field_2": "unique_value_2"
        });

        let strategy = analyzer.analyze(&simple_data).unwrap();
        assert_eq!(strategy, CompressionStrategy::None);
    }

    fn ints(base: u64, n: u64) -> JsonValue {
        JsonValue::Array((0..n).map(|i| json!(base + i)).collect())
    }

    fn analyzed(data: &JsonValue) -> CompressionStrategy {
        SchemaAnalyzer::new().analyze(data).unwrap()
    }

    /// Auto-selects a strategy, compresses through the streaming API and decodes it back.
    fn round_trip(data: &JsonValue) -> (CompressionStrategy, usize) {
        use crate::domain::value_objects::Priority;
        use crate::stream::{
            StreamFrame,
            compression_integration::{StreamingCompressor, StreamingDecompressor},
        };

        let mut compressor = SchemaCompressor::new();
        let strategy = compressor.analyze_and_optimize(data).unwrap().clone();
        let mut streaming =
            StreamingCompressor::with_strategies(strategy.clone(), strategy.clone());
        let frame = StreamFrame {
            data: data.clone(),
            priority: Priority::CRITICAL,
            metadata: HashMap::new(),
        };
        let mut compressed = streaming.compress_frame(frame).unwrap();
        let wire = compressed.compressed_data.compressed_size;
        let text = serde_json::to_string(&compressed.compressed_data.data).unwrap();
        compressed.compressed_data.data = serde_json::from_str(&text).unwrap();
        let decoded = StreamingDecompressor::new()
            .decompress_frame(compressed)
            .unwrap();
        assert_eq!(decoded.data, *data, "round trip mismatch for {strategy:?}");
        assert_eq!(
            decoded.data.to_string(),
            data.to_string(),
            "textual round trip mismatch for {strategy:?}"
        );
        (strategy, wire)
    }

    fn original_len(data: &JsonValue) -> usize {
        serde_json::to_string(data).unwrap().len()
    }

    #[test]
    fn test_round_trip_corpus() {
        use CompressionStrategy as S;
        let long = "premium_subscription_tier";
        let corpus = [
            (ints(1_700_000_000_000, 500), Some(S::Delta)),
            (
                JsonValue::Array(
                    (0..500u64)
                        .map(|i| json!({"t": 1_700_000_000_000u64 + i, "v": i}))
                        .collect(),
                ),
                Some(S::None),
            ),
            (ints(9_007_199_254_740_993, 300), Some(S::Delta)),
            (
                JsonValue::Array((0..300i64).map(|i| json!(i - 1_000_000)).collect()),
                Some(S::Delta),
            ),
            (json!([i64::MIN, u64::MAX, 0]), Some(S::None)),
            (json!({"c": vec![7; 300]}), Some(S::RunLength)),
            (json!(vec![0; 200_000]), Some(S::RunLength)),
            (json!(vec![long; 200]), Some(S::RunLength)),
            (
                JsonValue::Array((0..500).map(|i| json!(f64::from(i) * 0.5)).collect()),
                Some(S::None),
            ),
            (json!([1, "a", 2, "b", 3, "c", 4, "d"]), Some(S::None)),
            (
                JsonValue::Array((0..50).map(|_| json!([1, 2, 3])).collect()),
                Some(S::RunLength),
            ),
            (json!({"a": [1, 2], "b": [1000, 1001, 1002]}), Some(S::None)),
            (
                json!({"seq": ints(5_000_000, 100), "tags": [long, long, long, "x"]}),
                Some(S::Hybrid {
                    string_dict: HashMap::new(),
                }),
            ),
        ];
        for (data, expected) in &corpus {
            let (strategy, _) = round_trip(data);
            if let Some(expected) = expected {
                assert_eq!(
                    std::mem::discriminant(&strategy),
                    std::mem::discriminant(expected),
                    "unexpected strategy {strategy:?}"
                );
            }
        }
    }

    #[test]
    fn test_overflowing_delta_array_stays_raw() {
        let mut items: Vec<JsonValue> = (0..28).map(|i| json!(i)).collect();
        items.extend([json!(i64::MIN), json!(u64::MAX)]);
        let data = JsonValue::Array(items);
        assert!(delta_plan(data.as_array().unwrap(), &CompressionConfig::default()).is_none());
        let (strategy, _) = round_trip(&data);
        assert_ne!(strategy, CompressionStrategy::Delta);
    }

    #[test]
    fn test_nested_arrays_under_large_outer_do_not_count_rle() {
        let data = json!([vec![7; 200], vec![8; 200], vec![9; 200]]);
        assert_eq!(analyzed(&data), CompressionStrategy::None);
        round_trip(&data);
    }

    #[test]
    fn test_nested_arrays_under_small_outer_count_rle_exactly() {
        let data = json!([vec![7; 200], vec![7; 200]]);
        let modelled = 2 * rle_net_saving(data[0].as_array().unwrap());
        let (strategy, wire) = round_trip(&data);
        assert_eq!(strategy, CompressionStrategy::RunLength);
        assert_eq!(original_len(&data) - wire, modelled);
    }

    #[test]
    fn test_integer_sequence_inside_nested_array_selects_delta() {
        let data = json!({"outer": [ints(1_700_000_000_000, 40)]});
        let (strategy, wire) = round_trip(&data);
        assert_eq!(strategy, CompressionStrategy::Delta);
        assert!(wire < original_len(&data));
    }

    #[test]
    fn test_selection_tie_prefers_delta_over_run_length() {
        let analyzer = SchemaAnalyzer::new();
        let analysis = Analysis {
            strings: HashMap::new(),
            delta_net: 100,
            rle_net: 100,
        };
        assert_eq!(
            analyzer.determine_strategy(analysis),
            CompressionStrategy::Delta
        );
    }

    #[test]
    fn test_run_length_splits_at_max_count_and_keeps_raw_tail() {
        let data = json!(vec![0; MAX_RLE_COUNT + 1]);
        let mut compressor = SchemaCompressor::new();
        compressor.analyze_and_optimize(&data).unwrap();
        let compressed = compressor.compress(&data).unwrap();
        assert_eq!(
            compressed.data,
            json!([{"rle_value": 0, "rle_count": MAX_RLE_COUNT}, 0])
        );
        round_trip(&data);
    }

    #[test]
    fn test_sequential_timestamps_select_delta_and_shrink() {
        let data = json!({"ts": ints(1_700_000_000_000, 500)});
        let (strategy, wire) = round_trip(&data);
        assert_eq!(strategy, CompressionStrategy::Delta);
        assert!(wire < original_len(&data));
    }

    #[test]
    fn test_record_rows_do_not_select_run_length() {
        let rows: Vec<JsonValue> = (0..500u64)
            .map(|i| json!({"t": 1_700_000_000_000u64 + i, "v": i}))
            .collect();
        assert_eq!(analyzed(&JsonValue::Array(rows)), CompressionStrategy::None);
    }

    #[test]
    fn test_max_savings_prefers_run_length_over_dictionary() {
        let data = json!(vec!["premium_subscription_tier"; 200]);
        assert_eq!(analyzed(&data), CompressionStrategy::RunLength);
    }

    #[test]
    fn test_constant_integer_array_does_not_select_delta() {
        assert_ne!(analyzed(&json!(vec![7; 300])), CompressionStrategy::Delta);
    }

    #[test]
    fn test_hybrid_selected_when_both_parts_pay_off() {
        let long = "premium_subscription_tier";
        let data = json!({"seq": ints(5_000_000, 100), "tags": [long, long, long, "x"]});
        let (strategy, wire) = round_trip(&data);
        assert!(matches!(strategy, CompressionStrategy::Hybrid { .. }));
        assert!(wire < original_len(&data));
    }

    #[test]
    fn test_zero_floor_still_requires_positive_saving() {
        let analyzer = SchemaAnalyzer::with_config(CompressionConfig {
            min_net_savings: 0,
            ..CompressionConfig::default()
        });
        let data = json!({"a": "hello", "b": "world", "c": [1, 2, 3]});
        assert_eq!(analyzer.analyze(&data).unwrap(), CompressionStrategy::None);
    }

    #[test]
    fn test_short_runs_are_left_uncompressed() {
        assert_eq!(analyzed(&json!([1, 1, 1])), CompressionStrategy::None);
    }

    #[test]
    fn test_modelled_delta_saving_equals_measured_wire_delta() {
        let data = json!({"t": ints(1_700_000_000_000, 500)});
        let arr = data["t"].as_array().unwrap();
        let modelled = delta_plan(arr, &CompressionConfig::default())
            .unwrap()
            .saving;
        let (_, wire) = round_trip(&data);
        assert_eq!(original_len(&data) - wire, modelled);
    }

    #[test]
    fn test_modelled_rle_saving_equals_measured_wire_delta() {
        let data = json!({"s": vec!["premium_subscription_tier"; 200]});
        let modelled = rle_net_saving(data["s"].as_array().unwrap());
        let (strategy, wire) = round_trip(&data);
        assert_eq!(strategy, CompressionStrategy::RunLength);
        assert_eq!(original_len(&data) - wire, modelled);
    }

    #[test]
    fn test_delta_plan_rejects_non_candidates() {
        let config = CompressionConfig::default();
        for arr in [
            json!([1.0, 2.0, 3.0]),
            json!([i64::MIN, u64::MAX, 0]),
            json!([100, 101, 102]),
            json!([1, 2]),
            json!([1, "2", 3]),
        ] {
            assert!(
                delta_plan(arr.as_array().unwrap(), &config).is_none(),
                "{arr}"
            );
        }
    }

    #[test]
    fn test_delta_uses_min_as_base_for_negative_and_wide_values() {
        let arr = JsonValue::Array((0..20i64).map(|i| json!(i - 1_000_000_000_000)).collect());
        let plan = delta_plan(arr.as_array().unwrap(), &CompressionConfig::default()).unwrap();
        assert_eq!(plan.base, &json!(-1_000_000_000_000i64));
        assert_eq!(plan.delta_of(&json!(-999_999_999_990i64)), Some(10));
    }

    #[test]
    fn test_run_length_keeps_every_element_of_short_runs() {
        let mut items = vec![json!(1), json!(1), json!(2), json!(2), json!(3)];
        items.extend(vec![json!("premium_subscription_tier"); 60]);
        let data = JsonValue::Array(items);
        let (strategy, _) = round_trip(&data);
        assert_eq!(strategy, CompressionStrategy::RunLength);
    }

    #[test]
    fn test_run_length_preserves_sign_of_zero() {
        let mut items = vec![json!(0.0); 30];
        items.extend(vec![json!(-0.0); 30]);
        let data = JsonValue::Array(items);
        let mut compressor = SchemaCompressor::new();
        compressor.analyze_and_optimize(&data).unwrap();
        let compressed = compressor.compress(&data).unwrap();
        let runs = compressed.data.as_array().unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0]["rle_value"].to_string(), "0.0");
        assert_eq!(runs[1]["rle_value"].to_string(), "-0.0");
    }

    #[test]
    fn test_delta_on_nested_rows_round_trips() {
        let data = json!({"rows": [
            {"series": ints(1_700_000_000_000, 40)},
            {"series": ints(1_800_000_000_000, 40)}
        ]});
        let (strategy, wire) = round_trip(&data);
        assert_eq!(strategy, CompressionStrategy::Delta);
        assert!(wire < original_len(&data));
    }
}
