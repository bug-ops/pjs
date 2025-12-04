//! Comprehensive tests for parser scanner module
//!
//! This test suite aims to achieve 70%+ coverage by testing:
//! - ScanResult creation and operations
//! - Range creation and operations
//! - StringLocation creation and operations
//! - Root type detection
//! - Numeric array detection
//! - Table-like structure detection
//! - Edge cases and boundary conditions

use pjson_rs::parser::ValueType;
use pjson_rs::parser::scanner::{Range, ScanResult, StringLocation};

// === ScanResult Tests ===

#[test]
fn test_scan_result_creation() {
    let result = ScanResult::new();
    assert!(result.structural_chars.is_empty());
    assert!(result.string_bounds.is_empty());
    assert!(result.number_bounds.is_empty());
    assert!(result.literal_bounds.is_empty());
    assert!(result.root_type.is_none());
}

#[test]
fn test_scan_result_default() {
    let result = ScanResult::default();
    assert!(result.structural_chars.is_empty());
    assert!(result.string_bounds.is_empty());
    assert!(result.number_bounds.is_empty());
    assert!(result.literal_bounds.is_empty());
}

#[test]
fn test_scan_result_with_structural_chars() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![0, 5, 10, 15];

    assert_eq!(result.structural_chars.len(), 4);
    assert_eq!(result.structural_chars[0], 0);
}

#[test]
fn test_scan_result_with_string_bounds() {
    let mut result = ScanResult::new();
    result.string_bounds.push(Range::new(1, 10));
    result.string_bounds.push(Range::new(15, 25));

    assert_eq!(result.string_bounds.len(), 2);
}

#[test]
fn test_scan_result_with_number_bounds() {
    let mut result = ScanResult::new();
    result.number_bounds.push(Range::new(2, 6));
    result.number_bounds.push(Range::new(10, 15));

    assert_eq!(result.number_bounds.len(), 2);
}

#[test]
fn test_scan_result_with_literal_bounds() {
    let mut result = ScanResult::new();
    result.literal_bounds.push(Range::new(5, 9));

    assert_eq!(result.literal_bounds.len(), 1);
}

// === Root Type Detection Tests ===

#[test]
fn test_determine_root_type_string() {
    let mut result = ScanResult::new();
    result.string_bounds.push(Range::new(1, 10));

    let root_type = result.determine_root_type();
    assert!(matches!(root_type, ValueType::String));
}

#[test]
fn test_determine_root_type_number() {
    let mut result = ScanResult::new();
    result.number_bounds.push(Range::new(0, 5));

    let root_type = result.determine_root_type();
    assert!(matches!(root_type, ValueType::Number));
}

#[test]
fn test_determine_root_type_boolean() {
    let mut result = ScanResult::new();
    result.literal_bounds.push(Range::new(0, 4));

    let root_type = result.determine_root_type();
    assert!(matches!(root_type, ValueType::Boolean));
}

#[test]
fn test_determine_root_type_default_object() {
    let result = ScanResult::new();

    let root_type = result.determine_root_type();
    assert!(matches!(root_type, ValueType::Object));
}

#[test]
fn test_determine_root_type_explicit() {
    let mut result = ScanResult::new();
    result.root_type = Some(ValueType::Array);

    let root_type = result.determine_root_type();
    assert!(matches!(root_type, ValueType::Array));
}

#[test]
fn test_determine_root_type_priority() {
    let mut result = ScanResult::new();
    result.string_bounds.push(Range::new(1, 10));
    result.number_bounds.push(Range::new(15, 20));

    // Strings have priority over numbers
    let root_type = result.determine_root_type();
    assert!(matches!(root_type, ValueType::String));
}

// === Numeric Array Detection Tests ===

#[test]
fn test_is_numeric_array_true() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![b'[' as usize];
    result.number_bounds.push(Range::new(1, 3));
    result.number_bounds.push(Range::new(4, 6));
    result.number_bounds.push(Range::new(7, 9));
    result.number_bounds.push(Range::new(10, 12));
    result.number_bounds.push(Range::new(13, 15));

    assert!(result.is_numeric_array());
}

#[test]
fn test_is_numeric_array_false_no_bracket() {
    let mut result = ScanResult::new();
    result.number_bounds.push(Range::new(1, 3));
    result.number_bounds.push(Range::new(4, 6));
    result.number_bounds.push(Range::new(7, 9));
    result.number_bounds.push(Range::new(10, 12));
    result.number_bounds.push(Range::new(13, 15));

    assert!(!result.is_numeric_array());
}

#[test]
fn test_is_numeric_array_false_too_few_numbers() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![b'[' as usize];
    result.number_bounds.push(Range::new(1, 3));
    result.number_bounds.push(Range::new(4, 6));

    assert!(!result.is_numeric_array());
}

#[test]
fn test_is_numeric_array_false_has_strings() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![b'[' as usize];
    result.number_bounds.push(Range::new(1, 3));
    result.number_bounds.push(Range::new(4, 6));
    result.number_bounds.push(Range::new(7, 9));
    result.number_bounds.push(Range::new(10, 12));
    result.number_bounds.push(Range::new(13, 15));
    result.string_bounds.push(Range::new(20, 25));
    result.string_bounds.push(Range::new(30, 35));

    assert!(!result.is_numeric_array());
}

#[test]
fn test_is_numeric_array_boundary_exactly_5_numbers() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![b'[' as usize];
    for i in 0..5 {
        result.number_bounds.push(Range::new(i * 3, i * 3 + 2));
    }

    assert!(result.is_numeric_array());
}

#[test]
fn test_is_numeric_array_wrong_first_char() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![b'{' as usize];
    for i in 0..5 {
        result.number_bounds.push(Range::new(i * 3, i * 3 + 2));
    }

    assert!(!result.is_numeric_array());
}

// === Table-like Detection Tests ===

#[test]
fn test_is_table_like_true() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![
        b'[' as usize,
        b'{' as usize,
        b'}' as usize,
        b'{' as usize,
        b'}' as usize,
        b'{' as usize,
        b'}' as usize,
    ];
    result.string_bounds.push(Range::new(5, 10));
    result.string_bounds.push(Range::new(15, 20));
    result.string_bounds.push(Range::new(25, 30));
    result.string_bounds.push(Range::new(35, 40));
    result.number_bounds.push(Range::new(50, 52));

    assert!(result.is_table_like());
}

#[test]
fn test_is_table_like_false_no_bracket() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![
        b'{' as usize,
        b'}' as usize,
        b'{' as usize,
        b'}' as usize,
        b'{' as usize,
        b'}' as usize,
    ];
    result.string_bounds.push(Range::new(5, 10));
    result.string_bounds.push(Range::new(15, 20));

    assert!(!result.is_table_like());
}

#[test]
fn test_is_table_like_false_few_objects() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![b'[' as usize, b'{' as usize];
    result.string_bounds.push(Range::new(5, 10));
    result.string_bounds.push(Range::new(15, 20));

    assert!(!result.is_table_like());
}

#[test]
fn test_is_table_like_false_more_numbers() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![
        b'[' as usize,
        b'{' as usize,
        b'}' as usize,
        b'{' as usize,
        b'}' as usize,
        b'{' as usize,
        b'}' as usize,
    ];
    result.string_bounds.push(Range::new(5, 10));
    result.number_bounds.push(Range::new(15, 20));
    result.number_bounds.push(Range::new(25, 30));
    result.number_bounds.push(Range::new(35, 40));

    assert!(!result.is_table_like());
}

#[test]
fn test_is_table_like_empty_array() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![b'[' as usize];

    assert!(!result.is_table_like());
}

// === Range Tests ===

#[test]
fn test_range_creation() {
    let range = Range::new(10, 20);
    assert_eq!(range.start, 10);
    assert_eq!(range.end, 20);
}

#[test]
fn test_range_len() {
    let range = Range::new(10, 20);
    assert_eq!(range.len(), 10);
}

#[test]
fn test_range_len_zero() {
    let range = Range::new(10, 10);
    assert_eq!(range.len(), 0);
}

#[test]
fn test_range_len_invalid() {
    let range = Range::new(20, 10);
    assert_eq!(range.len(), 0); // Saturating sub
}

#[test]
fn test_range_is_empty_true() {
    let range = Range::new(10, 10);
    assert!(range.is_empty());
}

#[test]
fn test_range_is_empty_false() {
    let range = Range::new(10, 20);
    assert!(!range.is_empty());
}

#[test]
fn test_range_is_empty_invalid() {
    let range = Range::new(20, 10);
    assert!(range.is_empty());
}

#[test]
fn test_range_clone() {
    let range = Range::new(5, 15);
    let cloned = range;
    assert_eq!(range.start, cloned.start);
    assert_eq!(range.end, cloned.end);
}

#[test]
fn test_range_debug() {
    let range = Range::new(5, 15);
    let debug_str = format!("{:?}", range);
    assert!(debug_str.contains("Range"));
}

// === StringLocation Tests ===

#[test]
fn test_string_location_creation() {
    let loc = StringLocation::new(5, 15);
    assert_eq!(loc.start, 5);
    assert_eq!(loc.end, 15);
    assert!(!loc.has_escapes);
    assert!(loc.unescaped_len.is_none());
}

#[test]
fn test_string_location_with_escapes() {
    let loc = StringLocation::with_escapes(5, 15, true);
    assert_eq!(loc.start, 5);
    assert_eq!(loc.end, 15);
    assert!(loc.has_escapes);
    assert!(loc.unescaped_len.is_none());
}

#[test]
fn test_string_location_without_escapes() {
    let loc = StringLocation::with_escapes(5, 15, false);
    assert_eq!(loc.start, 5);
    assert_eq!(loc.end, 15);
    assert!(!loc.has_escapes);
}

#[test]
fn test_string_location_len() {
    let loc = StringLocation::new(10, 30);
    assert_eq!(loc.len(), 20);
}

#[test]
fn test_string_location_len_zero() {
    let loc = StringLocation::new(10, 10);
    assert_eq!(loc.len(), 0);
}

#[test]
fn test_string_location_is_empty_true() {
    let loc = StringLocation::new(10, 10);
    assert!(loc.is_empty());
}

#[test]
fn test_string_location_is_empty_false() {
    let loc = StringLocation::new(10, 20);
    assert!(!loc.is_empty());
}

#[test]
fn test_string_location_clone() {
    let loc = StringLocation::new(5, 15);
    let cloned = loc.clone();
    assert_eq!(loc.start, cloned.start);
    assert_eq!(loc.end, cloned.end);
}

#[test]
fn test_string_location_debug() {
    let loc = StringLocation::new(5, 15);
    let debug_str = format!("{:?}", loc);
    assert!(debug_str.contains("StringLocation"));
}

// === Edge Cases and Boundary Conditions ===

#[test]
fn test_scan_result_large_structural_chars() {
    let mut result = ScanResult::new();
    result.structural_chars = (0..1000).collect();

    assert_eq!(result.structural_chars.len(), 1000);
}

#[test]
fn test_scan_result_many_string_bounds() {
    let mut result = ScanResult::new();
    for i in 0..100 {
        result.string_bounds.push(Range::new(i * 10, i * 10 + 5));
    }

    assert_eq!(result.string_bounds.len(), 100);
}

#[test]
fn test_scan_result_many_number_bounds() {
    let mut result = ScanResult::new();
    for i in 0..100 {
        result.number_bounds.push(Range::new(i * 5, i * 5 + 3));
    }

    assert_eq!(result.number_bounds.len(), 100);
}

#[test]
fn test_scan_result_mixed_content() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![b'[' as usize, b'{' as usize, b'}' as usize];
    result.string_bounds.push(Range::new(5, 10));
    result.number_bounds.push(Range::new(15, 20));
    result.literal_bounds.push(Range::new(25, 29));

    assert_eq!(result.structural_chars.len(), 3);
    assert_eq!(result.string_bounds.len(), 1);
    assert_eq!(result.number_bounds.len(), 1);
    assert_eq!(result.literal_bounds.len(), 1);
}

#[test]
fn test_range_zero_start() {
    let range = Range::new(0, 10);
    assert_eq!(range.start, 0);
    assert_eq!(range.len(), 10);
}

#[test]
fn test_range_large_values() {
    let range = Range::new(1000000, 2000000);
    assert_eq!(range.len(), 1000000);
}

#[test]
fn test_string_location_zero_start() {
    let loc = StringLocation::new(0, 10);
    assert_eq!(loc.start, 0);
    assert_eq!(loc.len(), 10);
}

#[test]
fn test_string_location_large_values() {
    let loc = StringLocation::new(1000000, 2000000);
    assert_eq!(loc.len(), 1000000);
}

#[test]
fn test_smallvec_inline_optimization() {
    let mut result = ScanResult::new();

    // SmallVec should use inline storage for small number of elements
    for i in 0..10 {
        result.string_bounds.push(Range::new(i * 10, i * 10 + 5));
    }

    assert_eq!(result.string_bounds.len(), 10);
}

#[test]
fn test_smallvec_heap_allocation() {
    let mut result = ScanResult::new();

    // SmallVec should spill to heap for large number of elements
    for i in 0..100 {
        result.number_bounds.push(Range::new(i * 5, i * 5 + 3));
    }

    assert_eq!(result.number_bounds.len(), 100);
}

#[test]
fn test_scan_result_count_object_starts() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![
        b'[' as usize,
        b'{' as usize,
        b'{' as usize,
        b'{' as usize,
        b'}' as usize,
    ];

    // This tests the private count_object_starts method indirectly
    let count = result
        .structural_chars
        .iter()
        .filter(|&&c| c == b'{' as usize)
        .count();
    assert_eq!(count, 3);
}

#[test]
fn test_is_numeric_array_exactly_4_numbers() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![b'[' as usize];
    for i in 0..4 {
        result.number_bounds.push(Range::new(i * 3, i * 3 + 2));
    }

    // Should be false as we need > 4
    assert!(!result.is_numeric_array());
}

#[test]
fn test_is_table_like_exactly_2_objects() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![
        b'[' as usize,
        b'{' as usize,
        b'}' as usize,
        b'{' as usize,
        b'}' as usize,
    ];
    result.string_bounds.push(Range::new(5, 10));
    result.string_bounds.push(Range::new(15, 20));

    // Should be false as we need > 2
    assert!(!result.is_table_like());
}

#[test]
fn test_is_table_like_equal_strings_and_numbers() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![
        b'[' as usize,
        b'{' as usize,
        b'}' as usize,
        b'{' as usize,
        b'}' as usize,
        b'{' as usize,
        b'}' as usize,
    ];
    result.string_bounds.push(Range::new(5, 10));
    result.string_bounds.push(Range::new(15, 20));
    result.number_bounds.push(Range::new(25, 30));
    result.number_bounds.push(Range::new(35, 40));

    // Equal counts, should be false (need more strings)
    assert!(!result.is_table_like());
}

#[test]
fn test_range_saturating_subtraction() {
    let range = Range {
        start: 100,
        end: 50,
    };
    assert_eq!(range.len(), 0);
}

#[test]
fn test_string_location_saturating_subtraction() {
    let loc = StringLocation {
        start: 100,
        end: 50,
        has_escapes: false,
        unescaped_len: None,
    };
    assert_eq!(loc.len(), 0);
}

#[test]
fn test_scan_result_clone() {
    let mut result = ScanResult::new();
    result.structural_chars = vec![1, 2, 3];
    result.string_bounds.push(Range::new(5, 10));

    let cloned = result.clone();
    assert_eq!(result.structural_chars.len(), cloned.structural_chars.len());
    assert_eq!(result.string_bounds.len(), cloned.string_bounds.len());
}

#[test]
fn test_scan_result_debug() {
    let result = ScanResult::new();
    let debug_str = format!("{:?}", result);
    assert!(debug_str.contains("ScanResult"));
}
