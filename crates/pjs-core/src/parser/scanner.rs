//! JSON scanning interface and common types

use crate::{Result, semantic::NumericDType};
use smallvec::SmallVec;

/// Main scanning interface implemented by SIMD and scalar scanners
pub trait JsonScanner {
    /// Scan JSON input and return structural information
    fn scan(&self, input: &[u8]) -> Result<ScanResult>;

    /// Check if this scanner supports SIMD operations
    fn supports_simd(&self) -> bool;

    /// Parse numeric array with SIMD optimization if available
    fn parse_numeric_array(
        &self,
        input: &[u8],
        dtype: NumericDType,
        length: Option<usize>,
    ) -> Result<crate::parser::JsonValue<'_>>;

    /// Find all string boundaries in the input
    fn find_strings(&self, input: &[u8]) -> Result<Vec<StringLocation>>;

    /// Find structural characters ({}[],:) positions
    fn find_structural_chars(&self, input: &[u8]) -> Result<Vec<usize>>;
}

/// Result of scanning JSON input
#[derive(Debug, Clone)]
pub struct ScanResult {
    /// Positions of structural characters
    pub structural_chars: Vec<usize>,
    /// String boundary positions
    pub string_bounds: SmallVec<[Range; 16]>,
    /// Number boundary positions  
    pub number_bounds: SmallVec<[Range; 16]>,
    /// Literal boundary positions (true/false/null)
    pub literal_bounds: SmallVec<[Range; 8]>,
    /// Detected root value type
    pub root_type: Option<crate::parser::ValueType>,
}

/// Range representing start and end positions
#[derive(Debug, Clone, Copy)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

/// String location with metadata
#[derive(Debug, Clone)]
pub struct StringLocation {
    /// Start position of string (after opening quote)
    pub start: usize,
    /// End position of string (before closing quote)
    pub end: usize,
    /// Whether string contains escape sequences
    pub has_escapes: bool,
    /// Estimated length after unescaping
    pub unescaped_len: Option<usize>,
}

impl ScanResult {
    /// Create new empty scan result
    pub fn new() -> Self {
        Self {
            structural_chars: Vec::new(),
            string_bounds: SmallVec::new(),
            number_bounds: SmallVec::new(),
            literal_bounds: SmallVec::new(),
            root_type: None,
        }
    }

    /// Determine the root JSON value type
    pub fn determine_root_type(&self) -> crate::parser::ValueType {
        if let Some(root_type) = self.root_type {
            return root_type;
        }

        // Simplified type detection
        if !self.string_bounds.is_empty() {
            crate::parser::ValueType::String
        } else if !self.number_bounds.is_empty() {
            crate::parser::ValueType::Number
        } else if !self.literal_bounds.is_empty() {
            crate::parser::ValueType::Boolean // or Null
        } else {
            crate::parser::ValueType::Object // Default
        }
    }

    /// Check if this appears to be a numeric array
    pub fn is_numeric_array(&self) -> bool {
        // Heuristic: starts with '[', has many numbers, few strings
        self.structural_chars
            .first()
            .is_some_and(|&c| c as u8 == b'[')
            && self.number_bounds.len() > 4
            && self.string_bounds.len() < 2
    }

    /// Check if this appears to be a table/object array
    pub fn is_table_like(&self) -> bool {
        // Heuristic: starts with '[', has balanced objects and strings
        self.structural_chars
            .first()
            .is_some_and(|&c| c as u8 == b'[')
            && self.count_object_starts() > 2
            && self.string_bounds.len() > self.number_bounds.len()
    }

    /// Count opening braces to estimate object count
    fn count_object_starts(&self) -> usize {
        self.structural_chars
            .iter()
            .filter(|&&pos| pos as u8 == b'{')
            .count()
    }
}

impl Default for ScanResult {
    fn default() -> Self {
        Self::new()
    }
}

impl Range {
    /// Create new range
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Get range length
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Check if range is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl StringLocation {
    /// Create new string location
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            has_escapes: false,
            unescaped_len: None,
        }
    }

    /// Create with escape information
    pub fn with_escapes(start: usize, end: usize, has_escapes: bool) -> Self {
        Self {
            start,
            end,
            has_escapes,
            unescaped_len: None,
        }
    }

    /// Get string length in bytes
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_result_creation() {
        let result = ScanResult::new();
        assert!(result.structural_chars.is_empty());
        assert!(result.string_bounds.is_empty());
    }

    #[test]
    fn test_range_operations() {
        let range = Range::new(10, 20);
        assert_eq!(range.len(), 10);
        assert!(!range.is_empty());
    }

    #[test]
    fn test_string_location() {
        let loc = StringLocation::new(5, 15);
        assert_eq!(loc.len(), 10);
        assert!(!loc.has_escapes);
    }
}
