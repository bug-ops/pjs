//! Shared oracles for the fuzz targets.

use pjson_rs::Error;
use serde_json::{Number, Value};

/// Largest float drift tolerated by the oracle: the default (best-effort)
/// serde_json float parser may be 1 ULP off (#575).
const MAX_FLOAT_ULPS: u64 = 1;

fn is_float_only(n: &Number) -> bool {
    !n.is_i64() && !n.is_u64()
}

/// Maps the bit pattern of an `f64` to an unsigned integer that increases
/// monotonically with the float value, so adjacent floats differ by 1.
fn ordered_bits(f: f64) -> u64 {
    let bits = f.to_bits();
    if bits >> 63 == 1 {
        !bits
    } else {
        bits | (1 << 63)
    }
}

fn floats_within_ulps(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    ordered_bits(a).abs_diff(ordered_bits(b)) <= MAX_FLOAT_ULPS
}

fn numbers_approx_eq(a: &Number, b: &Number) -> bool {
    match (is_float_only(a), is_float_only(b), a.as_f64(), b.as_f64()) {
        (true, true, Some(x), Some(y)) => floats_within_ulps(x, y),
        _ => a == b,
    }
}

/// Structural equality that tolerates [`MAX_FLOAT_ULPS`] of drift on
/// floating-point numbers and is exact for everything else.
pub fn approx_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => numbers_approx_eq(x, y),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(l, r)| approx_eq(l, r))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, l)| y.get(k).is_some_and(|r| approx_eq(l, r)))
        }
        _ => a == b,
    }
}

/// Oracle for decompressing hostile bytes: every failure must be a typed
/// codec or bomb-protection error (`CompressionBomb` includes
/// `CompressedSizeExceeded`); anything else is a classification bug.
pub fn assert_decode_error<T>(result: Result<T, Error>) {
    if let Err(error) = result {
        assert!(
            matches!(error, Error::CompressionBomb(_) | Error::Codec { .. }),
            "unexpected decode error: {error:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(f: f64) -> Value {
        Value::Number(Number::from_f64(f).expect("finite"))
    }

    fn next_up(f: f64) -> f64 {
        f64::from_bits(f.to_bits() + 1)
    }

    #[test]
    fn one_ulp_passes() {
        let x = 985.6906946328695;
        assert!(approx_eq(&num(x), &num(next_up(x))));
        assert!(approx_eq(&num(-x), &num(-next_up(x))));
    }

    #[test]
    fn two_ulps_fail() {
        let x = 985.6906946328695;
        assert!(!approx_eq(&num(x), &num(next_up(next_up(x)))));
    }

    #[test]
    fn signed_zeros_are_equal() {
        assert!(approx_eq(&num(0.0), &num(-0.0)));
    }

    #[test]
    fn integers_and_structure_are_exact() {
        let a: Value = serde_json::from_str(r#"{"a":[1,2.5,"s"],"b":null}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"b":null,"a":[1,2.5,"s"]}"#).unwrap();
        let c: Value = serde_json::from_str(r#"{"a":[2,2.5,"s"],"b":null}"#).unwrap();
        assert!(approx_eq(&a, &b));
        assert!(!approx_eq(&a, &c));
    }
}
