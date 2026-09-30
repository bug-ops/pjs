//! Exact wire-byte savings models shared by the encoder and the analyzer.
//!
//! Every function here mirrors what [`super::SchemaCompressor`] emits for compact `serde_json`
//! output, so the analyzer's modelled saving equals the measured `wire_size` difference.

use super::{
    CompressionConfig, MAX_DECOMPRESSED_ELEMENTS, MAX_DELTA_ARRAY_SIZE, MAX_RLE_COUNT, as_integer,
};
use serde_json::Value as JsonValue;
use std::io;

/// Length of `{"delta_base":,"delta_type":"numeric_sequence"}` plus the extra array separator.
const DELTA_HEADER_LEN: usize = 48;
/// Length of `{"rle_value":,"rle_count":}`.
const RLE_ENVELOPE_LEN: usize = 27;

struct ByteCount(usize);

impl io::Write for ByteCount {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Number of base-10 digits in `n`'s decimal representation (`0` has 1 digit).
pub(super) fn decimal_digits(n: u64) -> usize {
    n.checked_ilog10().map_or(1, |d| d as usize + 1)
}

fn json_len(value: &JsonValue) -> usize {
    let mut counter = ByteCount(0);
    // Serializing a `Value` into an infallible writer cannot fail.
    let _ = serde_json::to_writer(&mut counter, value);
    counter.0
}

/// Structural equality that distinguishes `0.0` from `-0.0`, so run-length encoding preserves
/// the sign of zero.
fn same_value(a: &JsonValue, b: &JsonValue) -> bool {
    match (a, b) {
        (JsonValue::Number(x), JsonValue::Number(y)) if x.is_f64() && y.is_f64() => {
            x.as_f64().map(f64::to_bits) == y.as_f64().map(f64::to_bits)
        }
        (JsonValue::Array(x), JsonValue::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_value(p, q))
        }
        (JsonValue::Object(x), JsonValue::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, p)| y.get(k).is_some_and(|q| same_value(p, q)))
        }
        _ => a == b,
    }
}

/// Whether an array of `len` elements may be run-length encoded (the decoder caps expansion).
fn rle_len_within_cap(len: usize) -> bool {
    len <= MAX_DECOMPRESSED_ELEMENTS
}

/// Whether an array of `len` elements plus the delta header fits the decoder cap.
fn delta_len_within_cap(len: usize) -> bool {
    len < MAX_DELTA_ARRAY_SIZE
}

/// Runs of equal consecutive values as `(value, count)`, each `count <= MAX_RLE_COUNT`.
///
/// Returns `None` for arrays longer than `MAX_DECOMPRESSED_ELEMENTS`, which must stay raw
/// because the decoder caps the expanded element count per array.
pub(super) fn rle_runs(
    arr: &[JsonValue],
) -> Option<impl Iterator<Item = (&JsonValue, usize)> + '_> {
    rle_len_within_cap(arr.len()).then(|| {
        arr.chunk_by(same_value).flat_map(|chunk| {
            chunk
                .chunks(MAX_RLE_COUNT)
                .map(move |piece| (&chunk[0], piece.len()))
        })
    })
}

/// Bytes saved by encoding a run of `count` copies of `value` as one RLE object, if positive.
pub(super) fn run_saving(value: &JsonValue, count: usize) -> Option<usize> {
    if count < 2 {
        return None;
    }
    let gross = (count - 1).saturating_mul(json_len(value) + 1);
    gross
        .checked_sub(RLE_ENVELOPE_LEN + decimal_digits(count as u64))
        .filter(|&saving| saving > 0)
}

/// Total RLE saving the encoder achieves on `arr`.
pub(super) fn rle_net_saving(arr: &[JsonValue]) -> usize {
    rle_runs(arr).map_or(0, |runs| {
        runs.filter_map(|(value, count)| run_saving(value, count))
            .sum()
    })
}

/// A profitable, lossless delta encoding of an all-integer array.
pub(super) struct DeltaPlan<'a> {
    /// The minimum element, emitted verbatim in the delta header.
    pub(super) base: &'a JsonValue,
    base_int: i128,
    /// Bytes saved on the wire, always positive.
    pub(super) saving: usize,
}

impl DeltaPlan<'_> {
    /// Offset of `value` from the base, `None` if it is not a non-negative `u64` offset.
    pub(super) fn delta_of(&self, value: &JsonValue) -> Option<u64> {
        u64::try_from(as_integer(value)? - self.base_int).ok()
    }
}

/// Plans delta encoding for `arr`: every element must be an integer whose offset from the
/// minimum fits in `u64`, and the encoding must shrink the wire size.
pub(super) fn delta_plan<'a>(
    arr: &'a [JsonValue],
    config: &CompressionConfig,
) -> Option<DeltaPlan<'a>> {
    if arr.len() < config.min_numeric_sequence_size.max(3) || !delta_len_within_cap(arr.len()) {
        return None;
    }
    let mut min: Option<(i128, &JsonValue)> = None;
    for value in arr {
        let n = as_integer(value)?;
        if min.is_none_or(|(m, _)| n < m) {
            min = Some((n, value));
        }
    }
    let (base_int, base) = min?;
    let plan = DeltaPlan {
        base,
        base_int,
        saving: 0,
    };

    let mut original = 0usize;
    let mut delta = 0usize;
    for value in arr {
        original += json_len(value);
        delta += decimal_digits(plan.delta_of(value)?);
    }
    let saving = original
        .checked_sub(delta + DELTA_HEADER_LEN + json_len(base))
        .filter(|&s| s > 0)?;
    Some(DeltaPlan { saving, ..plan })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_apply_at_exact_boundaries() {
        assert!(rle_len_within_cap(MAX_DECOMPRESSED_ELEMENTS));
        assert!(!rle_len_within_cap(MAX_DECOMPRESSED_ELEMENTS + 1));
        assert!(delta_len_within_cap(MAX_DELTA_ARRAY_SIZE - 1));
        assert!(!delta_len_within_cap(MAX_DELTA_ARRAY_SIZE));
    }

    #[test]
    fn rle_runs_never_exceed_max_count_and_cover_the_array() {
        let arr = vec![JsonValue::from(0); MAX_RLE_COUNT * 2 + 5];
        let counts: Vec<usize> = rle_runs(&arr).unwrap().map(|(_, c)| c).collect();
        assert_eq!(counts, [MAX_RLE_COUNT, MAX_RLE_COUNT, 5]);
    }

    #[test]
    fn same_value_separates_signed_zeros() {
        let pos = JsonValue::from(0.0);
        let neg = JsonValue::from(-0.0);
        assert!(!same_value(&pos, &neg));
        assert!(same_value(&pos, &JsonValue::from(0.0)));
        assert!(!same_value(
            &serde_json::json!([0.0]),
            &serde_json::json!([-0.0])
        ));
        assert!(!same_value(&JsonValue::from(0), &pos));
    }

    #[test]
    fn run_saving_matches_serialized_lengths() {
        let value = JsonValue::from("x".repeat(20));
        let count = 50;
        let raw = serde_json::to_string(&vec![value.clone(); count])
            .unwrap()
            .len();
        let rle = serde_json::to_string(&[serde_json::json!({
            "rle_value": value,
            "rle_count": count
        })])
        .unwrap()
        .len();
        assert_eq!(run_saving(&value, count), Some(raw - rle));
        assert_eq!(run_saving(&JsonValue::from(1), 3), None);
    }

    #[test]
    fn run_saving_of_exactly_zero_is_none() {
        assert_eq!(run_saving(&JsonValue::from("x".repeat(25)), 2), None);
        assert_eq!(run_saving(&JsonValue::from("x".repeat(26)), 2), Some(1));
        assert_eq!(run_saving(&JsonValue::from("x".repeat(100)), 1), None);
    }

    #[test]
    fn delta_plan_requires_three_elements_even_with_lower_config() {
        let config = CompressionConfig {
            min_numeric_sequence_size: 1,
            ..CompressionConfig::default()
        };
        let arr = [
            JsonValue::from(1_000_000_000_000u64),
            JsonValue::from(1_000_000_000_001u64),
        ];
        assert!(delta_plan(&arr, &config).is_none());
    }
}
