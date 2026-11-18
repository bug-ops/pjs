//! Acceptance tests for #754: `SessionQueryCriteria` filters on `SessionState`
//! values, so an invalid state cannot be expressed and matching is exact.

use pjson_rs::domain::DomainError;
use pjson_rs::domain::aggregates::{StreamSession, stream_session::SessionConfig};
use pjson_rs::domain::events::SessionState;
use pjson_rs::domain::ports::{SessionPagination, SessionQueryCriteria, StreamRepositoryGat};
use pjson_rs::domain::value_objects::SessionId;
use pjson_rs::infrastructure::adapters::gat_memory_repository::GatInMemoryStreamRepository;

fn criteria(states: Vec<SessionState>) -> SessionQueryCriteria {
    SessionQueryCriteria::builder()
        .states(states)
        .build()
        .expect("non-empty states are valid")
}

async fn seeded() -> (GatInMemoryStreamRepository, SessionId, SessionId, SessionId) {
    let repo = GatInMemoryStreamRepository::new();

    let initializing = StreamSession::new(SessionConfig::default());
    let initializing_id = initializing.id();
    repo.save_session(initializing).await.unwrap();

    let mut active = StreamSession::new(SessionConfig::default());
    active.activate().unwrap();
    let active_id = active.id();
    repo.save_session(active).await.unwrap();

    let mut completed = StreamSession::new(SessionConfig::default());
    completed.activate().unwrap();
    completed.close().unwrap();
    let completed_id = completed.id();
    repo.save_session(completed).await.unwrap();

    (repo, initializing_id, active_id, completed_id)
}

async fn matching(
    repo: &GatInMemoryStreamRepository,
    criteria: SessionQueryCriteria,
) -> Vec<SessionId> {
    let result = repo
        .find_sessions_by_criteria(criteria, SessionPagination::default())
        .await
        .unwrap();
    let mut ids: Vec<SessionId> = result.sessions.iter().map(StreamSession::id).collect();
    ids.sort_by_key(SessionId::as_uuid);
    ids
}

fn sorted(mut ids: Vec<SessionId>) -> Vec<SessionId> {
    ids.sort_by_key(SessionId::as_uuid);
    ids
}

#[test]
fn session_query_criteria_exposes_the_typed_states() {
    let built = criteria(vec![SessionState::Active, SessionState::Failed]);
    assert_eq!(
        built.states(),
        Some(&[SessionState::Active, SessionState::Failed][..])
    );
    assert_eq!(SessionQueryCriteria::default().states(), None);
}

#[test]
fn session_query_criteria_build_rejects_an_empty_state_list() {
    let result = SessionQueryCriteria::builder().states(Vec::new()).build();
    assert!(
        matches!(result, Err(DomainError::InvalidInput(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn find_sessions_by_criteria_matches_exactly_the_listed_states() {
    let (repo, initializing, active, completed) = seeded().await;

    assert_eq!(
        matching(&repo, criteria(vec![SessionState::Active])).await,
        vec![active]
    );
    assert_eq!(
        matching(
            &repo,
            criteria(vec![SessionState::Initializing, SessionState::Completed])
        )
        .await,
        sorted(vec![initializing, completed])
    );
    assert_eq!(
        matching(&repo, criteria(vec![SessionState::Failed])).await,
        Vec::new()
    );
    assert_eq!(
        matching(&repo, SessionQueryCriteria::default()).await,
        sorted(vec![initializing, active, completed])
    );
}
