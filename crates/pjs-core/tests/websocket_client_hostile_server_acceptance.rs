//! Acceptance tests for the Rust WebSocket client against a scripted hostile
//! server: #718 (frame id monotonicity and framing consistency drop the
//! session) and #702 (a 409 keeps the session only for the exact
//! duplicate-session-id reply).
#![cfg(feature = "websocket-client")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use pjson_rs::domain::SessionId;
use pjson_rs::infrastructure::websocket::{PjsWebSocketClient, StreamOptions};

const SID: &str = "$SID";

fn merge_frame(frame_id: u32, payload: Value) -> Value {
    json!({"type": "StreamFrame", "data": {
        "session_id": SID, "frame_id": frame_id, "priority": 80,
        "payload": payload, "is_complete": false
    }})
}

fn patch_frame(frame_id: u32, key: &str) -> Value {
    json!({"type": "StreamPatch", "data": {
        "session_id": SID, "frame_id": frame_id, "priority": 80,
        "patches": [["set", [key], 1]], "is_complete": false
    }})
}

fn error_reply(code: u16, text: &str) -> Value {
    json!({"type": "Error", "data": {"session_id": SID, "error": text, "code": code}})
}

/// Reads the client's `StreamInit`, then sends `script` with `$SID` replaced by the adopted id.
async fn spawn_scripted_server(script: Vec<Value>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut ws = accept_async(tcp).await.expect("handshake");
        let Some(Ok(Message::Text(init))) = ws.next().await else {
            return;
        };
        let init: Value = serde_json::from_str(&init).expect("StreamInit");
        let id = init["data"]["session_id"]
            .as_str()
            .expect("session id")
            .to_owned();
        for message in script {
            let text = message.to_string().replace(SID, &id);
            if ws.send(Message::Text(text.into())).await.is_err() {
                return;
            }
        }
        while ws.next().await.is_some() {}
    });
    addr
}

struct Run {
    client: Arc<PjsWebSocketClient>,
    id: SessionId,
    connect: JoinHandle<pjson_rs::Result<()>>,
}

async fn run(options: StreamOptions, script: Vec<Value>) -> Run {
    let addr = spawn_scripted_server(script).await;
    let client = Arc::new(PjsWebSocketClient::new(format!("ws://{addr}")).expect("url"));
    let connect = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.connect().await }
    });
    let id = client
        .request_stream(json!({}), options)
        .await
        .expect("request");
    Run {
        client,
        id,
        connect,
    }
}

impl Run {
    async fn eventually(&self, expected: Option<Value>) -> bool {
        timeout(Duration::from_secs(2), async {
            while self.client.get_current_data(self.id).await.expect("lookup") != expected {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok()
    }

    async fn is_dropped(&self) -> bool {
        self.eventually(None).await
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.connect.abort();
    }
}

#[tokio::test]
async fn patch_session_is_dropped_on_duplicate_gap_or_wrong_first_frame_id() {
    let cases = [
        (
            "duplicate id",
            vec![patch_frame(0, "a"), patch_frame(0, "b")],
        ),
        ("gap", vec![patch_frame(0, "a"), patch_frame(2, "b")]),
        ("first id not zero", vec![patch_frame(7, "a")]),
        (
            "id going backwards",
            vec![
                patch_frame(0, "a"),
                patch_frame(1, "b"),
                patch_frame(0, "c"),
            ],
        ),
    ];
    for (name, script) in cases {
        let run = run(StreamOptions::patch(), script).await;
        assert!(run.is_dropped().await, "{name}: session must be dropped");
    }
}

#[tokio::test]
async fn merge_session_is_dropped_on_duplicate_gap_or_wrong_first_frame_id() {
    let cases = [
        (
            "duplicate id",
            vec![
                merge_frame(0, json!({"a": 1})),
                merge_frame(0, json!({"b": 1})),
            ],
        ),
        (
            "gap",
            vec![
                merge_frame(0, json!({"a": 1})),
                merge_frame(2, json!({"b": 1})),
            ],
        ),
        ("first id not zero", vec![merge_frame(5, json!({"a": 1}))]),
    ];
    for (name, script) in cases {
        let run = run(StreamOptions::default(), script).await;
        assert!(run.is_dropped().await, "{name}: session must be dropped");
    }
}

#[tokio::test]
async fn session_is_dropped_when_frame_kind_differs_from_requested_framing() {
    let merge_then_patch = vec![merge_frame(0, json!({"a": 1})), patch_frame(1, "b")];
    let patch_then_merge = vec![patch_frame(0, "a"), merge_frame(1, json!({"b": 1}))];

    let merge = run(StreamOptions::default(), merge_then_patch).await;
    assert!(merge.is_dropped().await, "patch frame on a merge session");

    let patch = run(StreamOptions::patch(), patch_then_merge).await;
    assert!(patch.is_dropped().await, "merge frame on a patch session");
}

#[tokio::test]
async fn conflict_reply_keeps_the_session_only_for_the_exact_duplicate_id_pair() {
    let marker = json!({"k": "v"});
    let exact = vec![
        error_reply(409, "session id already in use"),
        merge_frame(0, marker.clone()),
    ];
    let kept = run(StreamOptions::default(), exact).await;
    assert!(
        kept.eventually(Some(marker.clone())).await,
        "exact pair keeps the session"
    );

    let other_text = vec![
        error_reply(409, "some other conflict"),
        merge_frame(0, marker),
    ];
    let dropped = run(StreamOptions::default(), other_text).await;
    assert!(
        dropped.is_dropped().await,
        "409 with another text drops the session"
    );
}
