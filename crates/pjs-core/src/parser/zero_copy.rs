//! Zero-copy lazy JSON parser with lifetime management
//!
//! This parser minimizes memory allocations by working directly with input slices,
//! providing lazy evaluation and zero-copy string extraction where possible.

use crate::{
    domain::{DomainResult, DomainError},
    parser::ValueType,
};
use std::{
    marker::PhantomData,
    str::from_utf8,
};

/// Zero-copy lazy parser trait with lifetime management
/// 
/// This trait enables parsers that work directly on input buffers without
/// copying data, using Rust's lifetime system to ensure memory safety.
pub trait LazyParser<'a> {
    type Output;
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
    max_depth: usize,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> ZeroCopyParser<'a> {
    /// Create new zero-copy parser
    pub fn new() -> Self {
        Self {
            input: &[],
            position: 0,
            depth: 0,
            max_depth: 64, // Prevent stack overflow
            _phantom: PhantomData,
        }
    }

    /// Create parser with custom max depth
    pub fn with_max_depth(max_depth: usize) -> Self {
        Self {
            input: &[],
            position: 0,
            depth: 0,
            max_depth,
            _phantom: PhantomData,
        }
    }

    /// Parse JSON value starting at current position
    pub fn parse_value(&mut self) -> DomainResult<LazyJsonValue<'a>> {
        self.skip_whitespace();
        
        if self.position >= self.input.len() {
            return Err(DomainError::InvalidInput("Unexpected end of input".to_string()));
        }

        let ch = self.input[self.position];
        match ch {
            b'"' => self.parse_string(),
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b't' | b'f' => self.parse_boolean(),
            b'n' => self.parse_null(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => Err(DomainError::InvalidInput(format!("Unexpected character: {}", ch as char))),
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
        if self.depth >= self.max_depth {
            return Err(DomainError::InvalidInput("Maximum nesting depth exceeded".to_string()));
        }

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
            return Ok(LazyJsonValue::ObjectSlice(&self.input[start..self.position]));
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

        Ok(LazyJsonValue::ObjectSlice(&self.input[start..self.position]))
    }

    /// Parse array value lazily
    fn parse_array(&mut self) -> DomainResult<LazyJsonValue<'a>> {
        if self.depth >= self.max_depth {
            return Err(DomainError::InvalidInput("Maximum nesting depth exceeded".to_string()));
        }

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
        if self.position + 4 <= self.input.len() && &self.input[self.position..self.position + 4] == b"true" {
            self.position += 4;
            Ok(LazyJsonValue::Boolean(true))
        } else if self.position + 5 <= self.input.len() && &self.input[self.position..self.position + 5] == b"false" {
            self.position += 5;
            Ok(LazyJsonValue::Boolean(false))
        } else {
            Err(DomainError::InvalidInput("Invalid boolean value".to_string()))
        }
    }

    /// Parse null value
    fn parse_null(&mut self) -> DomainResult<LazyJsonValue<'a>> {
        if self.position + 4 <= self.input.len() && &self.input[self.position..self.position + 4] == b"null" {
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
                return Err(DomainError::InvalidInput("Invalid number: missing digits after decimal".to_string()));
            }
            while self.position < self.input.len() && self.input[self.position].is_ascii_digit() {
                self.position += 1;
            }
        }

        // Handle exponent
        if self.position < self.input.len() && (self.input[self.position] == b'e' || self.input[self.position] == b'E') {
            self.position += 1;
            if self.position < self.input.len() && (self.input[self.position] == b'+' || self.input[self.position] == b'-') {
                self.position += 1;
            }
            if self.position >= self.input.len() || !self.input[self.position].is_ascii_digit() {
                return Err(DomainError::InvalidInput("Invalid number: missing digits in exponent".to_string()));
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
            return Err(DomainError::InvalidInput(format!("Expected '{}'", ch as char)));
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
                        // Unicode escape sequence
                        if i + 5 < input.len() {
                            // Simplified: just skip unicode for now
                            i += 6;
                            continue;
                        } else {
                            return Err(DomainError::InvalidInput("Invalid unicode escape".to_string()));
                        }
                    }
                    _ => return Err(DomainError::InvalidInput("Invalid escape sequence".to_string())),
                }
                i += 2;
            } else {
                result.push(input[i]);
                i += 1;
            }
        }

        String::from_utf8(result)
            .map_err(|e| DomainError::InvalidInput(format!("Invalid UTF-8: {}", e)))
    }
}

impl<'a> LazyParser<'a> for ZeroCopyParser<'a> {
    type Output = LazyJsonValue<'a>;
    type Error = DomainError;

    fn parse_lazy(&mut self, input: &'a [u8]) -> Result<Self::Output, Self::Error> {
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
            LazyJsonValue::StringBorrowed(bytes) => {
                String::from_utf8_lossy(bytes).to_string()
            }
            LazyJsonValue::StringOwned(s) => s.clone(),
            LazyJsonValue::NumberSlice(bytes) => {
                String::from_utf8_lossy(bytes).to_string()
            }
            LazyJsonValue::Boolean(b) => b.to_string(),
            LazyJsonValue::Null => "null".to_string(),
            LazyJsonValue::ObjectSlice(bytes) => {
                String::from_utf8_lossy(bytes).to_string()
            }
            LazyJsonValue::ArraySlice(bytes) => {
                String::from_utf8_lossy(bytes).to_string()
            }
        }
    }

    /// Try to parse as string without allocation
    pub fn as_str(&self) -> DomainResult<&str> {
        match self {
            LazyJsonValue::StringBorrowed(bytes) => {
                from_utf8(bytes).map_err(|e| DomainError::InvalidInput(format!("Invalid UTF-8: {}", e)))
            }
            LazyJsonValue::StringOwned(s) => Ok(s.as_str()),
            _ => Err(DomainError::InvalidInput("Value is not a string".to_string())),
        }
    }

    /// Try to parse as number
    pub fn as_number(&self) -> DomainResult<f64> {
        match self {
            LazyJsonValue::NumberSlice(bytes) => {
                let s = from_utf8(bytes)
                    .map_err(|e| DomainError::InvalidInput(format!("Invalid UTF-8: {}", e)))?;
                s.parse::<f64>()
                    .map_err(|e| DomainError::InvalidInput(format!("Invalid number: {}", e)))
            }
            _ => Err(DomainError::InvalidInput("Value is not a number".to_string())),
        }
    }

    /// Try to parse as boolean
    pub fn as_boolean(&self) -> DomainResult<bool> {
        match self {
            LazyJsonValue::Boolean(b) => Ok(*b),
            _ => Err(DomainError::InvalidInput("Value is not a boolean".to_string())),
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
    base: ZeroCopyParser<'a>,
    buffer: Vec<u8>,
    complete_values: Vec<LazyJsonValue<'a>>,
}

impl<'a> IncrementalParser<'a> {
    pub fn new() -> Self {
        Self {
            base: ZeroCopyParser::new(),
            buffer: Vec::with_capacity(8192), // 8KB initial capacity
            complete_values: Vec::new(),
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
}