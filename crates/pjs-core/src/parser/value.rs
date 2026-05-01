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
}

/// Lazy object that parses fields on-demand
#[derive(Debug, Clone)]
pub struct LazyObject<'a> {
    /// Raw JSON bytes
    raw: &'a [u8],
    /// Pre-computed key-value boundaries
    fields: SmallVec<[FieldRange; 16]>,
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

    /// Force parse raw bytes into the appropriate structured variant.
    ///
    /// Classifies the underlying bytes by their first non-whitespace character
    /// and replaces `JsonValue::Raw` with the matching typed variant
    /// (`Null`, `Bool`, `Number`, `String`, `Array`, or `Object`). Variants other
    /// than `Raw` are left unchanged.
    ///
    /// Numbers, strings, arrays, and objects keep zero-copy semantics by borrowing
    /// from the original byte slice. Strings containing escape sequences cannot be
    /// represented in `JsonValue::String<&str>` without allocation and are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidJson`] when the bytes are empty, contain an
    /// unterminated string, contain an escaped string (zero-copy not possible),
    /// or do not begin with a recognised JSON token.
    /// Returns [`Error::Utf8`] when string contents are not valid UTF-8.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::parser::JsonValue;
    ///
    /// let mut v = JsonValue::Raw(b"42");
    /// v.parse_raw().unwrap();
    /// assert_eq!(v.as_i64(), Some(42));
    ///
    /// let mut v = JsonValue::Raw(b"\"hello\"");
    /// v.parse_raw().unwrap();
    /// assert_eq!(v.as_str(), Some("hello"));
    /// ```
    pub fn parse_raw(&mut self) -> Result<()> {
        let bytes = if let JsonValue::Raw(bytes) = self {
            *bytes
        } else {
            return Ok(());
        };

        let Some(start) = bytes.iter().position(|b| !b.is_ascii_whitespace()) else {
            return Err(Error::invalid_json(0, "empty input"));
        };
        let end = bytes
            .iter()
            .rposition(|b| !b.is_ascii_whitespace())
            .map(|i| i + 1)
            .unwrap_or(bytes.len());
        let trimmed = &bytes[start..end];

        *self = match trimmed[0] {
            b'n' if trimmed == b"null" => JsonValue::Null,
            b't' if trimmed == b"true" => JsonValue::Bool(true),
            b'f' if trimmed == b"false" => JsonValue::Bool(false),
            b'"' => {
                if trimmed.len() < 2 || trimmed[trimmed.len() - 1] != b'"' {
                    return Err(Error::invalid_json(start, "unterminated string"));
                }
                let inner = &trimmed[1..trimmed.len() - 1];
                if inner.contains(&b'\\') {
                    return Err(Error::invalid_json(
                        start,
                        "escaped strings cannot be represented zero-copy",
                    ));
                }
                JsonValue::String(std::str::from_utf8(inner)?)
            }
            b'[' => JsonValue::Array(LazyArray::from_scan(trimmed, ScanResult::new())),
            b'{' => JsonValue::Object(LazyObject::from_scan(trimmed, ScanResult::new())),
            b'-' | b'0'..=b'9' => JsonValue::Number(trimmed),
            _ => return Err(Error::invalid_json(start, "unrecognised JSON value")),
        };

        Ok(())
    }
}

impl<'a> LazyArray<'a> {
    /// Create new lazy array from scan result
    pub fn from_scan(raw: &'a [u8], scan_result: ScanResult) -> Self {
        // Extract array element boundaries from scan result
        let boundaries = Self::extract_element_boundaries(raw, &scan_result);

        Self { raw, boundaries }
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

    /// Extract top-level element boundaries from a JSON array.
    ///
    /// Parses `raw` bytes assuming it is a JSON array (`[...]`) and returns
    /// a `Range` for each top-level element, trimmed of surrounding whitespace.
    /// Nested arrays/objects and strings (including escaped quotes) are treated
    /// opaquely — only depth-0 commas and the closing `]` act as delimiters.
    ///
    /// # Invariant
    ///
    /// Assumes well-formed JSON. Mismatched brackets in nested content (e.g. `[{]}`) may
    /// produce incorrect ranges without signalling an error.
    fn extract_element_boundaries(raw: &[u8], _scan_result: &ScanResult) -> SmallVec<[Range; 32]> {
        let mut result = SmallVec::new();
        let len = raw.len();

        // Find the opening '['.
        let mut pos = 0;
        while pos < len && raw[pos] != b'[' {
            pos += 1;
        }
        if pos == len {
            return result;
        }
        pos += 1; // skip '['

        let mut depth: usize = 1;
        let mut in_string = false;
        let mut elem_start: Option<usize> = None;

        while pos < len {
            let b = raw[pos];

            if in_string {
                if b == b'\\' {
                    // Skip the escaped character.
                    pos += 1;
                } else if b == b'"' {
                    in_string = false;
                }
                pos += 1;
                continue;
            }

            match b {
                b'"' => {
                    in_string = true;
                    if elem_start.is_none() {
                        elem_start = Some(pos);
                    }
                }
                b'[' | b'{' => {
                    depth += 1;
                    if elem_start.is_none() {
                        elem_start = Some(pos);
                    }
                }
                b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        // Closing bracket of the top-level array — emit last element.
                        if let Some(start) = elem_start {
                            let end = trim_end(raw, start, pos);
                            if end > start {
                                result.push(Range::new(start, end));
                            }
                        }
                        break;
                    }
                }
                b',' if depth == 1 => {
                    // Top-level separator — emit the current element.
                    if let Some(start) = elem_start {
                        let end = trim_end(raw, start, pos);
                        if end > start {
                            result.push(Range::new(start, end));
                        }
                    }
                    elem_start = None;
                }
                b' ' | b'\t' | b'\n' | b'\r' => {
                    // Whitespace before first non-space character of an element.
                    pos += 1;
                    continue;
                }
                _ => {
                    if elem_start.is_none() {
                        elem_start = Some(pos);
                    }
                }
            }
            pos += 1;
        }

        result
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

        Self { raw, fields }
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

    /// Extract top-level field boundaries from a JSON object.
    ///
    /// Parses `raw` bytes assuming it is a JSON object (`{...}`) and returns a
    /// `FieldRange` for each top-level field.  The `key` range covers the string
    /// content **without** surrounding quotes; the `value` range covers the full
    /// value representation (including quotes when the value is a string).
    ///
    /// # Invariant
    ///
    /// Assumes well-formed JSON. Malformed input (e.g. duplicate commas, mismatched
    /// brackets) may produce incomplete results without signalling an error.
    fn extract_field_boundaries(
        raw: &[u8],
        _scan_result: &ScanResult,
    ) -> SmallVec<[FieldRange; 16]> {
        let mut result = SmallVec::new();
        let len = raw.len();

        // Find the opening '{'.
        let mut pos = 0;
        while pos < len && raw[pos] != b'{' {
            pos += 1;
        }
        if pos == len {
            return result;
        }
        pos += 1; // skip '{'

        loop {
            // --- skip whitespace before key ---
            while pos < len && raw[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos >= len || raw[pos] == b'}' {
                break;
            }
            if raw[pos] != b'"' {
                // Malformed input; stop.
                break;
            }
            pos += 1; // skip opening '"'
            let key_start = pos;
            // Scan to closing '"', honouring backslash escapes.
            while pos < len && raw[pos] != b'"' {
                if raw[pos] == b'\\' {
                    pos += 1; // skip escaped char
                }
                pos += 1;
            }
            let key_end = pos;
            if pos < len {
                pos += 1; // skip closing '"'
            }

            // --- skip whitespace and ':' ---
            while pos < len && (raw[pos].is_ascii_whitespace() || raw[pos] == b':') {
                pos += 1;
            }
            if pos >= len {
                break;
            }

            // --- parse value with depth tracking ---
            let value_start = pos;
            let mut depth: usize = 0;
            let mut in_str = false;

            while pos < len {
                let b = raw[pos];
                if in_str {
                    if b == b'\\' {
                        pos += 1; // skip escaped char
                    } else if b == b'"' {
                        in_str = false;
                        if depth == 0 {
                            pos += 1;
                            break;
                        }
                    }
                    pos += 1;
                    continue;
                }
                match b {
                    b'"' => {
                        in_str = true;
                    }
                    b'[' | b'{' => depth += 1,
                    b']' | b'}' => {
                        if depth == 0 {
                            // Closing brace of the parent object — do not consume.
                            break;
                        }
                        depth -= 1;
                        if depth == 0 {
                            pos += 1;
                            break;
                        }
                    }
                    b',' if depth == 0 => {
                        // Separator between fields — do not consume.
                        break;
                    }
                    _ => {}
                }
                pos += 1;
            }

            let value_end = trim_end(raw, value_start, pos);
            if value_end > value_start {
                result.push(FieldRange::new(
                    Range::new(key_start, key_end),
                    Range::new(value_start, value_end),
                ));
            }

            // Skip ',' between fields (or '}' will exit on the next iteration).
            while pos < len && (raw[pos].is_ascii_whitespace() || raw[pos] == b',') {
                pos += 1;
            }
        }

        result
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

/// Return the index past the last non-whitespace byte in `raw[start..end]`.
///
/// Used to strip trailing whitespace from element and value ranges.
fn trim_end(raw: &[u8], start: usize, end: usize) -> usize {
    let mut e = end;
    while e > start && raw[e - 1].is_ascii_whitespace() {
        e -= 1;
    }
    e
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

        assert_eq!(array.len(), 3);
        assert_eq!(array.get(0), Some(b"1".as_ref()));
        assert_eq!(array.get(1), Some(b"2".as_ref()));
        assert_eq!(array.get(2), Some(b"3".as_ref()));
    }

    #[test]
    fn test_lazy_array_empty() {
        let array = LazyArray::from_scan(b"[]", ScanResult::new());
        assert_eq!(array.len(), 0);
        assert!(array.is_empty());
    }

    #[test]
    fn test_lazy_array_strings() {
        let raw = b"[\"hello\", \"world\"]";
        let array = LazyArray::from_scan(raw, ScanResult::new());
        assert_eq!(array.len(), 2);
        assert_eq!(array.get(0), Some(b"\"hello\"".as_ref()));
    }

    #[test]
    fn test_lazy_array_nested() {
        let raw = b"[1, [2, 3], {\"a\": 4}]";
        let array = LazyArray::from_scan(raw, ScanResult::new());
        assert_eq!(array.len(), 3);
        assert_eq!(array.get(0), Some(b"1".as_ref()));
        assert_eq!(array.get(1), Some(b"[2, 3]".as_ref()));
        assert_eq!(array.get(2), Some(b"{\"a\": 4}".as_ref()));
    }

    #[test]
    fn test_lazy_array_escaped_string() {
        let raw = br#"["say \"hi\"", "bye"]"#;
        let array = LazyArray::from_scan(raw, ScanResult::new());
        assert_eq!(array.len(), 2);
    }

    #[test]
    fn test_lazy_object_creation() {
        let obj = LazyObject::from_scan(b"{\"a\": 1, \"b\": 2}", ScanResult::new());
        assert_eq!(obj.len(), 2);
        assert_eq!(obj.get("a"), Some(b"1".as_ref()));
        assert_eq!(obj.get("b"), Some(b"2".as_ref()));
    }

    #[test]
    fn test_lazy_object_empty() {
        let obj = LazyObject::from_scan(b"{}", ScanResult::new());
        assert_eq!(obj.len(), 0);
        assert!(obj.is_empty());
    }

    #[test]
    fn test_lazy_object_string_value() {
        let raw = b"{\"name\": \"alice\"}";
        let obj = LazyObject::from_scan(raw, ScanResult::new());
        assert_eq!(obj.len(), 1);
        assert_eq!(obj.get("name"), Some(b"\"alice\"".as_ref()));
    }

    #[test]
    fn test_lazy_object_nested_value() {
        let raw = b"{\"arr\": [1, 2], \"n\": 42}";
        let obj = LazyObject::from_scan(raw, ScanResult::new());
        assert_eq!(obj.len(), 2);
        assert_eq!(obj.get("arr"), Some(b"[1, 2]".as_ref()));
        assert_eq!(obj.get("n"), Some(b"42".as_ref()));
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
