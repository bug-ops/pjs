//! Acceptance tests for #624 at the controller level: priority planning,
//! `max_frame_size` splitting (merge and patch framing), plan limits, the
//! per-connection plan budget, pacing and the stall/throughput deadline.
//!
//! The controller is driven through its public API over an in-memory byte
//! bounded channel, with paused tokio time, so the tests are deterministic.
//! Patch content is checked on the JSON wire form with a reference applier.
#![cfg(feature = "websocket-server")]

use std::collections::HashMap;
use std::time::Duration;

use pjson_rs::domain::{Priority, SessionId};
use pjson_rs::error::{PlanError, StreamOptionsError};
use pjson_rs::infrastructure::bounded_channel::{Envelope, byte_bounded_channel};
use pjson_rs::infrastructure::websocket::{
    AdaptiveStreamController, ConnectionId, CreateSessionError, Framing, StreamOptions,
};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc::Receiver;
use tokio::time::{Instant, sleep, timeout};

mod wire_patch;
use wire_patch::apply_wire_patch;

const MIB: usize = 1024 * 1024;

struct Harness {
    controller: AdaptiveStreamController,
    owner: ConnectionId,
    rx: Receiver<Envelope<String>>,
}

impl Harness {
    fn new() -> Self {
        Self::with_channel(1000, 16 * MIB)
    }

    fn with_channel(items: usize, bytes: usize) -> Self {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let (tx, rx) = byte_bounded_channel::<String>(items, bytes);
        controller.attach_connection(owner, tx);
        Self {
            controller,
            owner,
            rx,
        }
    }

    async fn start(
        &self,
        id: SessionId,
        data: Value,
        options: StreamOptions,
    ) -> Result<(), CreateSessionError> {
        self.controller
            .create_session(id, self.owner, data, options)
            .await?;
        self.controller
            .start_streaming(self.owner, id)
            .expect("session exists");
        Ok(())
    }

    /// Streams `data` to completion and returns every message up to and
    /// including `StreamComplete`.
    async fn stream_all(&mut self, data: Value, options: StreamOptions) -> Plan {
        let id = SessionId::new();
        self.start(id, data, options)
            .await
            .expect("session is accepted");
        let mut texts = Vec::new();
        loop {
            let envelope = timeout(Duration::from_secs(600), self.rx.recv())
                .await
                .expect("stream stalled in the test")
                .expect("channel closed");
            let text = envelope.into_inner();
            let kind = message_type(&text);
            assert_ne!(kind, "Error", "unexpected error message: {text}");
            texts.push(text);
            if kind == "StreamComplete" {
                return Plan::new(texts);
            }
        }
    }
}

fn message_type(text: &str) -> String {
    serde_json::from_str::<Value>(text).expect("server sends JSON")["type"]
        .as_str()
        .expect("type tag")
        .to_owned()
}

struct Plan {
    texts: Vec<String>,
    /// Every message except the trailing `StreamComplete`.
    frames: Vec<Value>,
}

impl Plan {
    fn new(texts: Vec<String>) -> Self {
        let mut messages: Vec<Value> = texts
            .iter()
            .map(|t| serde_json::from_str(t).expect("server sends JSON"))
            .collect();
        let last = messages.pop().expect("at least StreamComplete");
        assert_eq!(last["type"], "StreamComplete");
        Self {
            texts,
            frames: messages,
        }
    }

    fn frame_bytes(&self) -> usize {
        self.texts[..self.frames.len()]
            .iter()
            .map(String::len)
            .sum()
    }

    fn priorities(&self) -> Vec<u64> {
        self.frames
            .iter()
            .map(|f| f["data"]["priority"].as_u64().expect("priority"))
            .collect()
    }

    /// Frame ids run 0..n-1 and only the last frame completes the stream.
    fn assert_sequence(&self, expected_type: &str) {
        assert!(!self.frames.is_empty());
        let last = self.frames.len() - 1;
        for (i, frame) in self.frames.iter().enumerate() {
            assert_eq!(frame["type"], expected_type, "frame {i}");
            assert_eq!(frame["data"]["frame_id"], json!(i), "frame {i}");
            assert_eq!(frame["data"]["is_complete"], json!(i == last), "frame {i}");
        }
    }

    fn assert_each_message_at_most(&self, max: usize) {
        for (i, text) in self.texts[..self.frames.len()].iter().enumerate() {
            assert!(
                text.len() <= max,
                "frame {i} is {} bytes > {max}",
                text.len()
            );
        }
    }

    fn patch_ops(&self) -> Vec<&Value> {
        self.frames
            .iter()
            .flat_map(|f| f["data"]["patches"].as_array().expect("patches").iter())
            .collect()
    }

    /// Applies every patch in order to `{}`.
    fn apply_patches(&self) -> Value {
        let mut state = json!({});
        for op in self.patch_ops() {
            apply_wire_patch(&mut state, op);
        }
        state
    }

    /// Shallow-merges every `StreamFrame` payload in order into `{}`.
    fn apply_merge_frames(&self) -> Value {
        let mut state = json!({});
        for frame in &self.frames {
            let payload = &frame["data"]["payload"];
            match (state.as_object_mut(), payload.as_object()) {
                (Some(target), Some(entries)) => {
                    target.extend(entries.iter().map(|(k, v)| (k.clone(), v.clone())));
                }
                _ => state = payload.clone(),
            }
        }
        state
    }
}

fn options(framing: Framing, max_frame_size: usize) -> StreamOptions {
    StreamOptions {
        framing,
        max_frame_size,
        ..StreamOptions::default()
    }
}

fn entries(count: usize, value_len: usize) -> Value {
    Value::Object(
        (0..count)
            .map(|i| (format!("k{i:04}"), json!("x".repeat(value_len))))
            .collect(),
    )
}

fn priority(value: u8) -> Priority {
    Priority::new(value).expect("valid priority")
}

// ---------------------------------------------------------------------------
// Patch framing
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_small_object_round_trips() {
    let data = json!({"a": 1, "b": [1, 2], "c": {"d": "x"}});

    let plan = Harness::new()
        .stream_all(data.clone(), options(Framing::Patch, 64 * 1024))
        .await;

    plan.assert_sequence("StreamPatch");
    assert_eq!(plan.apply_patches(), data);
}

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_object_above_max_frame_size_splits_into_bounded_frames() {
    let data = entries(200, 100);

    let plan = Harness::new()
        .stream_all(data.clone(), options(Framing::Patch, 4096))
        .await;

    assert!(plan.frames.len() > 4, "{} frames", plan.frames.len());
    plan.assert_sequence("StreamPatch");
    plan.assert_each_message_at_most(4096);
    assert_eq!(plan.apply_patches(), data);
}

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_object_root_uses_merge_batches_without_root_set() {
    let data = entries(200, 100);

    let plan = Harness::new()
        .stream_all(data, options(Framing::Patch, 4096))
        .await;

    let ops = plan.patch_ops();
    assert!(
        ops.iter().any(|op| op[0] == "merge" && op[1] == json!([])),
        "an object root is delivered by Merge([], ..) batches"
    );
    assert!(
        !ops.iter().any(|op| op[0] == "set" && op[1] == json!([])),
        "no root Set for an object root"
    );
}

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_empty_object_and_scalar_roots_are_one_root_set() {
    for data in [json!({}), json!(42), json!("s"), json!(null), json!(true)] {
        let plan = Harness::new()
            .stream_all(data.clone(), options(Framing::Patch, 4096))
            .await;

        assert_eq!(plan.frames.len(), 1, "{data}");
        assert_eq!(plan.patch_ops(), vec![&json!(["set", [], data.clone()])]);
        assert_eq!(plan.apply_patches(), data);
    }
}

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_array_roots_round_trip() {
    for data in [
        json!([]),
        json!([1, 2, 3]),
        json!((0..3000).collect::<Vec<_>>()),
    ] {
        let plan = Harness::new()
            .stream_all(data.clone(), options(Framing::Patch, 2048))
            .await;

        plan.assert_each_message_at_most(2048);
        assert_eq!(plan.apply_patches(), data);
    }
}

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_huge_string_is_split_at_char_boundaries() {
    let data = json!({"blob": "\u{e9}\"\\\n\u{1f600}".repeat(5000)});

    let plan = Harness::new()
        .stream_all(data.clone(), options(Framing::Patch, 2048))
        .await;

    assert!(plan.patch_ops().iter().any(|op| op[0] == "append_str"));
    plan.assert_each_message_at_most(2048);
    assert_eq!(plan.apply_patches(), data);
}

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_large_array_uses_append_batches() {
    let data = json!({"items": (0..5000).collect::<Vec<_>>()});

    let plan = Harness::new()
        .stream_all(data.clone(), options(Framing::Patch, 4096))
        .await;

    assert!(plan.patch_ops().iter().any(|op| op[0] == "append"));
    plan.assert_each_message_at_most(4096);
    assert_eq!(plan.apply_patches(), data);
}

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_array_of_oversized_elements_recurses_into_them() {
    let rows: Vec<Value> = (0..5)
        .map(|i| json!({"id": i, "text": "x".repeat(3000)}))
        .collect();
    let data = json!({"rows": rows});

    let plan = Harness::new()
        .stream_all(data.clone(), options(Framing::Patch, 2048))
        .await;

    plan.assert_each_message_at_most(2048);
    assert_eq!(plan.apply_patches(), data);
}

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_deeply_nested_document_round_trips() {
    let mut data = json!({"leaf": "x".repeat(3000), "n": 1});
    for depth in 0..40 {
        data = json!({"level": depth, "pad": "p".repeat(200), "child": data});
    }

    let plan = Harness::new()
        .stream_all(data.clone(), options(Framing::Patch, 2048))
        .await;

    plan.assert_each_message_at_most(2048);
    assert_eq!(plan.apply_patches(), data);
}

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_keys_with_dots_brackets_and_empty_round_trip() {
    let data = json!({
        "a.b": {"[0]": ["x", {"c]": 1}]},
        "": {"": 2},
        "z[": "y".repeat(5000),
        "0": {"0": [0]}
    });

    let plan = Harness::new()
        .stream_all(data.clone(), options(Framing::Patch, 1024))
        .await;

    plan.assert_each_message_at_most(1024);
    assert_eq!(plan.apply_patches(), data);
}

#[tokio::test(start_paused = true)]
async fn test_create_session_patch_990kb_wide_object_plans_in_few_frames_without_rejection() {
    let items: Map<String, Value> = (0..90_000)
        .map(|i| (format!("k{i:05}"), json!(0)))
        .collect();
    let data = json!({"response": {"payload": {"items": items}}});
    let data_len = serde_json::to_string(&data).unwrap().len();
    assert!(data_len > 980_000 && data_len < 1_000_000, "{data_len}");

    let plan = Harness::new()
        .stream_all(
            data.clone(),
            StreamOptions {
                framing: Framing::Patch,
                ..StreamOptions::default()
            },
        )
        .await;

    assert!(
        (15..=24).contains(&plan.frames.len()),
        "{} frames",
        plan.frames.len()
    );
    assert!(
        plan.frame_bytes() as f64 <= 1.1 * data_len as f64,
        "plan is {} bytes for {data_len} bytes of data",
        plan.frame_bytes()
    );
    let merged_into_items: usize = plan
        .patch_ops()
        .iter()
        .filter(|op| op[0] == "merge" && op[1] == json!(["response", "payload", "items"]))
        .map(|op| op[2].as_object().unwrap().len())
        .sum();
    assert_eq!(merged_into_items, 90_000);
    assert_eq!(plan.apply_patches(), data);
}

// ---------------------------------------------------------------------------
// Merge framing (legacy)
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn test_create_session_merge_small_object_is_one_complete_frame() {
    let data = json!({"a": 1});

    let plan = Harness::new()
        .stream_all(data.clone(), StreamOptions::default())
        .await;

    assert_eq!(plan.frames.len(), 1);
    plan.assert_sequence("StreamFrame");
    assert_eq!(plan.apply_merge_frames(), data);
}

#[tokio::test(start_paused = true)]
async fn test_create_session_merge_object_above_max_frame_size_splits_by_top_level_key() {
    let data = entries(100, 300);

    let plan = Harness::new()
        .stream_all(data.clone(), options(Framing::Merge, 2048))
        .await;

    assert!(plan.frames.len() > 10, "{} frames", plan.frames.len());
    plan.assert_sequence("StreamFrame");
    plan.assert_each_message_at_most(2048);
    assert_eq!(plan.apply_merge_frames(), data);
}

#[tokio::test(start_paused = true)]
async fn test_create_session_merge_single_entry_above_max_frame_size_travels_alone() {
    let data = json!({"big": "x".repeat(10_000), "s": 1});

    let plan = Harness::new()
        .stream_all(data.clone(), options(Framing::Merge, 2048))
        .await;

    plan.assert_sequence("StreamFrame");
    let carrying_big: Vec<&Value> = plan
        .frames
        .iter()
        .filter(|f| f["data"]["payload"].get("big").is_some())
        .collect();
    assert_eq!(carrying_big.len(), 1);
    assert_eq!(
        carrying_big[0]["data"]["payload"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(plan.apply_merge_frames(), data);
}

#[tokio::test(start_paused = true)]
async fn test_create_session_merge_scalar_and_array_roots_are_one_frame() {
    for data in [json!(7), json!([1, 2, 3]), json!({})] {
        let plan = Harness::new()
            .stream_all(data.clone(), options(Framing::Merge, 1024))
            .await;

        assert_eq!(plan.frames.len(), 1, "{data}");
        assert_eq!(plan.frames[0]["data"]["payload"], data);
    }
}

// ---------------------------------------------------------------------------
// Priorities
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn test_create_session_priority_mapping_orders_frames_by_priority_descending() {
    let data = json!({"lo": "a".repeat(900), "hi": "b".repeat(900), "mid": "c".repeat(900)});
    let mapping = HashMap::from([
        ("hi".to_owned(), priority(90)),
        ("lo".to_owned(), priority(10)),
    ]);

    for framing in [Framing::Merge, Framing::Patch] {
        let plan = Harness::new()
            .stream_all(
                data.clone(),
                StreamOptions {
                    framing,
                    max_frame_size: 2048,
                    priority_mapping: Some(mapping.clone()),
                    ..StreamOptions::default()
                },
            )
            .await;

        let priorities = plan.priorities();
        assert!(
            priorities.windows(2).all(|w| w[0] >= w[1]),
            "{framing:?}: priorities must not increase: {priorities:?}"
        );
        assert_eq!(priorities.first(), Some(&90), "{framing:?}");
        assert_eq!(priorities.last(), Some(&10), "{framing:?}");
        assert!(
            priorities.contains(&80),
            "{framing:?}: unmapped keys get HIGH"
        );
        let applied = match framing {
            Framing::Merge => plan.apply_merge_frames(),
            Framing::Patch => plan.apply_patches(),
        };
        assert_eq!(applied, data, "{framing:?}");
    }
}

// ---------------------------------------------------------------------------
// Options validation
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn test_create_session_rejects_max_frame_size_below_minimum() {
    let harness = Harness::new();
    let id = SessionId::new();

    let result = harness
        .controller
        .create_session(
            id,
            harness.owner,
            json!({"a": 1}),
            options(Framing::Merge, 1023),
        )
        .await;

    assert_eq!(
        result,
        Err(CreateSessionError::InvalidOptions(
            StreamOptionsError::MaxFrameSizeTooSmall {
                requested: 1023,
                min: 1024
            }
        ))
    );
    harness
        .controller
        .create_session(
            id,
            harness.owner,
            json!({"a": 1}),
            options(Framing::Merge, 1024),
        )
        .await
        .expect("a rejected request leaves no session behind, and 1024 is accepted");
}

#[tokio::test(start_paused = true)]
async fn test_create_session_rejects_zero_client_fps() {
    let harness = Harness::new();

    let result = harness
        .controller
        .create_session(
            SessionId::new(),
            harness.owner,
            json!({}),
            StreamOptions {
                client_fps: Some(0),
                ..StreamOptions::default()
            },
        )
        .await;

    assert_eq!(
        result,
        Err(CreateSessionError::InvalidOptions(
            StreamOptionsError::ZeroClientFps
        ))
    );
}

// ---------------------------------------------------------------------------
// Per-connection plan budget
// ---------------------------------------------------------------------------

fn maximal_plan_data() -> Value {
    json!({"blob": "x".repeat(3_000_000)})
}

async fn fill_budget(harness: &Harness) -> Vec<SessionId> {
    let mut accepted = Vec::new();
    loop {
        let id = SessionId::new();
        match harness
            .controller
            .create_session(
                id,
                harness.owner,
                maximal_plan_data(),
                options(Framing::Patch, 64 * 1024),
            )
            .await
        {
            Ok(()) => accepted.push(id),
            Err(CreateSessionError::PlanRejected(PlanError::ConnectionBudgetExhausted {
                live,
                requested,
                max,
            })) => {
                assert!(live + requested > max);
                assert!(live <= max);
                return accepted;
            }
            Err(other) => panic!("unexpected error {other:?}"),
        }
        assert!(accepted.len() < 64, "budget never exhausted");
    }
}

#[tokio::test(start_paused = true)]
async fn test_create_session_connection_plan_budget_rejects_beyond_16_mib() {
    let harness = Harness::new();

    let accepted = fill_budget(&harness).await;

    assert!(
        (4..=5).contains(&accepted.len()),
        "{} sessions of ~3 MB fit a 16 MiB budget",
        accepted.len()
    );
}

#[tokio::test(start_paused = true)]
async fn test_create_session_connection_plan_budget_is_per_connection() {
    let harness = Harness::new();
    fill_budget(&harness).await;
    let other = ConnectionId::new();

    let result = harness
        .controller
        .create_session(
            SessionId::new(),
            other,
            maximal_plan_data(),
            options(Framing::Patch, 64 * 1024),
        )
        .await;

    assert_eq!(result, Ok(()));
}

#[tokio::test(start_paused = true)]
async fn test_remove_session_releases_plan_bytes() {
    let harness = Harness::new();
    let accepted = fill_budget(&harness).await;

    assert!(
        harness
            .controller
            .remove_session(harness.owner, accepted[0])
    );

    harness
        .controller
        .create_session(
            SessionId::new(),
            harness.owner,
            maximal_plan_data(),
            options(Framing::Patch, 64 * 1024),
        )
        .await
        .expect("a freed session makes room for a new maximal plan");
}

#[tokio::test(start_paused = true)]
async fn test_remove_sessions_owned_by_releases_plan_bytes_of_streaming_sessions() {
    let harness = Harness::with_channel(1, 16 * MIB);
    let accepted = fill_budget(&harness).await;
    for id in &accepted {
        harness
            .controller
            .start_streaming(harness.owner, *id)
            .expect("starts");
    }
    sleep(Duration::from_millis(100)).await;

    assert_eq!(
        harness.controller.remove_sessions_owned_by(harness.owner),
        accepted.len()
    );

    let refilled = fill_budget(&harness).await;
    assert_eq!(
        refilled.len(),
        accepted.len(),
        "aborted sessions release their unsent bytes"
    );
}

#[tokio::test(start_paused = true)]
async fn test_stream_completion_releases_plan_bytes() {
    let mut harness = Harness::new();

    for round in 0..6 {
        let plan = harness
            .stream_all(maximal_plan_data(), options(Framing::Patch, 64 * 1024))
            .await;

        assert_eq!(
            plan.apply_patches(),
            maximal_plan_data(),
            "round {round}: six sequential 3 MB streams exceed a 16 MiB budget unless each releases its bytes"
        );
    }
}

// ---------------------------------------------------------------------------
// Pacing, stall and deadline
// ---------------------------------------------------------------------------

fn one_entry_per_frame(count: usize) -> Value {
    entries(count, 700)
}

#[tokio::test(start_paused = true)]
async fn test_stream_client_fps_is_a_floor_for_short_plans() {
    let mut harness = Harness::new();
    let started = Instant::now();

    let plan = harness
        .stream_all(
            one_entry_per_frame(3),
            StreamOptions {
                max_frame_size: 1024,
                client_fps: Some(1),
                ..StreamOptions::default()
            },
        )
        .await;

    assert_eq!(plan.frames.len(), 3);
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_secs(2) && elapsed <= Duration::from_secs(5),
        "3 frames at 1 fps take about 2 s, took {elapsed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn test_stream_total_pacing_is_bounded_for_long_plans() {
    let mut harness = Harness::new();
    let started = Instant::now();

    let plan = harness
        .stream_all(
            one_entry_per_frame(100),
            StreamOptions {
                max_frame_size: 1024,
                client_fps: Some(1),
                ..StreamOptions::default()
            },
        )
        .await;

    assert_eq!(plan.frames.len(), 100);
    assert!(
        started.elapsed() <= Duration::from_secs(31),
        "pacing is capped at 30 s, took {:?}",
        started.elapsed()
    );
}

async fn session_is_live(harness: &Harness, id: SessionId) -> bool {
    matches!(
        harness
            .controller
            .create_session(id, harness.owner, json!({}), StreamOptions::default())
            .await,
        Err(CreateSessionError::AlreadyExists(_))
    )
}

#[tokio::test(start_paused = true)]
async fn test_stream_stalled_enqueue_removes_session_after_30_seconds() {
    let harness = Harness::with_channel(1, 16 * MIB);
    let id = SessionId::new();
    harness
        .start(
            id,
            one_entry_per_frame(3),
            StreamOptions {
                max_frame_size: 1024,
                ..StreamOptions::default()
            },
        )
        .await
        .expect("accepted");

    sleep(Duration::from_secs(29)).await;
    assert!(
        session_is_live(&harness, id).await,
        "29 s of stall is tolerated"
    );

    sleep(Duration::from_secs(2)).await;
    assert!(
        !session_is_live(&harness, id).await,
        "31 s of stall removes the session"
    );
}

fn spawn_slow_reader(
    mut rx: Receiver<Envelope<String>>,
    pause: Duration,
) -> tokio::task::JoinHandle<Vec<String>> {
    tokio::spawn(async move {
        let mut seen = Vec::new();
        while let Ok(Some(envelope)) = timeout(Duration::from_secs(400), rx.recv()).await {
            seen.push(envelope.into_inner());
            sleep(pause).await;
        }
        seen
    })
}

#[tokio::test(start_paused = true)]
async fn test_stream_slow_but_progressing_client_completes() {
    let Harness {
        controller,
        owner,
        rx,
    } = Harness::with_channel(1, 16 * MIB);
    let reader = spawn_slow_reader(rx, Duration::from_secs(20));
    let id = SessionId::new();
    controller
        .create_session(
            id,
            owner,
            one_entry_per_frame(4),
            StreamOptions {
                max_frame_size: 1024,
                ..StreamOptions::default()
            },
        )
        .await
        .expect("accepted");
    controller.start_streaming(owner, id).expect("starts");

    let seen = reader.await.expect("reader");

    let kinds: Vec<String> = seen.iter().map(|t| message_type(t)).collect();
    assert_eq!(
        kinds.last().map(String::as_str),
        Some("StreamComplete"),
        "{kinds:?}"
    );
    assert!(!kinds.contains(&"Error".to_owned()), "{kinds:?}");
}

#[tokio::test(start_paused = true)]
async fn test_stream_absolute_deadline_ends_session_without_stream_complete() {
    let Harness {
        controller,
        owner,
        rx,
    } = Harness::with_channel(1, 16 * MIB);
    let reader = spawn_slow_reader(rx, Duration::from_secs(25));
    let id = SessionId::new();
    controller
        .create_session(
            id,
            owner,
            one_entry_per_frame(8),
            StreamOptions {
                max_frame_size: 1024,
                ..StreamOptions::default()
            },
        )
        .await
        .expect("accepted");
    controller.start_streaming(owner, id).expect("starts");

    sleep(Duration::from_secs(130)).await;
    let reusable = controller
        .create_session(id, owner, json!({}), StreamOptions::default())
        .await;
    let seen = reader.await.expect("reader");

    assert_eq!(
        reusable,
        Ok(()),
        "the deadline removed the session at 120 s"
    );
    assert!(
        !seen.iter().any(|t| message_type(t) == "StreamComplete"),
        "a deadline-killed stream never completes"
    );
}
