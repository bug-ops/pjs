//! Acceptance tests for #673 (offload seam), #671 (permits, pre-plan budget
//! reject) and the `requested` meaning change.
//!
//! Child of `controller`, so it reaches the private `offload_plan` seam:
//! `async fn offload_plan<F>(permits: Arc<Semaphore>, plan: F) -> Result<Plan, PlanError>`
//! with `F: FnOnce() -> Result<Plan, PlanError> + Send + 'static`.

use super::{AdaptiveStreamController, ConnectionId, CreateSessionError, offload_plan};
use crate::{
    domain::SessionId,
    error::PlanError,
    infrastructure::websocket::{
        StreamOptions,
        planner::{Plan, PlanOptions, plan_stream},
    },
};
use serde_json::{Value, json};
use std::{
    future::Future,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Waker},
    time::Duration,
};
use tokio::sync::Semaphore;

const WAIT: Duration = Duration::from_secs(10);

fn small_plan() -> Result<Plan, PlanError> {
    let options = PlanOptions::from(&StreamOptions::default());
    plan_stream(SessionId::new(), json!({"a": 1}), &options)
}

#[tokio::test]
async fn test_offload_plan_returns_the_plan_the_closure_computed() {
    let expected = small_plan().expect("plans");

    let plan = tokio::time::timeout(WAIT, offload_plan(Arc::new(Semaphore::new(1)), small_plan))
        .await
        .expect("no hang")
        .expect("plan passes through");

    assert_eq!(plan.messages.len(), expected.messages.len());
    assert_eq!(plan.total_bytes, expected.total_bytes);
}

#[tokio::test]
async fn test_offload_plan_returns_the_planner_error_unchanged() {
    let result = tokio::time::timeout(
        WAIT,
        offload_plan(Arc::new(Semaphore::new(1)), || {
            Err(PlanError::Unserializable)
        }),
    )
    .await
    .expect("no hang");

    assert_eq!(result.unwrap_err(), PlanError::Unserializable);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_offload_plan_resumes_a_planner_panic_with_its_payload() {
    let task = tokio::spawn(offload_plan(Arc::new(Semaphore::new(1)), || {
        panic!("planner boom")
    }));

    let joined = tokio::time::timeout(WAIT, task).await.expect("no hang");

    let payload = joined.expect_err("the panic must surface").into_panic();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"planner boom"));
}

/// `spawn_blocking` after `shutdown_background` yields an already cancelled
/// join handle, so `Pending` is the only outcome that is not an error.
#[test]
fn test_offload_plan_stays_pending_when_the_blocking_task_is_cancelled() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .build()
        .expect("runtime");
    let handle = runtime.handle().clone();
    runtime.shutdown_background();
    let _entered = handle.enter();
    let mut future = std::pin::pin!(offload_plan(Arc::new(Semaphore::new(1)), || {
        unreachable!("a cancelled blocking task never runs")
    }));

    // Poll the outer future only: re-polling the finished inner handle panics.
    let first = future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()));

    assert!(
        first.is_pending(),
        "cancellation must not resolve the future"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_offload_plan_keeps_its_permit_while_the_plan_runs_after_the_future_is_dropped() {
    let permits = Arc::new(Semaphore::new(1));
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let task = tokio::spawn(offload_plan(Arc::clone(&permits), move || {
        let _ = started_tx.send(());
        let _ = release_rx.recv_timeout(WAIT);
        small_plan()
    }));
    tokio::time::timeout(WAIT, started_rx)
        .await
        .expect("plan starts")
        .expect("started signal");

    task.abort();
    let _ = task.await;

    assert_eq!(
        permits.available_permits(),
        0,
        "the permit travels with the running plan"
    );
    release_tx.send(()).expect("closure still waiting");
    tokio::time::timeout(WAIT, async {
        while permits.available_permits() != 1 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the permit is released once the plan finishes");
}

/// Runs `tasks` offloaded plans and returns the most that ran at once.
/// With `rendezvous`, each plan waits (bounded) until `tasks` plans run.
async fn max_concurrent_plans(permits: usize, tasks: usize, rendezvous: bool) -> usize {
    let semaphore = Arc::new(Semaphore::new(permits));
    let running = Arc::new(AtomicUsize::new(0));
    // Never decremented: a plan that leaves early cannot strand the others.
    let arrived = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..tasks {
        let (running, arrived, peak) = (
            Arc::clone(&running),
            Arc::clone(&arrived),
            Arc::clone(&peak),
        );
        handles.push(tokio::spawn(offload_plan(
            Arc::clone(&semaphore),
            move || {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                arrived.fetch_add(1, Ordering::SeqCst);
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                if rendezvous {
                    while arrived.load(Ordering::SeqCst) < tasks
                        && std::time::Instant::now() < deadline
                    {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                } else {
                    std::thread::sleep(Duration::from_millis(30));
                }
                running.fetch_sub(1, Ordering::SeqCst);
                small_plan()
            },
        )));
    }
    for handle in handles {
        tokio::time::timeout(WAIT, handle)
            .await
            .expect("no hang")
            .expect("task joins")
            .expect("plan succeeds");
    }
    peak.load(Ordering::SeqCst)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_offload_plan_runs_at_most_one_plan_per_permit() {
    assert_eq!(max_concurrent_plans(1, 4, false).await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_offload_plan_runs_plans_in_parallel_up_to_the_permit_count() {
    assert_eq!(max_concurrent_plans(2, 2, true).await, 2);
}

fn controller_with_budget(budget: usize) -> AdaptiveStreamController {
    AdaptiveStreamController::new()
        .with_connection_plan_budget(NonZeroUsize::new(budget).expect("non-zero"))
}

fn string_of(len: usize) -> Value {
    Value::String("x".repeat(len))
}

async fn create(
    controller: &AdaptiveStreamController,
    owner: ConnectionId,
    document: Value,
) -> Result<(), CreateSessionError> {
    tokio::time::timeout(
        WAIT,
        controller.create_session(SessionId::new(), owner, document, StreamOptions::default()),
    )
    .await
    .expect("no hang")
}

/// #671: with at most 32 KiB of room left, a document whose own size exceeds the
/// room is refused before planning, and `requested` is the lower bound
/// `room + 1`, not a planned size.
#[tokio::test]
async fn test_create_session_refuses_an_oversized_document_before_planning() {
    let controller = controller_with_budget(40_000);
    let owner = ConnectionId::new();
    create(&controller, owner, string_of(10_000))
        .await
        .expect("first session fits");

    let result = create(&controller, owner, string_of(40_000)).await;

    let Err(CreateSessionError::PlanRejected(PlanError::ConnectionBudgetExhausted {
        live,
        requested,
        max,
    })) = result
    else {
        panic!("expected ConnectionBudgetExhausted, got {result:?}");
    };
    assert_eq!(max, 40_000);
    assert!(
        live > 10_000 && live < max,
        "live bytes of the first plan: {live}"
    );
    assert_eq!(requested, max - live + 1);
}

#[tokio::test]
async fn test_create_session_still_accepts_a_document_that_fits_the_room() {
    let controller = controller_with_budget(40_000);
    let owner = ConnectionId::new();
    create(&controller, owner, string_of(10_000))
        .await
        .expect("first session fits");

    create(&controller, owner, string_of(5_000))
        .await
        .expect("a document well inside the room is planned and accepted");
}

/// Guard: with more than 32 KiB of room the exact post-plan check still runs and
/// reports the planned size.
#[tokio::test]
async fn test_create_session_reports_the_planned_size_when_the_room_is_large() {
    let controller = controller_with_budget(100_000);

    let result = create(&controller, ConnectionId::new(), string_of(120_000)).await;

    let Err(CreateSessionError::PlanRejected(PlanError::ConnectionBudgetExhausted {
        live,
        requested,
        max,
    })) = result
    else {
        panic!("expected ConnectionBudgetExhausted, got {result:?}");
    };
    assert_eq!((live, max), (0, 100_000));
    assert!(requested >= 120_002, "planned size, got {requested}");
}

#[test]
fn test_new_controller_has_one_plan_permit_per_cpu() {
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);

    let controller = AdaptiveStreamController::new();

    assert_eq!(controller.plan_permits.available_permits(), cpus);
}

fn live_plan_bytes(controller: &AdaptiveStreamController, owner: ConnectionId) -> usize {
    controller
        .connections
        .read()
        .get(&owner)
        .map_or(0, |entry| entry.live_plan_bytes)
}

fn budget_exhausted(result: Result<(), CreateSessionError>) -> (usize, usize, usize) {
    let Err(CreateSessionError::PlanRejected(PlanError::ConnectionBudgetExhausted {
        live,
        requested,
        max,
    })) = result
    else {
        panic!("expected ConnectionBudgetExhausted, got {result:?}");
    };
    (live, requested, max)
}

/// A document of exactly `room` serialized bytes is not provably too big, so it
/// is planned and refused by the exact check with the planned size; one byte
/// more is refused before planning with `room + 1`.
#[tokio::test]
async fn test_create_session_pre_plan_reject_starts_one_byte_above_the_room() {
    let controller = controller_with_budget(40_000);
    let owner = ConnectionId::new();
    create(&controller, owner, string_of(10_000))
        .await
        .expect("first session fits");
    let live = live_plan_bytes(&controller, owner);
    let room = 40_000 - live;

    let at_room = budget_exhausted(create(&controller, owner, string_of(room - 2)).await);
    let over_room = budget_exhausted(create(&controller, owner, string_of(room - 1)).await);

    assert_eq!(at_room.0, live);
    assert!(
        at_room.1 > room + 1,
        "planned size expected, got {}",
        at_room.1
    );
    assert_eq!(over_room, (live, room + 1, 40_000));
}
