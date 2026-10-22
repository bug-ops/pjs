#![no_main]

use libfuzzer_sys::fuzz_target;
use pjson_rs::parser::zero_copy::{LazyParser, ZeroCopyParser};

fuzz_target!(|data: &[u8]| {
    let mut parser = ZeroCopyParser::new();
    let _ = parser.parse_lazy(data);
});
