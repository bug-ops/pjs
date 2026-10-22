#![no_main]

use libfuzzer_sys::fuzz_target;
use pjson_rs::parser::{JiterPartialParser, PartialJsonParser};

fuzz_target!(|data: &[u8]| {
    if let Ok(result) = JiterPartialParser::default().parse_partial(data) {
        assert!(result.consumed <= data.len());
    }
});
