#![no_main]

use libfuzzer_sys::fuzz_target;
use pjs_fuzz::reparsed;
use pjson_rs::infrastructure::WsMessage;

fuzz_target!(|data: &[u8]| {
    let Ok(message) = serde_json::from_slice::<WsMessage>(data) else {
        return;
    };
    let value = serde_json::to_value(&message).expect("a decoded message serializes");
    let encoded = serde_json::to_vec(&message).expect("a decoded message serializes");
    let decoded_again: WsMessage =
        serde_json::from_slice(&encoded).expect("a re-encoded message decodes");
    let reparsed_value =
        serde_json::to_value(&decoded_again).expect("a decoded message serializes");
    assert_eq!(reparsed_value, reparsed(&value), "round trip drifted");
});
