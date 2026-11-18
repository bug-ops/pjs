//! Shared oracles for the fuzz targets.

use pjson_rs::Error;
use serde_json::Value;

/// The document after one serialize/parse cycle through the production float
/// path, i.e. `from_slice(to_vec(value))`.
///
/// A planner round trip performs exactly one such cycle, so this is the exact
/// expected value: it removes float re-parse drift from the oracle (#684).
///
/// # Panics
///
/// Panics if `value` cannot be serialized or its output cannot be parsed back,
/// which cannot happen for a `serde_json::Value`.
pub fn reparsed(value: &Value) -> Value {
    let bytes = serde_json::to_vec(value).expect("a Value serializes");
    serde_json::from_slice(&bytes).expect("serialized JSON parses")
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
    use serde_json::Number;

    use super::*;

    fn is_float_only(n: &Number) -> bool {
        !n.is_i64() && !n.is_u64()
    }

    fn num(f: f64) -> Value {
        Value::Number(Number::from_f64(f).expect("finite"))
    }

    #[test]
    fn reparsed_keeps_structure_and_integers() {
        let plain: Value = serde_json::from_str(r#"{"a":[1,"s",null],"b":-7}"#).unwrap();
        assert_eq!(reparsed(&plain), plain);
    }

    #[test]
    fn reparsed_pins_floats_to_the_production_parse_path() {
        assert_eq!(reparsed(&num(100.0)), num(100.0));
        let huge: Value = serde_json::from_str(&"1".repeat(199)).unwrap();
        let again = reparsed(&huge);
        let Value::Number(n) = &again else {
            panic!("a number must stay a number: {again:?}");
        };
        assert!(
            is_float_only(n),
            "199 digits overflow u64, so it is a float"
        );
        assert!(n.as_f64().is_some_and(f64::is_finite));
    }
}
