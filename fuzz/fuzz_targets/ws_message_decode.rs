#![no_main]

use libfuzzer_sys::fuzz_target;
use pjs_fuzz::approx_eq;
use pjson_rs::infrastructure::WsMessage;

fuzz_target!(|data: &[u8]| {
    let Ok(message) = serde_json::from_slice::<WsMessage>(data) else {
        return;
    };
    let value = serde_json::to_value(&message).expect("a decoded message serializes");
    let encoded = serde_json::to_vec(&message).expect("a decoded message serializes");
    let reparsed: WsMessage =
        serde_json::from_slice(&encoded).expect("a re-encoded message decodes");
    let reparsed_value = serde_json::to_value(&reparsed).expect("a decoded message serializes");
    assert!(
        approx_eq(&reparsed_value, &value),
        "round trip drifted: {value} vs {reparsed_value}"
    );
});
