//! HTTP handler for serving trained zstd dictionaries.
//!
//! The endpoint is placed inside `protected_routes()` so it inherits the
//! auth and rate-limit layers applied by the router factories in
//! [`crate::infrastructure::http::axum_adapter`].

use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use pjson_rs_domain::value_objects::SessionId;

use crate::{
    domain::ports::{EventPublisherGat, StreamRepositoryGat, StreamStoreGat},
    infrastructure::http::axum_adapter::{PjsAppState, PjsError},
};

/// Serve the trained zstd dictionary for a session.
///
/// # Responses
///
/// | Status | Condition |
/// |--------|-----------|
/// | `200 OK` | Dictionary is trained; body is raw bytes with `Content-Type: application/zstd-dictionary` and `Cache-Control: private, max-age=300`. |
/// | `404 Not Found` | Session unknown or training not yet complete (fewer than `N_TRAIN` samples). |
/// | `400 Bad Request` | `session_id` path segment is not a valid session UUID. |
///
/// # Caching
///
/// `Cache-Control: private, max-age=300` (5 minutes). `immutable` is omitted
/// intentionally — the session may expire before the cache TTL, and serving a
/// stale dict to a new session would cause decompression errors.
pub async fn get_session_dictionary<R, P, S>(
    Path(session_id): Path<String>,
    State(state): State<PjsAppState<R, P, S>>,
) -> Result<Response, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let sid = SessionId::from_string(&session_id)
        .map_err(|_| PjsError::InvalidSessionId(session_id.clone()))?;

    let dict = state
        .dictionary_store
        .get_dictionary(sid)
        .await
        .map_err(|e| PjsError::HttpError(e.to_string()))?;

    let Some(dict) = dict else {
        return Ok((StatusCode::NOT_FOUND, "dictionary not yet trained").into_response());
    };

    // ZstdDictionary's type invariant guarantees len() <= MAX_DICT_SIZE (112 KiB) —
    // no additional size check is needed here.
    //
    // Bytes::copy_from_slice performs a ~64 KiB memcpy per request (low frequency,
    // acceptable). The Arc remains in the store; the response body owns its slice.
    let body = Bytes::copy_from_slice(dict.as_bytes());

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zstd-dictionary"),
            (header::CACHE_CONTROL, "private, max-age=300"),
        ],
        body,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::to_bytes, http::Request, routing::get};
    use chrono::Utc;
    use std::{collections::HashMap, sync::Arc};
    use tower::ServiceExt;

    use crate::{
        compression::zstd::{MAX_DICT_SIZE, N_TRAIN, ZstdDictCompressor},
        domain::ports::dictionary_store::NoopDictionaryStore,
        domain::{
            aggregates::StreamSession,
            entities::Stream,
            events::DomainEvent,
            ports::{
                EventPublisherGat, Pagination, PriorityDistribution, SessionHealthSnapshot,
                SessionQueryCriteria, SessionQueryResult, StreamFilter, StreamRepositoryGat,
                StreamStatistics, StreamStatus, StreamStoreGat,
            },
            value_objects::StreamId,
        },
        infrastructure::{http::axum_adapter::PjsAppState, repositories::InMemoryDictionaryStore},
        security::CompressionBombDetector,
    };

    struct MockRepo(parking_lot::Mutex<HashMap<SessionId, StreamSession>>);
    impl MockRepo {
        fn new() -> Self {
            Self(parking_lot::Mutex::new(HashMap::new()))
        }
    }

    impl StreamRepositoryGat for MockRepo {
        type FindSessionFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<Option<StreamSession>>>
            + Send
            + 'a
        where
            Self: 'a;
        type SaveSessionFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;
        type RemoveSessionFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;
        type FindActiveSessionsFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<Vec<StreamSession>>>
            + Send
            + 'a
        where
            Self: 'a;
        type FindSessionsByCriteriaFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<SessionQueryResult>>
            + Send
            + 'a
        where
            Self: 'a;
        type GetSessionHealthFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<SessionHealthSnapshot>>
            + Send
            + 'a
        where
            Self: 'a;
        type SessionExistsFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<bool>> + Send + 'a
        where
            Self: 'a;

        fn find_session(&self, sid: SessionId) -> Self::FindSessionFuture<'_> {
            async move { Ok(self.0.lock().get(&sid).cloned()) }
        }
        fn save_session(&self, s: StreamSession) -> Self::SaveSessionFuture<'_> {
            async move {
                self.0.lock().insert(s.id(), s);
                Ok(())
            }
        }
        fn remove_session(&self, sid: SessionId) -> Self::RemoveSessionFuture<'_> {
            async move {
                self.0.lock().remove(&sid);
                Ok(())
            }
        }
        fn find_active_sessions(&self) -> Self::FindActiveSessionsFuture<'_> {
            async move { Ok(self.0.lock().values().cloned().collect()) }
        }
        fn find_sessions_by_criteria(
            &self,
            _: SessionQueryCriteria,
            p: Pagination,
        ) -> Self::FindSessionsByCriteriaFuture<'_> {
            async move {
                let all: Vec<_> = self.0.lock().values().cloned().collect();
                let total = all.len();
                let page: Vec<_> = all.into_iter().skip(p.offset).take(p.limit).collect();
                let has_more = p.offset + page.len() < total;
                Ok(SessionQueryResult {
                    sessions: page,
                    total_count: total,
                    has_more,
                    query_duration_ms: 0,
                    scan_limit_reached: false,
                })
            }
        }
        fn get_session_health(&self, session_id: SessionId) -> Self::GetSessionHealthFuture<'_> {
            async move {
                Ok(SessionHealthSnapshot {
                    session_id,
                    is_healthy: true,
                    active_streams: 0,
                    total_frames: 0,
                    last_activity: Utc::now(),
                    error_rate: 0.0,
                    metrics: HashMap::new(),
                })
            }
        }
        fn session_exists(&self, sid: SessionId) -> Self::SessionExistsFuture<'_> {
            async move { Ok(self.0.lock().contains_key(&sid)) }
        }
    }

    struct MockPublisher;
    impl EventPublisherGat for MockPublisher {
        type PublishFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;
        type PublishBatchFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        fn publish(&self, _: DomainEvent) -> Self::PublishFuture<'_> {
            async move { Ok(()) }
        }
        fn publish_batch(&self, _: Vec<DomainEvent>) -> Self::PublishBatchFuture<'_> {
            async move { Ok(()) }
        }
    }

    struct MockStore;
    impl StreamStoreGat for MockStore {
        type StoreStreamFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;
        type GetStreamFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<Option<Stream>>>
            + Send
            + 'a
        where
            Self: 'a;
        type DeleteStreamFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;
        type ListStreamsForSessionFuture<'a>
            =
            impl std::future::Future<Output = crate::domain::DomainResult<Vec<Stream>>> + Send + 'a
        where
            Self: 'a;
        type FindStreamsBySessionFuture<'a>
            =
            impl std::future::Future<Output = crate::domain::DomainResult<Vec<Stream>>> + Send + 'a
        where
            Self: 'a;
        type UpdateStreamStatusFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;
        type GetStreamStatisticsFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<StreamStatistics>>
            + Send
            + 'a
        where
            Self: 'a;

        fn store_stream(&self, _: Stream) -> Self::StoreStreamFuture<'_> {
            async move { Ok(()) }
        }
        fn get_stream(&self, _: StreamId) -> Self::GetStreamFuture<'_> {
            async move { Ok(None) }
        }
        fn delete_stream(&self, _: StreamId) -> Self::DeleteStreamFuture<'_> {
            async move { Ok(()) }
        }
        fn list_streams_for_session(&self, _: SessionId) -> Self::ListStreamsForSessionFuture<'_> {
            async move { Ok(vec![]) }
        }
        fn find_streams_by_session(
            &self,
            _: SessionId,
            _: StreamFilter,
        ) -> Self::FindStreamsBySessionFuture<'_> {
            async move { Ok(vec![]) }
        }
        fn update_stream_status(
            &self,
            _: StreamId,
            _: StreamStatus,
        ) -> Self::UpdateStreamStatusFuture<'_> {
            async move { Ok(()) }
        }
        fn get_stream_statistics(&self, _: StreamId) -> Self::GetStreamStatisticsFuture<'_> {
            async move {
                Ok(StreamStatistics {
                    total_frames: 0,
                    total_bytes: 0,
                    priority_distribution: PriorityDistribution::default(),
                    avg_frame_size: 0.0,
                    creation_time: Utc::now(),
                    completion_time: None,
                    processing_duration: None,
                })
            }
        }
    }

    fn build_router(
        dict_store: Arc<dyn crate::domain::ports::dictionary_store::DictionaryStore>,
    ) -> Router {
        let state = PjsAppState::<MockRepo, MockPublisher, MockStore>::with_dictionary_store(
            Arc::new(MockRepo::new()),
            Arc::new(MockPublisher),
            Arc::new(MockStore),
            dict_store,
        );
        Router::new()
            .route(
                "/pjs/sessions/{session_id}/dictionary",
                get(get_session_dictionary::<MockRepo, MockPublisher, MockStore>),
            )
            .with_state(state)
    }

    #[tokio::test]
    async fn test_dictionary_endpoint_404_when_no_dict() {
        let router = build_router(Arc::new(NoopDictionaryStore));
        let sid = SessionId::new();
        let req = Request::builder()
            .uri(format!("/pjs/sessions/{sid}/dictionary"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_dictionary_endpoint_200_after_register() {
        let store = Arc::new(InMemoryDictionaryStore::new(
            Arc::new(CompressionBombDetector::default()),
            MAX_DICT_SIZE,
        ));
        let samples: Vec<Vec<u8>> = (0..N_TRAIN)
            .map(|i| format!(r#"{{"n":{i},"v":"x"}}"#).into_bytes())
            .collect();
        let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();
        let sid = SessionId::new();
        store.register(sid, dict).unwrap();

        let router = build_router(store);
        let req = Request::builder()
            .uri(format!("/pjs/sessions/{sid}/dictionary"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/zstd-dictionary"
        );
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "private, max-age=300"
        );
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert!(!body.is_empty());
    }

    #[tokio::test]
    async fn test_dictionary_endpoint_invalid_session_id_returns_400() {
        let router = build_router(Arc::new(NoopDictionaryStore));
        let req = Request::builder()
            .uri("/pjs/sessions/not-a-valid-uuid/dictionary")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
