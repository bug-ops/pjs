#![no_main]

use libfuzzer_sys::fuzz_target;
use pjson_rs::infrastructure::WsMessage;

fuzz_target!(|data: &[u8]| {
    let Ok(message) = serde_json::from_slice::<WsMessage>(data) else {
        return;
    };
    let value = serde_json::to_value(&message).expect("a decoded message serializes");
    let encoded = serde_json::to_vec(&message).expect("a decoded message serializes");
    let reparsed: WsMessage =
        serde_json::from_slice(&encoded).expect("a re-encoded message decodes");
    assert_eq!(
        serde_json::to_value(&reparsed).expect("a decoded message serializes"),
        value
    );
});
