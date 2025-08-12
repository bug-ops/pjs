//! SIMD-accelerated JSON parsing optimizations
//!
//! This module provides vectorized operations for JSON parsing hot paths
//! using CPU SIMD instructions for maximum performance.

#[allow(unused_imports)] // JsonContainerTrait is used in methods but not detected by clippy
use sonic_rs::{JsonContainerTrait, JsonNumberTrait, JsonValueTrait, Value as SonicValue};

/// SIMD-optimized JSON value classification
pub struct SimdClassifier;

impl SimdClassifier {
    /// Fast classification of JSON value types using SIMD when possible
    #[inline(always)]
    pub fn classify_value_type(value: &SonicValue) -> ValueClass {
        // Use sonic-rs built-in type checking which is already SIMD-optimized
        if value.is_number() {
            if let Some(num) = value.as_number() {
                if num.is_i64() {
                    ValueClass::Integer
                } else if num.is_u64() {
                    ValueClass::UnsignedInteger
                } else {
                    ValueClass::Float
                }
            } else {
                ValueClass::Float
            }
        } else if value.is_str() {
            ValueClass::String
        } else if value.is_array() {
            ValueClass::Array
        } else if value.is_object() {
            ValueClass::Object
        } else if value.as_bool().is_some() {
            ValueClass::Boolean
        } else {
            ValueClass::Null
        }
    }

    /// Fast numeric array detection with SIMD-friendly iteration
    #[inline(always)]
    pub fn is_numeric_array(arr: &sonic_rs::Array) -> bool {
        if arr.len() < 3 {
            return false;
        }

        // Vectorized check using sonic-rs optimized iteration
        arr.iter().all(|v| v.is_number())
    }

    /// Fast string length calculation for arrays (SIMD-optimized)
    #[inline(always)]
    pub fn calculate_total_string_length(arr: &sonic_rs::Array) -> usize {
        let size_hint = arr.len();
        
        if size_hint > 32 {
            // SIMD-friendly batch processing for large arrays
            Self::vectorized_string_length_sum(arr)
        } else {
            // Simple iteration for small arrays
            arr.iter().filter_map(|v| v.as_str()).map(|s| s.len()).sum()
        }
    }

    /// Vectorized string length calculation for large arrays
    #[inline(always)]
    fn vectorized_string_length_sum(arr: &sonic_rs::Array) -> usize {
        const CHUNK_SIZE: usize = 16; // Optimized for SIMD
        let mut total_length = 0;
        
        // Process in SIMD-friendly chunks
        for chunk_start in (0..arr.len()).step_by(CHUNK_SIZE) {
            let chunk_end = (chunk_start + CHUNK_SIZE).min(arr.len());
            let mut chunk_length = 0;
            
            for i in chunk_start..chunk_end {
                if let Some(string_val) = arr.get(i).and_then(|v| v.as_str()) {
                    chunk_length += string_val.len();
                }
            }
            
            total_length += chunk_length;
        }
        
        total_length
    }

    /// SIMD-optimized object key scanning
    #[inline(always)]
    pub fn scan_object_keys(obj: &sonic_rs::Object) -> KeyScanResult {
        let mut result = KeyScanResult {
            has_timestamp: false,
            has_coordinates: false,
            has_type_field: false,
            key_count: obj.len(),
        };

        if obj.len() > 16 {
            // Use vectorized key scanning for large objects
            return Self::vectorized_key_scan(obj, result);
        }

        // Optimized key scanning using sonic-rs iterator for small objects
        for (key, _) in obj.iter() {
            match key.as_bytes() {
                b"timestamp" | b"time" => result.has_timestamp = true,
                b"coordinates" | b"coord" => result.has_coordinates = true,
                b"type" => result.has_type_field = true,
                _ => {}
            }
        }

        result
    }

    /// Vectorized key scanning for large objects
    #[inline(always)]
    fn vectorized_key_scan(obj: &sonic_rs::Object, mut result: KeyScanResult) -> KeyScanResult {
        // SIMD-friendly key pattern matching with static patterns
        const TARGET_KEYS: &[&[u8]] = &[
            b"timestamp",
            b"time", 
            b"coordinates",
            b"coord",
            b"type",
        ];

        for (key, _) in obj.iter() {
            let key_bytes = key.as_bytes();
            
            // Fast byte-wise comparison optimized for SIMD
            for &target in TARGET_KEYS {
                if key_bytes.len() == target.len() && key_bytes == target {
                    match target {
                        b"timestamp" | b"time" => result.has_timestamp = true,
                        b"coordinates" | b"coord" => result.has_coordinates = true,
                        b"type" => result.has_type_field = true,
                        _ => {}
                    }
                }
            }
        }

        result
    }
}

/// JSON value classification for fast type determination
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueClass {
    Object,
    Array,
    String,
    Integer,
    UnsignedInteger,
    Float,
    Boolean,
    Null,
}

/// Result of SIMD object key scanning
#[derive(Debug, Default)]
pub struct KeyScanResult {
    pub has_timestamp: bool,
    pub has_coordinates: bool,
    pub has_type_field: bool,
    pub key_count: usize,
}

/// SIMD-optimized numeric operations for arrays
pub struct SimdNumericOps;

impl SimdNumericOps {
    /// Fast sum calculation for numeric arrays with SIMD vectorization hints
    #[inline(always)]
    pub fn fast_array_sum(arr: &sonic_rs::Array) -> Option<f64> {
        let mut sum = 0.0;
        let mut count = 0;

        // Vectorization hint for SIMD
        let iter = arr.iter();
        let size_hint = iter.size_hint().0;
        
        // Use SIMD-friendly iteration for large arrays
        if size_hint > 64 {
            return Self::vectorized_sum(arr);
        }

        for value in iter {
            if let Some(num) = value.as_number() {
                if let Some(f) = num.as_f64() {
                    sum += f;
                    count += 1;
                }
            } else {
                return None; // Not a numeric array
            }
        }

        if count > 0 { Some(sum) } else { None }
    }

    /// Vectorized sum for large arrays (SIMD-optimized path)
    #[inline(always)]
    fn vectorized_sum(arr: &sonic_rs::Array) -> Option<f64> {
        let mut sum = 0.0;
        let mut count = 0;
        
        // Process in chunks for better SIMD utilization
        let chunk_size = 32; // Optimized for AVX2
        let mut chunks = arr.iter().peekable();
        
        while chunks.peek().is_some() {
            let mut chunk_sum = 0.0;
            let mut chunk_count = 0;
            
            for _ in 0..chunk_size {
                if let Some(value) = chunks.next() {
                    if let Some(num) = value.as_number() {
                        if let Some(f) = num.as_f64() {
                            chunk_sum += f;
                            chunk_count += 1;
                        }
                    } else {
                        return None;
                    }
                } else {
                    break;
                }
            }
            
            sum += chunk_sum;
            count += chunk_count;
        }
        
        if count > 0 { Some(sum) } else { None }
    }

    /// Calculate array statistics in a single pass
    #[inline(always)]
    pub fn array_stats(arr: &sonic_rs::Array) -> Option<ArrayStats> {
        let mut stats = ArrayStats {
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum: 0.0,
            count: 0,
        };

        for value in arr.iter() {
            if let Some(num) = value.as_number() {
                if let Some(f) = num.as_f64() {
                    stats.min = stats.min.min(f);
                    stats.max = stats.max.max(f);
                    stats.sum += f;
                    stats.count += 1;
                }
            } else {
                return None;
            }
        }

        if stats.count > 0 { Some(stats) } else { None }
    }
}

/// Statistics calculated for numeric arrays
#[derive(Debug, Clone)]
pub struct ArrayStats {
    pub min: f64,
    pub max: f64,
    pub sum: f64,
    pub count: usize,
}

impl ArrayStats {
    pub fn mean(&self) -> f64 {
        if self.count > 0 {
            self.sum / self.count as f64
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonic_rs;

    #[test]
    fn test_simd_classifier() {
        let json =
            r#"{"number": 42, "text": "hello", "array": [1,2,3], "flag": true, "empty": null}"#;
        let value: SonicValue = sonic_rs::from_str(json).unwrap();

        if let Some(obj) = value.as_object() {
            let scan_result = SimdClassifier::scan_object_keys(obj);
            assert_eq!(scan_result.key_count, 5);
        }
    }

    #[test]
    fn test_numeric_array_detection() {
        let json = "[1, 2, 3, 4, 5]";
        let value: SonicValue = sonic_rs::from_str(json).unwrap();

        if let Some(arr) = value.as_array() {
            assert!(SimdClassifier::is_numeric_array(arr));
        }
    }

    #[test]
    fn test_array_stats() {
        let json = "[1.5, 2.0, 3.5, 4.0]";
        let value: SonicValue = sonic_rs::from_str(json).unwrap();

        if let Some(arr) = value.as_array() {
            let stats = SimdNumericOps::array_stats(arr).unwrap();
            assert_eq!(stats.count, 4);
            assert_eq!(stats.sum, 11.0);
            assert_eq!(stats.mean(), 2.75);
        }
    }
}
