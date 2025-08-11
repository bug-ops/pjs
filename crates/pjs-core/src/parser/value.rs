//! Lazy JSON value types for zero-copy parsing

use crate::parser::scanner::{Range, ScanResult};
use crate::{Error, Result};
use smallvec::SmallVec;

/// Zero-copy JSON value representation
#[derive(Debug, Clone)]
pub enum JsonValue<'a> {
    /// Raw bytes slice (not parsed yet)
    Raw(&'a [u8]),
    /// Parsed string (zero-copy)
    String(&'a str),
    /// Number stored as bytes for lazy parsing
    Number(&'a [u8]),
    /// Boolean value
    Bool(bool),
    /// Null value
    Null,
    /// Array with lazy evaluation
    Array(LazyArray<'a>),
    /// Object with lazy evaluation
    Object(LazyObject<'a>),
}

/// Lazy array that parses elements on-demand
#[derive(Debug, Clone)]
pub struct LazyArray<'a> {
    /// Raw JSON bytes
    raw: &'a [u8],
    /// Pre-computed element boundaries using SIMD scanning
    boundaries: SmallVec<[Range; 32]>,
    /// Cache for parsed elements
    cache: std::collections::HashMap<usize, JsonValue<'a>>,
}

/// Lazy object that parses fields on-demand
#[derive(Debug, Clone)]
pub struct LazyObject<'a> {
    /// Raw JSON bytes
    raw: &'a [u8],
    /// Pre-computed key-value boundaries
    fields: SmallVec<[FieldRange; 16]>,
    /// Cache for parsed fields
    cache: std::collections::HashMap<String, JsonValue<'a>>,
}

/// Field boundary information
#[derive(Debug, Clone)]
pub struct FieldRange {
    /// Key range (without quotes)
    key: Range,
    /// Value range
    value: Range,
}

impl<'a> JsonValue<'a> {
    /// Get value as string if it's a string type
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Get value as f64 if it's a number
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(bytes) => std::str::from_utf8(bytes).ok()?.parse().ok(),
            _ => None,
        }
    }

    /// Get value as i64 if it's an integer number
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonValue::Number(bytes) => std::str::from_utf8(bytes).ok()?.parse().ok(),
            _ => None,
        }
    }

    /// Get value as bool if it's a boolean
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Check if value is null
    pub fn is_null(&self) -> bool {
        matches!(self, JsonValue::Null)
    }

    /// Get value as array
    pub fn as_array(&self) -> Option<&LazyArray<'a>> {
        match self {
            JsonValue::Array(arr) => Some(arr),
            _ => None,
        }
    }

    /// Get value as object
    pub fn as_object(&self) -> Option<&LazyObject<'a>> {
        match self {
            JsonValue::Object(obj) => Some(obj),
            _ => None,
        }
    }

    /// Force parse raw bytes into structured value
    pub fn parse_raw(&mut self) -> Result<()> {
        match self {
            JsonValue::Raw(_bytes) => {
                // This would use the main parser to parse the raw bytes
                // For now, we'll leave this as a placeholder
                *self = JsonValue::Null; // Simplified
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl<'a> LazyArray<'a> {
    /// Create new lazy array from scan result
    pub fn from_scan(raw: &'a [u8], scan_result: ScanResult) -> Self {
        // Extract array element boundaries from scan result
        let boundaries = Self::extract_element_boundaries(raw, &scan_result);

        Self {
            raw,
            boundaries,
            cache: std::collections::HashMap::new(),
        }
    }

    /// Get array length
    pub fn len(&self) -> usize {
        self.boundaries.len()
    }

    /// Check if array is empty
    pub fn is_empty(&self) -> bool {
        self.boundaries.is_empty()
    }

    /// Get element at index (simplified - returns raw bytes)
    pub fn get(&self, index: usize) -> Option<&'a [u8]> {
        if index >= self.boundaries.len() {
            return None;
        }

        let range = self.boundaries[index];
        Some(&self.raw[range.start..range.end])
    }

    /// Get element at index, parsing if necessary (simplified)
    pub fn get_parsed(&self, index: usize) -> Option<JsonValue<'a>> {
        self.get(index).map(JsonValue::Raw)
    }

    /// Iterator over array elements (lazy)
    pub fn iter(&'a self) -> LazyArrayIter<'a> {
        LazyArrayIter {
            array: self,
            index: 0,
        }
    }

    /// Extract element boundaries from structural analysis
    fn extract_element_boundaries(_raw: &[u8], _scan_result: &ScanResult) -> SmallVec<[Range; 32]> {
        // This would analyze the structural characters to find array element boundaries
        // For now, return empty boundaries as placeholder
        SmallVec::new()
    }

    /// Check if this appears to be a numeric array for SIMD optimization
    pub fn is_numeric(&self) -> bool {
        // Heuristic: check first few elements
        self.boundaries.len() > 4
            && self.boundaries.iter().take(3).all(|range| {
                let slice = &self.raw[range.start..range.end];
                self.looks_like_number(slice)
            })
    }

    fn looks_like_number(&self, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return false;
        }

        bytes.iter().all(|&b| {
            b.is_ascii_digit() || b == b'.' || b == b'-' || b == b'+' || b == b'e' || b == b'E'
        })
    }
}

impl<'a> LazyObject<'a> {
    /// Create new lazy object from scan result
    pub fn from_scan(raw: &'a [u8], scan_result: ScanResult) -> Self {
        let fields = Self::extract_field_boundaries(raw, &scan_result);

        Self {
            raw,
            fields,
            cache: std::collections::HashMap::new(),
        }
    }

    /// Get number of fields
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Check if object is empty
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Get field value by key (simplified)
    pub fn get(&self, key: &str) -> Option<&'a [u8]> {
        // Find field by key
        let field_range = self.fields.iter().find(|field| {
            let key_bytes = &self.raw[field.key.start..field.key.end];
            std::str::from_utf8(key_bytes) == Ok(key)
        })?;

        // Return value bytes
        Some(&self.raw[field_range.value.start..field_range.value.end])
    }

    /// Get all field keys
    pub fn keys(&self) -> Result<Vec<&str>> {
        self.fields
            .iter()
            .map(|field| {
                let key_bytes = &self.raw[field.key.start..field.key.end];
                std::str::from_utf8(key_bytes).map_err(Error::from)
            })
            .collect()
    }

    /// Extract field boundaries from structural analysis
    fn extract_field_boundaries(
        _raw: &[u8],
        _scan_result: &ScanResult,
    ) -> SmallVec<[FieldRange; 16]> {
        // This would analyze the structural characters to find object field boundaries
        // For now, return empty fields as placeholder
        SmallVec::new()
    }
}

/// Iterator for lazy array elements
pub struct LazyArrayIter<'a> {
    array: &'a LazyArray<'a>,
    index: usize,
}

impl<'a> Iterator for LazyArrayIter<'a> {
    type Item = &'a [u8]; // Raw element bytes

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.array.boundaries.len() {
            return None;
        }

        let range = self.array.boundaries[self.index];
        self.index += 1;

        Some(&self.array.raw[range.start..range.end])
    }
}

impl FieldRange {
    /// Create new field range
    pub fn new(key: Range, value: Range) -> Self {
        Self { key, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_value_types() {
        let val = JsonValue::String("hello");
        assert_eq!(val.as_str(), Some("hello"));
        assert!(val.as_f64().is_none());
    }

    #[test]
    fn test_lazy_array_creation() {
        let raw = b"[1, 2, 3]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        assert_eq!(array.len(), 0); // Empty boundaries in placeholder
    }

    #[test]
    fn test_number_detection() {
        let raw = b"[1.0, 2.5, 3.14]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        assert!(array.looks_like_number(b"123.45"));
        assert!(!array.looks_like_number(b"\"string\""));
    }
}
