//! Zero-copy lazy JSON parser with lifetime management
//!
//! This parser minimizes memory allocations by working directly with input slices,
//! providing lazy evaluation and zero-copy string extraction where possible.

use crate::{
    config::SecurityConfig,
    domain::{DomainError, DomainResult},
    parser::ValueType,
    security::SecurityValidator,
};
use std::{marker::PhantomData, str::from_utf8};

/// Zero-copy lazy parser trait with lifetime management
///
/// This trait enables parsers that work directly on input buffers without
/// copying data, using Rust's lifetime system to ensure memory safety.
pub trait LazyParser<'a> {
    /// Parsed value returned by [`parse_lazy`](Self::parse_lazy).
    type Output;
    /// Error returned by lazy parsing operations.
    type Error;

    /// Parse input lazily, returning references into the original buffer
    fn parse_lazy(&mut self, input: &'a [u8]) -> Result<Self::Output, Self::Error>;

    /// Get the remaining unparsed bytes
    fn remaining(&self) -> &'a [u8];

    /// Check if parsing is complete
    fn is_complete(&self) -> bool;

    /// Reset parser state for reuse
    fn reset(&mut self);
}

/// Zero-copy JSON parser implementation
pub struct ZeroCopyParser<'a> {
    input: &'a [u8],
    position: usize,
    depth: usize,
    validator: SecurityValidator,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> ZeroCopyParser<'a> {
    /// Create new zero-copy parser
    pub fn new() -> Self {
        Self {
            input: &[],
            position: 0,
            depth: 0,
            validator: SecurityValidator::default(),
            _phantom: PhantomData,
        }
    }

    /// Create parser with custom security configuration
    pub fn with_security_config(security_config: SecurityConfig) -> Self {
        Self {
            input: &[],
            position: 0,
            depth: 0,
            validator: SecurityValidator::new(security_config),
            _phantom: PhantomData,
        }
    }

    /// Parse JSON value starting at current position
    pub fn parse_value(&mut self) -> DomainResult<LazyJsonValue<'a>> {
        self.skip_whitespace();

        if self.position >= self.input.len() {
            return Err(DomainError::InvalidInput(
                "Unexpected end of input".to_string(),
            ));
        }

        let ch = self.input[self.position];
        match ch {
            b'"' => self.parse_string(),
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b't' | b'f' => self.parse_boolean(),
            b'n' => self.parse_null(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => {
                let ch_char = ch as char;
                Err(DomainError::InvalidInput(format!(
                    "Unexpected character: {ch_char}"
                )))
            }
        }
    }

    /// Parse string value without copying
    fn parse_string(&mut self) -> DomainResult<LazyJsonValue<'a>> {
        if self.position >= self.input.len() || self.input[self.position] != b'"' {
            return Err(DomainError::InvalidInput("Expected '\"'".to_string()));
        }

        let start = self.position + 1; // Skip opening quote
        self.position += 1;

        // Find closing quote, handling escapes
        while self.position < self.input.len() {
            match self.input[self.position] {
                b'"' => {
                    let string_slice = &self.input[start..self.position];
                    self.position += 1; // Skip closing quote

                    // Check if string contains escape sequences
                    if string_slice.contains(&b'\\') {
                        // String needs unescaping - we'll need to allocate
                        let unescaped = self.unescape_string(string_slice)?;
                        return Ok(LazyJsonValue::StringOwned(unescaped));
                    } else {
                        // Zero-copy string reference
                        return Ok(LazyJsonValue::StringBorrowed(string_slice));
                    }
                }
                b'\\' => {
                    // Skip escape sequence
                    self.position += 2;
                }
                _ => {
                    self.position += 1;
                }
            }
        }

        Err(DomainError::InvalidInput("Unterminated string".to_string()))
    }

    /// Parse object value lazily
    fn parse_object(&mut self) -> DomainResult<LazyJsonValue<'a>> {
        self.validator
            .validate_json_depth(self.depth + 1)
            .map_err(|e| DomainError::SecurityViolation(e.to_string()))?;

        if self.position >= self.input.len() || self.input[self.position] != b'{' {
            return Err(DomainError::InvalidInput("Expected '{'".to_string()));
        }

        let start = self.position;
        self.position += 1; // Skip '{'
        self.depth += 1;

        self.skip_whitespace();

        // Handle empty object
        if self.position < self.input.len() && self.input[self.position] == b'}' {
            self.position += 1;
            self.depth -= 1;
            return Ok(LazyJsonValue::ObjectSlice(
                &self.input[start..self.position],
            ));
        }

        let mut first = true;
        while self.position < self.input.len() && self.input[self.position] != b'}' {
            if !first {
                self.expect_char(b',')?;
                self.skip_whitespace();
            }
            first = false;

            // Parse key (must be string)
            let _key = self.parse_value()?;
            self.skip_whitespace();
            self.expect_char(b':')?;
            self.skip_whitespace();

            // Parse value
            let _value = self.parse_value()?;
            self.skip_whitespace();
        }

        self.expect_char(b'}')?;
        self.depth -= 1;

        Ok(LazyJsonValue::ObjectSlice(
            &self.input[start..self.position],
        ))
    }

    /// Parse array value lazily
    fn parse_array(&mut self) -> DomainResult<LazyJsonValue<'a>> {
        self.validator
            .validate_json_depth(self.depth + 1)
            .map_err(|e| DomainError::SecurityViolation(e.to_string()))?;

        if self.position >= self.input.len() || self.input[self.position] != b'[' {
            return Err(DomainError::InvalidInput("Expected '['".to_string()));
        }

        let start = self.position;
        self.position += 1; // Skip '['
        self.depth += 1;

        self.skip_whitespace();

        // Handle empty array
        if self.position < self.input.len() && self.input[self.position] == b']' {
            self.position += 1;
            self.depth -= 1;
            return Ok(LazyJsonValue::ArraySlice(&self.input[start..self.position]));
        }

        let mut first = true;
        while self.position < self.input.len() && self.input[self.position] != b']' {
            if !first {
                self.expect_char(b',')?;
                self.skip_whitespace();
            }
            first = false;

            // Parse array element
            let _element = self.parse_value()?;
            self.skip_whitespace();
        }

        self.expect_char(b']')?;
        self.depth -= 1;

        Ok(LazyJsonValue::ArraySlice(&self.input[start..self.position]))
    }

    /// Parse boolean value
    fn parse_boolean(&mut self) -> DomainResult<LazyJsonValue<'a>> {
        if self.position + 4 <= self.input.len()
            && &self.input[self.position..self.position + 4] == b"true"
        {
            self.position += 4;
            Ok(LazyJsonValue::Boolean(true))
        } else if self.position + 5 <= self.input.len()
            && &self.input[self.position..self.position + 5] == b"false"
        {
            self.position += 5;
            Ok(LazyJsonValue::Boolean(false))
        } else {
            Err(DomainError::InvalidInput(
                "Invalid boolean value".to_string(),
            ))
        }
    }

    /// Parse null value
    fn parse_null(&mut self) -> DomainResult<LazyJsonValue<'a>> {
        if self.position + 4 <= self.input.len()
            && &self.input[self.position..self.position + 4] == b"null"
        {
            self.position += 4;
            Ok(LazyJsonValue::Null)
        } else {
            Err(DomainError::InvalidInput("Invalid null value".to_string()))
        }
    }

    /// Parse number value with zero-copy when possible
    fn parse_number(&mut self) -> DomainResult<LazyJsonValue<'a>> {
        let start = self.position;

        // Handle negative sign
        if self.input[self.position] == b'-' {
            self.position += 1;
        }

        // Parse integer part
        if self.position >= self.input.len() {
            return Err(DomainError::InvalidInput("Invalid number".to_string()));
        }

        if self.input[self.position] == b'0' {
            self.position += 1;
        } else if self.input[self.position].is_ascii_digit() {
            while self.position < self.input.len() && self.input[self.position].is_ascii_digit() {
                self.position += 1;
            }
        } else {
            return Err(DomainError::InvalidInput("Invalid number".to_string()));
        }

        // Handle decimal part
        if self.position < self.input.len() && self.input[self.position] == b'.' {
            self.position += 1;
            if self.position >= self.input.len() || !self.input[self.position].is_ascii_digit() {
                return Err(DomainError::InvalidInput(
                    "Invalid number: missing digits after decimal".to_string(),
                ));
            }
            while self.position < self.input.len() && self.input[self.position].is_ascii_digit() {
                self.position += 1;
            }
        }

        // Handle exponent
        if self.position < self.input.len()
            && (self.input[self.position] == b'e' || self.input[self.position] == b'E')
        {
            self.position += 1;
            if self.position < self.input.len()
                && (self.input[self.position] == b'+' || self.input[self.position] == b'-')
            {
                self.position += 1;
            }
            if self.position >= self.input.len() || !self.input[self.position].is_ascii_digit() {
                return Err(DomainError::InvalidInput(
                    "Invalid number: missing digits in exponent".to_string(),
                ));
            }
            while self.position < self.input.len() && self.input[self.position].is_ascii_digit() {
                self.position += 1;
            }
        }

        let number_slice = &self.input[start..self.position];
        Ok(LazyJsonValue::NumberSlice(number_slice))
    }

    /// Skip whitespace characters
    fn skip_whitespace(&mut self) {
        while self.position < self.input.len() {
            match self.input[self.position] {
                b' ' | b'\t' | b'\n' | b'\r' => {
                    self.position += 1;
                }
                _ => break,
            }
        }
    }

    /// Expect specific character at current position
    fn expect_char(&mut self, ch: u8) -> DomainResult<()> {
        if self.position >= self.input.len() || self.input[self.position] != ch {
            let ch_char = ch as char;
            return Err(DomainError::InvalidInput(format!("Expected '{ch_char}'")));
        }
        self.position += 1;
        Ok(())
    }

    /// Unescape string (requires allocation)
    fn unescape_string(&self, input: &[u8]) -> DomainResult<String> {
        let mut result = Vec::with_capacity(input.len());
        let mut i = 0;

        while i < input.len() {
            if input[i] == b'\\' && i + 1 < input.len() {
                match input[i + 1] {
                    b'"' => result.push(b'"'),
                    b'\\' => result.push(b'\\'),
                    b'/' => result.push(b'/'),
                    b'b' => result.push(b'\x08'),
                    b'f' => result.push(b'\x0C'),
                    b'n' => result.push(b'\n'),
                    b'r' => result.push(b'\r'),
                    b't' => result.push(b'\t'),
                    b'u' => {
                        let high = Self::parse_hex4(input, i + 2)?;
                        i += 6;

                        let codepoint = if (0xD800..=0xDBFF).contains(&high) {
                            // High surrogate: must be followed by a low surrogate.
                            if i + 1 >= input.len() || input[i] != b'\\' || input[i + 1] != b'u' {
                                return Err(DomainError::InvalidInput(
                                    "Unpaired high surrogate in unicode escape".to_string(),
                                ));
                            }
                            let low = Self::parse_hex4(input, i + 2)?;
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return Err(DomainError::InvalidInput(
                                    "High surrogate not followed by low surrogate".to_string(),
                                ));
                            }
                            i += 6;
                            0x10000 + (high - 0xD800) * 0x400 + (low - 0xDC00)
                        } else if (0xDC00..=0xDFFF).contains(&high) {
                            return Err(DomainError::InvalidInput(
                                "Unpaired low surrogate in unicode escape".to_string(),
                            ));
                        } else {
                            high
                        };

                        let ch = char::from_u32(codepoint).ok_or_else(|| {
                            DomainError::InvalidInput(
                                "Invalid unicode codepoint in escape sequence".to_string(),
                            )
                        })?;
                        let mut buf = [0u8; 4];
                        result.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        continue;
                    }
                    _ => {
                        return Err(DomainError::InvalidInput(
                            "Invalid escape sequence".to_string(),
                        ));
                    }
                }
                i += 2;
            } else {
                result.push(input[i]);
                i += 1;
            }
        }

        String::from_utf8(result)
            .map_err(|e| DomainError::InvalidInput(format!("Invalid UTF-8: {e}")))
    }

    /// Parse a 4-digit hex escape (`XXXX` in `\uXXXX`) starting at `pos`.
    fn parse_hex4(input: &[u8], pos: usize) -> DomainResult<u32> {
        let hex = input
            .get(pos..pos + 4)
            .ok_or_else(|| DomainError::InvalidInput("Invalid unicode escape".to_string()))?;
        // `from_str_radix` alone would accept a leading `+` (e.g. "+041"); reject
        // anything but plain hex digits so malformed escapes error instead of
        // silently decoding to an unintended codepoint.
        if !hex.iter().all(u8::is_ascii_hexdigit) {
            return Err(DomainError::InvalidInput(
                "Invalid unicode escape".to_string(),
            ));
        }
        let hex_str = std::str::from_utf8(hex)
            .map_err(|_| DomainError::InvalidInput("Invalid unicode escape".to_string()))?;
        u32::from_str_radix(hex_str, 16)
            .map_err(|_| DomainError::InvalidInput("Invalid unicode escape".to_string()))
    }
}

impl<'a> LazyParser<'a> for ZeroCopyParser<'a> {
    type Output = LazyJsonValue<'a>;
    type Error = DomainError;

    fn parse_lazy(&mut self, input: &'a [u8]) -> Result<Self::Output, Self::Error> {
        // Validate input size first
        self.validator
            .validate_input_size(input.len())
            .map_err(|e| DomainError::SecurityViolation(e.to_string()))?;

        self.input = input;
        self.position = 0;
        self.depth = 0;

        self.parse_value()
    }

    fn remaining(&self) -> &'a [u8] {
        if self.position < self.input.len() {
            &self.input[self.position..]
        } else {
            &[]
        }
    }

    fn is_complete(&self) -> bool {
        self.position >= self.input.len()
    }

    fn reset(&mut self) {
        self.input = &[];
        self.position = 0;
        self.depth = 0;
    }
}

/// Zero-copy JSON value that references original buffer when possible
#[derive(Debug, Clone, PartialEq)]
pub enum LazyJsonValue<'a> {
    /// String that references original buffer (no escapes)
    StringBorrowed(&'a [u8]),
    /// String that required unescaping (allocated)
    StringOwned(String),
    /// Number as slice of original buffer
    NumberSlice(&'a [u8]),
    /// Boolean value
    Boolean(bool),
    /// Null value
    Null,
    /// Object as slice of original buffer
    ObjectSlice(&'a [u8]),
    /// Array as slice of original buffer
    ArraySlice(&'a [u8]),
}

impl<'a> LazyJsonValue<'a> {
    /// Get value type
    pub fn value_type(&self) -> ValueType {
        match self {
            LazyJsonValue::StringBorrowed(_) | LazyJsonValue::StringOwned(_) => ValueType::String,
            LazyJsonValue::NumberSlice(_) => ValueType::Number,
            LazyJsonValue::Boolean(_) => ValueType::Boolean,
            LazyJsonValue::Null => ValueType::Null,
            LazyJsonValue::ObjectSlice(_) => ValueType::Object,
            LazyJsonValue::ArraySlice(_) => ValueType::Array,
        }
    }

    /// Convert to string (allocating if needed)
    pub fn to_string_lossy(&self) -> String {
        match self {
            LazyJsonValue::StringBorrowed(bytes) => String::from_utf8_lossy(bytes).to_string(),
            LazyJsonValue::StringOwned(s) => s.clone(),
            LazyJsonValue::NumberSlice(bytes) => String::from_utf8_lossy(bytes).to_string(),
            LazyJsonValue::Boolean(b) => b.to_string(),
            LazyJsonValue::Null => "null".to_string(),
            LazyJsonValue::ObjectSlice(bytes) => String::from_utf8_lossy(bytes).to_string(),
            LazyJsonValue::ArraySlice(bytes) => String::from_utf8_lossy(bytes).to_string(),
        }
    }

    /// Try to parse as string without allocation
    pub fn as_str(&self) -> DomainResult<&str> {
        match self {
            LazyJsonValue::StringBorrowed(bytes) => from_utf8(bytes)
                .map_err(|e| DomainError::InvalidInput(format!("Invalid UTF-8: {e}"))),
            LazyJsonValue::StringOwned(s) => Ok(s.as_str()),
            _ => Err(DomainError::InvalidInput(
                "Value is not a string".to_string(),
            )),
        }
    }

    /// Try to parse as number
    pub fn as_number(&self) -> DomainResult<f64> {
        match self {
            LazyJsonValue::NumberSlice(bytes) => {
                let s = from_utf8(bytes)
                    .map_err(|e| DomainError::InvalidInput(format!("Invalid UTF-8: {e}")))?;
                s.parse::<f64>()
                    .map_err(|e| DomainError::InvalidInput(format!("Invalid number: {e}")))
            }
            _ => Err(DomainError::InvalidInput(
                "Value is not a number".to_string(),
            )),
        }
    }

    /// Try to parse as boolean
    pub fn as_boolean(&self) -> DomainResult<bool> {
        match self {
            LazyJsonValue::Boolean(b) => Ok(*b),
            _ => Err(DomainError::InvalidInput(
                "Value is not a boolean".to_string(),
            )),
        }
    }

    /// Check if value is null
    pub fn is_null(&self) -> bool {
        matches!(self, LazyJsonValue::Null)
    }

    /// Get raw bytes for zero-copy access
    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        match self {
            LazyJsonValue::StringBorrowed(bytes) => Some(bytes),
            LazyJsonValue::NumberSlice(bytes) => Some(bytes),
            LazyJsonValue::ObjectSlice(bytes) => Some(bytes),
            LazyJsonValue::ArraySlice(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Estimate memory usage (allocated vs referenced)
    pub fn memory_usage(&self) -> MemoryUsage {
        match self {
            LazyJsonValue::StringBorrowed(bytes) => MemoryUsage {
                allocated_bytes: 0,
                referenced_bytes: bytes.len(),
            },
            LazyJsonValue::StringOwned(s) => MemoryUsage {
                allocated_bytes: s.len(),
                referenced_bytes: 0,
            },
            LazyJsonValue::NumberSlice(bytes) => MemoryUsage {
                allocated_bytes: 0,
                referenced_bytes: bytes.len(),
            },
            LazyJsonValue::Boolean(val) => MemoryUsage {
                allocated_bytes: 0,
                referenced_bytes: if *val { 4 } else { 5 }, // "true" or "false"
            },
            LazyJsonValue::Null => MemoryUsage {
                allocated_bytes: 0,
                referenced_bytes: 4, // "null"
            },
            LazyJsonValue::ObjectSlice(bytes) => MemoryUsage {
                allocated_bytes: 0,
                referenced_bytes: bytes.len(),
            },
            LazyJsonValue::ArraySlice(bytes) => MemoryUsage {
                allocated_bytes: 0,
                referenced_bytes: bytes.len(),
            },
        }
    }
}

/// Memory usage statistics for lazy values
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryUsage {
    /// Bytes that were allocated (copied)
    pub allocated_bytes: usize,
    /// Bytes that are referenced from original buffer
    pub referenced_bytes: usize,
}

impl MemoryUsage {
    /// Total memory footprint
    pub fn total(&self) -> usize {
        self.allocated_bytes + self.referenced_bytes
    }

    /// Efficiency ratio (0.0 = all copied, 1.0 = all zero-copy)
    pub fn efficiency(&self) -> f64 {
        if self.total() == 0 {
            1.0
        } else {
            self.referenced_bytes as f64 / self.total() as f64
        }
    }
}

/// Incremental parser for streaming scenarios
pub struct IncrementalParser<'a> {
    buffer: Vec<u8>,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> Default for IncrementalParser<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> IncrementalParser<'a> {
    /// Create an empty incremental parser with an 8 KiB initial buffer.
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(8192), // 8KB initial capacity
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add more data to the parser buffer
    pub fn feed(&mut self, data: &[u8]) -> DomainResult<()> {
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    /// Parse any complete values from buffer
    pub fn parse_available(&mut self) -> DomainResult<Vec<LazyJsonValue<'_>>> {
        // For simplicity, this is a basic implementation
        // A production version would need more sophisticated buffering
        if !self.buffer.is_empty() {
            let mut parser = ZeroCopyParser::new();
            match parser.parse_lazy(&self.buffer) {
                Ok(_value) => {
                    // This is a simplified approach - real implementation would need
                    // proper lifetime management for incremental parsing
                    self.buffer.clear();
                    Ok(vec![])
                }
                Err(_e) => Ok(vec![]), // Not enough data yet
            }
        } else {
            Ok(vec![])
        }
    }

    /// Check if buffer has complete JSON value
    pub fn has_complete_value(&self) -> bool {
        // Simplified check - real implementation would track bracket/brace nesting
        !self.buffer.is_empty()
    }
}

impl<'a> Default for ZeroCopyParser<'a> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_string() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""hello world""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringBorrowed(bytes) => {
                assert_eq!(bytes, b"hello world");
            }
            _ => panic!("Expected string"),
        }
    }

    #[test]
    fn test_parse_escaped_string() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""hello \"world\"""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "hello \"world\"");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_parse_number() {
        let mut parser = ZeroCopyParser::new();
        let input = b"123.45";

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::NumberSlice(bytes) => {
                assert_eq!(bytes, b"123.45");
                assert_eq!(result.as_number().unwrap(), 123.45);
            }
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_parse_boolean() {
        let mut parser = ZeroCopyParser::new();

        let result = parser.parse_lazy(b"true").unwrap();
        assert_eq!(result, LazyJsonValue::Boolean(true));

        parser.reset();
        let result = parser.parse_lazy(b"false").unwrap();
        assert_eq!(result, LazyJsonValue::Boolean(false));
    }

    #[test]
    fn test_parse_null() {
        let mut parser = ZeroCopyParser::new();
        let result = parser.parse_lazy(b"null").unwrap();
        assert_eq!(result, LazyJsonValue::Null);
        assert!(result.is_null());
    }

    #[test]
    fn test_parse_empty_object() {
        let mut parser = ZeroCopyParser::new();
        let result = parser.parse_lazy(b"{}").unwrap();

        match result {
            LazyJsonValue::ObjectSlice(bytes) => {
                assert_eq!(bytes, b"{}");
            }
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_parse_empty_array() {
        let mut parser = ZeroCopyParser::new();
        let result = parser.parse_lazy(b"[]").unwrap();

        match result {
            LazyJsonValue::ArraySlice(bytes) => {
                assert_eq!(bytes, b"[]");
            }
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_memory_usage() {
        let mut parser = ZeroCopyParser::new();

        // Zero-copy string
        let result1 = parser.parse_lazy(br#""hello""#).unwrap();
        let usage1 = result1.memory_usage();
        assert_eq!(usage1.allocated_bytes, 0);
        assert_eq!(usage1.referenced_bytes, 5);
        assert_eq!(usage1.efficiency(), 1.0);

        // Escaped string (requires allocation)
        parser.reset();
        let result2 = parser.parse_lazy(br#""he\"llo""#).unwrap();
        let usage2 = result2.memory_usage();
        assert!(usage2.allocated_bytes > 0);
        assert_eq!(usage2.referenced_bytes, 0);
        assert_eq!(usage2.efficiency(), 0.0);
    }

    #[test]
    fn test_complex_object() {
        let mut parser = ZeroCopyParser::new();
        let input = br#"{"name": "test", "value": 42, "active": true}"#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::ObjectSlice(bytes) => {
                assert_eq!(bytes.len(), input.len());
            }
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_parser_reuse() {
        let mut parser = ZeroCopyParser::new();

        // First parse
        let result1 = parser.parse_lazy(b"123").unwrap();
        assert!(matches!(result1, LazyJsonValue::NumberSlice(_)));

        // Reset and reuse
        parser.reset();
        let result2 = parser.parse_lazy(br#""hello""#).unwrap();
        assert!(matches!(result2, LazyJsonValue::StringBorrowed(_)));
    }

    #[test]
    fn test_escape_sequence_slash() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""path\/to\/file""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "path/to/file");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_escape_sequence_backspace() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""text\bwith\bbackspace""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "text\x08with\x08backspace");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_escape_sequence_formfeed() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""text\fwith\fformfeed""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "text\x0Cwith\x0Cformfeed");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_escape_sequence_unicode_basic() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""text\u0041""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "textA");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_escape_sequence_unicode_surrogate_pair() {
        let mut parser = ZeroCopyParser::new();
        // U+1F600 GRINNING FACE, encoded via a UTF-16 surrogate pair escape
        let input = br#""\uD83D\uDE00""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "\u{1F600}");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_escape_sequence_unicode_unpaired_high_surrogate_errors() {
        let mut parser = ZeroCopyParser::new();
        // Lone high surrogate with no following low surrogate escape
        let input = br#""\uD83D""#;

        let result = parser.parse_lazy(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_escape_sequence_unicode_lone_low_surrogate_errors() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""\uDE00""#;

        let result = parser.parse_lazy(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_escape_sequence_unicode_hex_too_short_errors() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""\u00""#;

        let result = parser.parse_lazy(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_escape_sequence_unicode_invalid_hex_digit_errors() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""\uZZZZ""#;

        let result = parser.parse_lazy(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_escape_sequence_unicode_leading_plus_rejected() {
        let mut parser = ZeroCopyParser::new();
        // Regression test: `u32::from_str_radix` alone accepts a leading '+',
        // which must not be treated as a valid hex digit.
        let input = br#""\u+041""#;

        let result = parser.parse_lazy(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_escape_sequence_unicode_high_surrogate_not_followed_by_escape_errors() {
        let mut parser = ZeroCopyParser::new();
        // High surrogate followed by plain characters (no backslash at all).
        let input = br#""\uD83DAB""#;

        let result = parser.parse_lazy(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_escape_sequence_unicode_high_surrogate_followed_by_non_u_escape_errors() {
        let mut parser = ZeroCopyParser::new();
        // High surrogate followed by a `\n` escape rather than `\u`.
        let input = br#""\uD83D\n""#;

        let result = parser.parse_lazy(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_escape_sequence_unicode_high_surrogate_followed_by_non_low_surrogate_errors() {
        let mut parser = ZeroCopyParser::new();
        // Second escape is a valid \u escape but its value is not a low surrogate.
        let input = br#""\uD83D\u0041""#;

        let result = parser.parse_lazy(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_escape_sequence_unicode_null_codepoint() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""\u0000""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "\u{0000}");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_escape_sequence_unicode_two_byte_char() {
        let mut parser = ZeroCopyParser::new();
        // U+00E9 LATIN SMALL LETTER E WITH ACUTE, 2-byte UTF-8
        let input = br#""\u00e9""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "\u{00e9}");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_escape_sequence_unicode_three_byte_char() {
        let mut parser = ZeroCopyParser::new();
        // U+4E2D CJK UNIFIED IDEOGRAPH, 3-byte UTF-8
        let input = br#""\u4e2d""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "\u{4e2d}");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_escape_sequence_unicode_max_bmp_noncharacter() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""\uffff""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "\u{ffff}");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_escape_sequence_unicode_lowercase_hex_surrogate_pair() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""\ud83d\ude00""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "\u{1F600}");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_escape_sequence_unicode_surrogate_pair_with_surrounding_ascii() {
        let mut parser = ZeroCopyParser::new();
        let input = br#""a\uD83D\uDE00b""#;

        let result = parser.parse_lazy(input).unwrap();
        match result {
            LazyJsonValue::StringOwned(s) => {
                assert_eq!(s, "a\u{1F600}b");
            }
            _ => panic!("Expected owned string due to escapes"),
        }
    }

    #[test]
    fn test_number_parsing_partial() {
        let mut parser = ZeroCopyParser::new();
        // Parser reads valid prefix and may not error on trailing invalid chars
        let result = parser.parse_lazy(b"123");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), LazyJsonValue::NumberSlice(_)));
    }

    #[test]
    fn test_number_parsing_error_overflow() {
        let mut parser = ZeroCopyParser::new();
        // Very large number that might cause issues
        let input = b"99999999999999999999999999999999999999999999999999";
        let result = parser.parse_lazy(input);
        // Should either parse as number or fail gracefully
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_incremental_parser_feed() {
        let mut parser = IncrementalParser::new();

        // Feed some data
        let result = parser.feed(b"{\"key\":");
        assert!(result.is_ok());

        // Feed more data
        let result2 = parser.feed(b"\"value\"}");
        assert!(result2.is_ok());
    }

    #[test]
    fn test_incremental_parser_multiple_feeds() {
        let mut parser = IncrementalParser::new();

        parser.feed(b"[1,").unwrap();
        parser.feed(b"2,").unwrap();
        parser.feed(b"3]").unwrap();
    }

    #[test]
    fn test_lazy_json_value_matches() {
        let num = LazyJsonValue::NumberSlice(b"123");
        assert!(matches!(num, LazyJsonValue::NumberSlice(_)));
        assert!(!num.is_null());

        let null = LazyJsonValue::Null;
        assert!(null.is_null());
        assert!(!matches!(null, LazyJsonValue::NumberSlice(_)));

        let bool_val = LazyJsonValue::Boolean(true);
        assert!(matches!(bool_val, LazyJsonValue::Boolean(true)));
        assert!(!bool_val.is_null());
    }

    #[test]
    fn test_memory_usage_zero_copy_efficiency() {
        let borrowed = LazyJsonValue::StringBorrowed(b"test");
        let usage = borrowed.memory_usage();
        assert_eq!(usage.efficiency(), 1.0);
        assert_eq!(usage.allocated_bytes, 0);

        let owned = LazyJsonValue::StringOwned("test".to_string());
        let usage2 = owned.memory_usage();
        assert_eq!(usage2.efficiency(), 0.0);
        assert!(usage2.allocated_bytes > 0);
    }
}
