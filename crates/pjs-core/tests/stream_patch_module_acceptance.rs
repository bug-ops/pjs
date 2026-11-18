//! Acceptance tests for #751: `WirePatch` lives in the always-compiled
//! `stream::patch` module and keeps its wire form; the websocket module
//! re-exports the same type.

use pjson_rs::stream::patch::WirePatch;
use serde_json::json;

#[test]
fn wire_patch_round_trips_every_op_through_the_stream_patch_module() {
    let literals = [
        r#"["set",["a",0,"b.c"],{"k":[1,null]}]"#,
        r#"["merge",["o"],{"x":1}]"#,
        r#"["append",["l"],[1,"two"]]"#,
        r#"["append_str",["s"],"tail"]"#,
    ];
    for literal in literals {
        let patch: WirePatch = serde_json::from_str(literal).expect(literal);
        assert_eq!(serde_json::to_string(&patch).unwrap(), literal);
    }
}

#[test]
fn wire_patch_rejects_malformed_arrays() {
    let rows = [
        json!(["set", ["a"]]),
        json!(["set", ["a"], 1, 2]),
        json!(["merge", [], [1]]),
        json!(["replace", ["a"], 1]),
        json!(["set", [true], 1]),
    ];
    for row in rows {
        assert!(
            serde_json::from_value::<WirePatch>(row.clone()).is_err(),
            "{row}"
        );
    }
}

#[cfg(any(feature = "websocket-server", feature = "websocket-client"))]
#[test]
fn websocket_module_re_exports_the_same_wire_patch_type() {
    fn takes_stream_patch(patch: WirePatch) -> WirePatch {
        patch
    }
    let from_websocket: pjson_rs::infrastructure::websocket::WirePatch =
        serde_json::from_str(r#"["set",[],1]"#).unwrap();
    let same_type: WirePatch = takes_stream_patch(from_websocket);
    assert_eq!(
        serde_json::to_string(&same_type).unwrap(),
        r#"["set",[],1]"#
    );
}
