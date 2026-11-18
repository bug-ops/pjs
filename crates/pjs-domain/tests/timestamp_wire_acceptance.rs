//! Acceptance tests for #692 (chrono -> jiff): the RFC 3339 wire form of
//! timestamps stays `Z`-suffixed UTC, fraction digits are the shortest exact
//! form (no grouping by three), and `DomainEvent::metadata` renders `Z`.
//!
//! Driven through JSON only, so the assertions do not name the timestamp type.

use pjson_rs_domain::{DomainEvent, Frame};

const STREAM: &str = "22222222-2222-4222-8222-222222222222";
const SESSION: &str = "11111111-1111-4111-8111-111111111111";

fn frame_with(timestamp: &str) -> String {
    format!(
        r#"{{"stream_id":"{STREAM}","frame_type":"Complete","priority":100,"sequence":4,"timestamp":"{timestamp}","payload":{{}},"metadata":{{}}}}"#
    )
}

fn event_with(timestamp: &str) -> String {
    format!(
        r#"{{"event_type":"stream_created","session_id":"{SESSION}","stream_id":"{STREAM}","timestamp":"{timestamp}"}}"#
    )
}

fn reencoded_timestamp(timestamp: &str) -> String {
    let frame: Frame = serde_json::from_str(&frame_with(timestamp)).expect("frame decodes");
    let value = serde_json::to_value(&frame).expect("frame encodes");
    value["timestamp"]
        .as_str()
        .expect("timestamp string")
        .to_owned()
}

#[test]
fn frame_timestamp_wire_form_is_z_utc_with_shortest_fraction() {
    for (input, expected) in [
        ("2024-01-02T03:04:05Z", "2024-01-02T03:04:05Z"),
        ("2024-01-02T03:04:05.123Z", "2024-01-02T03:04:05.123Z"),
        ("2024-01-02T03:04:05.1234Z", "2024-01-02T03:04:05.1234Z"),
        ("2024-01-02T03:04:05.100Z", "2024-01-02T03:04:05.1Z"),
        (
            "2024-01-02T03:04:05.123456789Z",
            "2024-01-02T03:04:05.123456789Z",
        ),
        ("2024-01-02T05:04:05+02:00", "2024-01-02T03:04:05Z"),
        ("2024-01-02T03:04:05+00:00", "2024-01-02T03:04:05Z"),
    ] {
        assert_eq!(reencoded_timestamp(input), expected, "input {input}");
    }
}

#[test]
fn domain_event_metadata_timestamp_uses_z_suffix() {
    let event: DomainEvent =
        serde_json::from_str(&event_with("2024-01-02T03:04:05.5Z")).expect("event decodes");

    assert_eq!(event.metadata()["timestamp"], "2024-01-02T03:04:05.5Z");
}
