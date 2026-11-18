//! Acceptance tests for #624 over real sockets: negotiation of `framing`, the
//! 400/429 replies, legacy merge clients, the Rust client's `StreamPatch`
//! handling (session byte cap, depth cap, completion) and the `request_stream`
//! option defaults.
#![cfg(all(feature = "websocket-server", feature = "websocket-client"))]

use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use pjson_rs::domain::SessionId;
use pjson_rs::infrastructure::websocket::{
    AxumWebSocketTransport, PjsWebSocketClient, StreamOptions, WebSocketClientConfig,
    server::create_websocket_router,
};
use pjson_rs::security::RateLimitConfig;

mod wire_patch;
use wire_patch::{MAX_PATCH_PATH_DEPTH, apply_wire_patch};

type TestSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn spawn_server(transport: AxumWebSocketTransport) -> SocketAddr {
    let transport = Arc::new(transport);
    let app = create_websocket_router().with_state(transport);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    addr
}

fn ws_url(addr: SocketAddr) -> String {
    format!("ws://{addr}/ws")
}

async fn connect(addr: SocketAddr) -> TestSocket {
    connect_async(ws_url(addr)).await.expect("handshake").0
}

fn session_id(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

fn default_wire_options() -> Value {
    json!({
        "max_frame_size": 65536,
        "client_fps": null,
        "priority_mapping": null
    })
}

fn stream_init(id: &str, data: Value, options: Value) -> Message {
    Message::Text(
        json!({"type": "StreamInit", "data": {"session_id": id, "data": data, "options": options}})
            .to_string()
            .into(),
    )
}

async fn next_json<S>(ws: &mut S, wait: Duration) -> Option<Value>
where
    S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match timeout(wait, ws.next()).await {
            Err(_) | Ok(None) => return None,
            Ok(Some(Ok(Message::Text(text)))) => {
                return Some(serde_json::from_str(&text).expect("server sends JSON"));
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(error))) => panic!("socket error {error}"),
        }
    }
}

/// Reads every message of the session up to `StreamComplete`.
async fn collect_stream(ws: &mut TestSocket) -> Vec<Value> {
    let mut frames = Vec::new();
    loop {
        let message = next_json(ws, Duration::from_secs(20))
            .await
            .expect("stream ended without StreamComplete");
        match message["type"].as_str().expect("type") {
            "StreamComplete" => return frames,
            "Error" => panic!("unexpected error {message}"),
            _ => frames.push(message),
        }
    }
}

fn reconstruct_patches(frames: &[Value]) -> Value {
    let mut state = json!({});
    for frame in frames {
        assert_eq!(frame["type"], "StreamPatch", "{frame}");
        for op in frame["data"]["patches"].as_array().expect("patches") {
            apply_wire_patch(&mut state, op);
        }
    }
    state
}

fn wide_document() -> Value {
    json!({
        "entries": Value::Object(
            (0..300).map(|i| (format!("k{i:04}"), json!("x".repeat(500)))).collect()
        ),
        "series": (0..20_000).collect::<Vec<_>>(),
        "nested": {"a.b": {"[0]": ["deep"]}}
    })
}

// ---------------------------------------------------------------------------
// Server: negotiation and replies
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_stream_init_patch_framing_over_wire_reconstructs_document() {
    let addr = spawn_server(AxumWebSocketTransport::new()).await;
    let mut ws = connect(addr).await;
    let data = wide_document();
    let mut options = default_wire_options();
    options["framing"] = json!("patch");

    ws.send(stream_init(&session_id(1), data.clone(), options))
        .await
        .unwrap();
    let frames = collect_stream(&mut ws).await;

    assert!(frames.len() > 3, "{} frames", frames.len());
    for (i, frame) in frames.iter().enumerate() {
        assert_eq!(frame["data"]["frame_id"], json!(i));
        assert_eq!(frame["data"]["is_complete"], json!(i + 1 == frames.len()));
    }
    assert_eq!(reconstruct_patches(&frames), data);
}

#[tokio::test]
async fn test_stream_init_without_framing_over_wire_gets_merge_frames_only() {
    let addr = spawn_server(AxumWebSocketTransport::new()).await;
    let mut ws = connect(addr).await;
    let data = Value::Object(
        (0..400)
            .map(|i| (format!("k{i:04}"), json!("x".repeat(1000))))
            .collect(),
    );

    ws.send(stream_init(
        &session_id(2),
        data.clone(),
        default_wire_options(),
    ))
    .await
    .unwrap();
    let frames = collect_stream(&mut ws).await;

    assert!(frames.len() >= 5, "{} frames", frames.len());
    assert!(frames.iter().all(|f| f["type"] == "StreamFrame"));
    let last = frames.len() - 1;
    for (i, frame) in frames.iter().enumerate() {
        assert_eq!(frame["data"]["is_complete"], json!(i == last), "frame {i}");
    }
    let mut merged = serde_json::Map::new();
    for frame in &frames {
        merged.extend(
            frame["data"]["payload"]
                .as_object()
                .expect("object payload")
                .clone(),
        );
    }
    assert_eq!(Value::Object(merged), data);
}

#[tokio::test]
async fn test_stream_init_invalid_options_over_wire_replies_400_and_streams_nothing() {
    let addr = spawn_server(AxumWebSocketTransport::new()).await;
    let mut ws = connect(addr).await;
    let mut options = default_wire_options();
    options["max_frame_size"] = json!(512);

    ws.send(stream_init(&session_id(3), json!({"a": 1}), options))
        .await
        .unwrap();

    let reply = next_json(&mut ws, Duration::from_secs(5))
        .await
        .expect("an error reply");
    assert_eq!(reply["type"], "Error");
    assert_eq!(reply["data"]["code"], 400);
    assert_eq!(reply["data"]["error"], "invalid stream options");
    assert!(
        next_json(&mut ws, Duration::from_millis(300))
            .await
            .is_none()
    );
}

#[tokio::test]
async fn test_stream_init_zero_client_fps_over_wire_replies_400() {
    let addr = spawn_server(AxumWebSocketTransport::new()).await;
    let mut ws = connect(addr).await;
    let mut options = default_wire_options();
    options["client_fps"] = json!(0);

    ws.send(stream_init(&session_id(4), json!({"a": 1}), options))
        .await
        .unwrap();

    let reply = next_json(&mut ws, Duration::from_secs(5))
        .await
        .expect("reply");
    assert_eq!(reply["data"]["code"], 400);
    assert_eq!(reply["data"]["error"], "invalid stream options");
}

#[tokio::test]
async fn test_stream_init_connection_plan_budget_over_wire_replies_429_until_sessions_end() {
    let transport = AxumWebSocketTransport::with_rate_limit_config(RateLimitConfig {
        max_messages_per_second: 10_000,
        burst_allowance: 10_000,
        max_requests_per_window: 1_000_000,
        ..RateLimitConfig::default()
    });
    let addr = spawn_server(transport).await;
    let (mut sink, mut incoming) = connect(addr).await.split();
    let payload = json!({"b": "x".repeat(900_000)});
    let mut options = default_wire_options();
    options["framing"] = json!("patch");
    options["client_fps"] = json!(1);
    let total = 60_u64;

    // Send while reading: a client that stops reading for as long as the burst
    // takes would trip the server's write timeout on a loaded machine.
    let sender = tokio::spawn({
        let (payload, options) = (payload.clone(), options.clone());
        async move {
            for n in 0..total {
                sink.send(stream_init(
                    &session_id(100 + n),
                    payload.clone(),
                    options.clone(),
                ))
                .await
                .expect("send StreamInit");
            }
            sink
        }
    });
    // Every init resolves to exactly one first reply: an Error (rejected) or
    // its first StreamPatch (accepted). Waiting for all of them is bounded by
    // a generous deadline instead of a fixed silence window.
    let mut rejected = Vec::new();
    let mut accepted_ids = std::collections::HashSet::new();
    timeout(Duration::from_secs(240), async {
        while rejected.len() + accepted_ids.len() < total as usize {
            let message = next_json(&mut incoming, Duration::from_secs(60))
                .await
                .expect("the server closed before answering every StreamInit");
            let id = message["data"]["session_id"].clone();
            if message["type"] == "Error" {
                assert_ne!(message["data"]["code"], 413, "{message}");
                assert_eq!(message["data"]["code"], 429, "{message}");
                assert_eq!(message["data"]["error"], "connection plan budget exhausted");
                rejected.push(id);
            } else if message["type"] == "StreamPatch" {
                accepted_ids.insert(id.to_string());
            }
        }
    })
    .await
    .expect("every StreamInit gets a first reply");

    let mut sink = sender.await.expect("sender task");

    // Frames of accepted sessions are released progressively, so the exact
    // split depends on speed. Lower bound: admission only ever gets easier
    // with release, so at least the sessions that fit 16 MiB (about 18) are
    // accepted. Non-empty rejections: 60 x 0.9 MB exceeds the budget plus any
    // release a 1 fps session can achieve while the burst is sent.
    assert!(
        accepted_ids.len() >= 15,
        "{} sessions of ~0.9 MB must fit a 16 MiB connection budget",
        accepted_ids.len()
    );
    assert!(
        !rejected.is_empty(),
        "60 x 0.9 MB cannot fit a 16 MiB budget plus slow release"
    );

    for n in 0..total {
        sink.send(Message::Text(
            json!({"type": "StreamCancel", "data": {"session_id": session_id(100 + n)}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    }
    let fresh = session_id(999);
    sink.send(stream_init(&fresh, payload, options))
        .await
        .unwrap();
    let first_for_fresh = loop {
        let message = next_json(&mut incoming, Duration::from_secs(30))
            .await
            .expect("a reply for the fresh session");
        if message["data"]["session_id"] == json!(fresh) {
            break message;
        }
    };
    assert_eq!(
        first_for_fresh["type"], "StreamPatch",
        "cancelled sessions release the budget: {first_for_fresh}"
    );
}

fn large_inbound_transport() -> AxumWebSocketTransport {
    AxumWebSocketTransport::with_rate_limit_config(RateLimitConfig {
        max_frame_size: 64 * 1024 * 1024,
        ..RateLimitConfig::default()
    })
}

#[tokio::test]
async fn test_stream_init_payload_above_16_mib_is_split_instead_of_rejected() {
    let data = Value::Object(
        (0..20)
            .map(|i| (format!("k{i:02}"), json!("x".repeat(1_000_000))))
            .collect(),
    );
    for framing in ["merge", "patch"] {
        let addr = spawn_server(large_inbound_transport()).await;
        let mut ws = connect(addr).await;
        let mut options = default_wire_options();
        options["framing"] = json!(framing);
        options["max_frame_size"] = json!(2 * 1024 * 1024);

        ws.send(stream_init(&session_id(6), data.clone(), options))
            .await
            .unwrap();
        let frames = collect_stream(&mut ws).await;

        assert!(frames.len() >= 10, "{framing}: {} frames", frames.len());
        let rebuilt = if framing == "patch" {
            reconstruct_patches(&frames)
        } else {
            let mut merged = serde_json::Map::new();
            for frame in &frames {
                merged.extend(
                    frame["data"]["payload"]
                        .as_object()
                        .expect("object")
                        .clone(),
                );
            }
            Value::Object(merged)
        };
        assert_eq!(rebuilt, data, "{framing}");
    }
}

#[tokio::test]
async fn test_stream_init_merge_entry_above_16_mib_budget_replies_413() {
    let addr = spawn_server(large_inbound_transport()).await;
    let mut ws = connect(addr).await;
    let data = json!({"huge": "x".repeat(17 * 1024 * 1024), "small": 1});

    ws.send(stream_init(&session_id(7), data, default_wire_options()))
        .await
        .unwrap();

    let reply = next_json(&mut ws, Duration::from_secs(20))
        .await
        .expect("an error reply");
    assert_eq!(reply["type"], "Error");
    assert_eq!(reply["data"]["code"], 413);
    assert_eq!(
        reply["data"]["error"],
        "frame exceeds the outgoing byte budget"
    );
}

#[tokio::test]
async fn test_client_sent_stream_patch_is_ignored_and_connection_stays_open() {
    let addr = spawn_server(AxumWebSocketTransport::new()).await;
    let mut ws = connect(addr).await;
    let stray = json!({
        "type": "StreamPatch",
        "data": {
            "session_id": session_id(5),
            "frame_id": 0,
            "priority": 80,
            "patches": [["set", [], {"x": 1}]],
            "is_complete": true
        }
    });

    ws.send(Message::Text(stray.to_string().into()))
        .await
        .unwrap();

    assert!(
        next_json(&mut ws, Duration::from_millis(300))
            .await
            .is_none(),
        "a server-bound-only message gets no reply"
    );
    ws.send(Message::Ping(vec![7].into())).await.unwrap();
    let pong = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Pong(payload))) = ws.next().await {
                return payload;
            }
        }
    })
    .await
    .expect("the connection must stay open");
    assert_eq!(pong.as_ref(), [7]);
}

// ---------------------------------------------------------------------------
// Rust client against the in-tree server
// ---------------------------------------------------------------------------

async fn wait_until_complete(client: &PjsWebSocketClient, id: SessionId) {
    timeout(Duration::from_secs(20), async {
        while !client.is_stream_complete(id).await {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("stream must complete");
}

#[tokio::test]
async fn test_client_request_stream_default_options_reconstructs_multi_frame_patch_stream() {
    let addr = spawn_server(AxumWebSocketTransport::new()).await;
    let client = Arc::new(PjsWebSocketClient::new(ws_url(addr)).unwrap());
    let connect = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.connect().await }
    });
    let data = wide_document();

    let id = client
        .request_stream(data.clone(), StreamOptions::patch())
        .await
        .unwrap();
    wait_until_complete(&client, id).await;

    assert_eq!(client.get_current_data(id).await.unwrap(), Some(data));
    assert!(client.get_stream_stats(id).await.unwrap().total_frames > 3);
    connect.abort();
}

#[tokio::test]
async fn test_client_request_stream_explicit_default_options_uses_merge_framing() {
    let addr = spawn_server(AxumWebSocketTransport::new()).await;
    let client = Arc::new(PjsWebSocketClient::new(ws_url(addr)).unwrap());
    let connect = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.connect().await }
    });
    let data = wide_document();

    let id = client
        .request_stream(data.clone(), StreamOptions::default())
        .await
        .unwrap();
    wait_until_complete(&client, id).await;

    assert_eq!(client.get_current_data(id).await.unwrap(), Some(data));
    connect.abort();
}

/// Accepts connections and forwards every text message the client sends.
async fn spawn_spy_server() -> (SocketAddr, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let tx = tx.clone();
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(tcp).await else {
                    return;
                };
                while let Some(Ok(message)) = ws.next().await {
                    if let Message::Text(text) = message {
                        let _ = tx.send(text.to_string());
                    }
                }
            });
        }
    });
    (addr, rx)
}

#[tokio::test]
async fn test_client_request_stream_none_sends_patch_framing_and_some_sends_verbatim_options() {
    let (addr, mut sent) = spawn_spy_server().await;
    let client = Arc::new(PjsWebSocketClient::new(ws_url(addr)).unwrap());
    let connect = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.connect().await }
    });

    client
        .request_stream(json!({"a": 1}), StreamOptions::patch())
        .await
        .unwrap();
    client
        .request_stream(json!({"a": 2}), StreamOptions::default())
        .await
        .unwrap();

    let mut next_options = async || -> Value {
        let text = timeout(Duration::from_secs(5), sent.recv())
            .await
            .expect("client sends a StreamInit")
            .expect("spy open");
        serde_json::from_str::<Value>(&text).unwrap()["data"]["options"].clone()
    };
    let none_options = next_options().await;
    let default_options = next_options().await;
    assert_eq!(none_options["framing"], "patch");
    assert!(
        default_options.get("framing").is_none(),
        "{default_options}"
    );
    assert_eq!(default_options, default_wire_options());
    connect.abort();
}

// ---------------------------------------------------------------------------
// Rust client against a hostile server
// ---------------------------------------------------------------------------

/// Reads the client's `StreamInit`, then sends one `StreamPatch` per entry of
/// `frames` (`patches`, `is_complete`) on the adopted session id.
async fn spawn_scripted_server(frames: Vec<(Value, bool)>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut ws = tokio_tungstenite::accept_async(tcp)
            .await
            .expect("handshake");
        let Some(Ok(Message::Text(init))) = ws.next().await else {
            return;
        };
        let init: Value = serde_json::from_str(&init).expect("StreamInit");
        let id = init["data"]["session_id"].clone();
        for (frame_id, (patches, is_complete)) in frames.into_iter().enumerate() {
            let message = json!({
                "type": "StreamPatch",
                "data": {
                    "session_id": id,
                    "frame_id": frame_id,
                    "priority": 80,
                    "patches": patches,
                    "is_complete": is_complete
                }
            });
            if ws
                .send(Message::Text(message.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
        }
        while ws.next().await.is_some() {}
    });
    addr
}

struct ScriptedClient {
    client: Arc<PjsWebSocketClient>,
    id: SessionId,
    connect: tokio::task::JoinHandle<pjson_rs::Result<()>>,
}

async fn run_scripted(frames: Vec<(Value, bool)>, max_session_bytes: usize) -> ScriptedClient {
    let addr = spawn_scripted_server(frames).await;
    let client = Arc::new(PjsWebSocketClient::new(ws_url(addr)).unwrap().with_config(
        WebSocketClientConfig {
            max_session_bytes: NonZeroUsize::new(max_session_bytes).unwrap(),
            ..WebSocketClientConfig::default()
        },
    ));
    let connect = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.connect().await }
    });
    let id = client
        .request_stream(json!({}), StreamOptions::patch())
        .await
        .unwrap();
    ScriptedClient {
        client,
        id,
        connect,
    }
}

impl ScriptedClient {
    async fn eventually(&self, mut done: impl AsyncFnMut(&PjsWebSocketClient, SessionId) -> bool) {
        timeout(Duration::from_secs(5), async {
            while !done(&self.client, self.id).await {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("expected client state was not reached");
    }

    async fn assert_dropped(&self) {
        self.eventually(async |client, id| client.get_current_data(id).await.unwrap().is_none())
            .await;
    }

    fn finish(self) {
        self.connect.abort();
    }
}

#[tokio::test]
async fn test_client_applies_merge_patch_on_initial_empty_object_and_marks_complete() {
    let frames = vec![(json!([["merge", [], {"k": "v", "n": [1]}]]), true)];
    let scripted = run_scripted(frames, 1024 * 1024).await;

    scripted
        .eventually(async |client, id| client.is_stream_complete(id).await)
        .await;

    assert_eq!(
        scripted.client.get_current_data(scripted.id).await.unwrap(),
        Some(json!({"k": "v", "n": [1]}))
    );
    scripted.finish();
}

#[tokio::test]
async fn test_client_merge_patch_above_max_session_bytes_drops_session() {
    let frames = vec![
        (json!([["merge", [], {"k": "x".repeat(100)}]]), false),
        (json!([["merge", [], {"big": "y".repeat(3000)}]]), false),
    ];
    let scripted = run_scripted(frames, 2000).await;

    scripted.assert_dropped().await;
    scripted.finish();
}

#[tokio::test]
async fn test_client_merge_replacing_a_key_is_accounted_by_size_delta() {
    let rewrites: Vec<(Value, bool)> = (0..20)
        .map(|i| (json!([["merge", [], {"k": "x".repeat(1000 + i)}]]), false))
        .collect();
    let scripted = run_scripted(rewrites, 2000).await;

    scripted
        .eventually(async |client, id| {
            client
                .get_current_data(id)
                .await
                .unwrap()
                .is_some_and(|data| data["k"].as_str().is_some_and(|s| s.len() == 1019))
        })
        .await;

    scripted.finish();
}

#[tokio::test]
async fn test_client_append_str_growth_above_max_session_bytes_drops_session() {
    let mut frames = vec![(json!([["set", [], ""]]), false)];
    frames.extend((0..10).map(|_| (json!([["append_str", [], "z".repeat(500)]]), false)));
    let scripted = run_scripted(frames, 2000).await;

    scripted.assert_dropped().await;
    scripted.finish();
}

#[tokio::test]
async fn test_client_patch_path_deeper_than_cap_drops_session() {
    let nest: Vec<Value> = (1..=MAX_PATCH_PATH_DEPTH)
        .map(|n| {
            let path: Vec<Value> = (0..n).map(|i| json!(format!("k{i}"))).collect();
            json!(["set", path, {}])
        })
        .collect();
    let too_deep: Vec<Value> = (0..=MAX_PATCH_PATH_DEPTH)
        .map(|i| json!(format!("k{i}")))
        .collect();
    let frames = vec![
        (Value::Array(nest), false),
        (json!([["set", too_deep, 1]]), false),
    ];
    let scripted = run_scripted(frames, 1024 * 1024).await;

    scripted.assert_dropped().await;
    scripted.finish();
}

#[tokio::test]
async fn test_client_patch_with_unresolvable_path_drops_session() {
    let frames = vec![(json!([["set", ["missing", "child"], 1]]), false)];
    let scripted = run_scripted(frames, 1024 * 1024).await;

    scripted.assert_dropped().await;
    scripted.finish();
}
