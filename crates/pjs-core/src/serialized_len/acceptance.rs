//! Acceptance tests for #670: `json_len` and `json_str_len` equal the length of
//! `serde_json::to_vec` for every value, without allocating the bytes.

use super::{json_len, json_str_len};
use serde_json::{Value, json};

fn strings() -> Vec<String> {
    vec![
        String::new(),
        "plain".into(),
        "quote\" backslash\\ slash/".into(),
        "\u{0}\u{1}\u{8}\u{9}\u{a}\u{c}\u{d}\u{1f}".into(),
        "\u{7f} del".into(),
        "caf\u{e9} \u{4e16}\u{754c}".into(),
        "emoji \u{1f600} \u{1f9d1}\u{200d}\u{1f4bb}".into(),
        "\u{2028}\u{2029}".into(),
        "x".repeat(10_000),
        "\u{1}".repeat(500),
        (0..=0x7f_u8).map(char::from).collect(),
        "\u{1}\u{1}\u{1}\u{1}\"\"\"\"\u{e9}\u{1f600}\n\n".into(),
        "\u{7ff}\u{800}\u{ffff}\u{10000}\u{10ffff}\"\u{e9}\\".into(),
    ]
}

fn documents() -> Vec<Value> {
    let mut docs = vec![
        json!(null),
        json!(true),
        json!(false),
        json!(0),
        json!(-1),
        json!(i64::MIN),
        json!(u64::MAX),
        json!(0.1),
        json!(-1.5e300),
        json!(1.0e-7),
        json!(123_456_789.123_456_79),
        json!([]),
        json!({}),
        json!([[], {}, [[]], {"": ""}]),
        json!({"a": {"b": {"c": [1, 2, {"d": null}]}}, "k\"ey": "v\n"}),
        json!({"\u{e9}": "\u{1f600}", "\u{1}": [0.5, -0.5]}),
    ];
    docs.extend(strings().into_iter().map(Value::String));
    docs.push(Value::Array(
        (0..300)
            .map(|i| json!({"id": i, "label": format!("item-{i}"), "ratio": f64::from(i) / 7.0}))
            .collect(),
    ));
    docs
}

#[test]
fn test_json_len_equals_to_vec_len_for_every_value() {
    for document in documents() {
        let expected = serde_json::to_vec(&document).expect("serializable").len();

        assert_eq!(json_len(&document), expected, "{document}");
    }
}

#[test]
fn test_json_str_len_equals_to_vec_len_for_every_string() {
    for s in strings() {
        let expected = serde_json::to_vec(&s).expect("serializable").len();

        assert_eq!(json_str_len(&s), expected, "{s:?}");
    }
}

#[test]
fn test_json_len_of_a_string_value_matches_json_str_len() {
    for s in strings() {
        assert_eq!(json_len(&Value::String(s.clone())), json_str_len(&s));
    }
}

#[test]
fn test_json_str_len_equals_to_vec_len_for_every_char() {
    for c in (0..=0xd7ff_u32)
        .chain(0xe000..=0xffff)
        .filter_map(char::from_u32)
    {
        let s = format!("a{c}\"{c}");
        let expected = serde_json::to_vec(&s).expect("serializable").len();

        assert_eq!(json_str_len(&s), expected, "{c:?}");
    }
}

#[test]
fn test_decimal_digits_counts_base_10_digits() {
    use super::decimal_digits;
    for (n, digits) in [(0, 1), (9, 1), (10, 2), (99, 2), (100, 3), (u64::MAX, 20)] {
        assert_eq!(decimal_digits(n), digits, "{n}");
    }
}
