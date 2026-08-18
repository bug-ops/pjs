//! Session CRUD and query handlers: create, fetch, health, list, search, stats.

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
    http::header,
};

use crate::{
    application::{
        commands::CreateSessionCommand,
        handlers::{CommandHandlerGat, QueryHandlerGat, query_handlers::SessionQueryHandler},
        queries::{
            GetActiveSessionsQuery, GetSessionHealthQuery, GetSessionQuery, GetSessionStatsQuery,
            SearchSessionsQuery, SessionFilters, SessionResponse, SessionStatsResponse,
            SessionsResponse, SortOrder,
        },
    },
    domain::{
        aggregates::stream_session::SessionConfig,
        ports::{EventPublisherGat, StreamRepositoryGat, StreamStoreGat},
        value_objects::SessionId,
    },
    infrastructure::http::axum_adapter::{
        CreateSessionRequest, CreateSessionResponse, PaginationParams, PjsAppState, PjsError,
        SearchSessionsParams, SessionHealthResponse, parse_session_id, parse_session_state,
        parse_sort_field,
    },
};

/// Create a new streaming session
pub(crate) async fn create_session<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<CreateSessionResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let config = SessionConfig {
        max_concurrent_streams: request.max_concurrent_streams.unwrap_or(10),
        session_timeout_seconds: request.timeout_seconds.unwrap_or(3600),
        default_stream_config: Default::default(),
        enable_compression: true,
        metadata: Default::default(),
    };

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .map(String::from);

    let command = CreateSessionCommand {
        config,
        client_info: request.client_info,
        user_agent,
        ip_address: None,
    };

    let session_id: SessionId = CommandHandlerGat::handle(&*state.command_handler, command)
        .await
        .map_err(PjsError::Application)?;

    let expires_at = chrono::Utc::now()
        + chrono::Duration::seconds(request.timeout_seconds.unwrap_or(3600) as i64);

    Ok(Json(CreateSessionResponse {
        session_id: session_id.to_string(),
        expires_at,
    }))
}

/// Get session information
pub(crate) async fn get_session<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<SessionResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id = parse_session_id(session_id)?;

    let query = GetSessionQuery {
        session_id: session_id.into(),
    };

    let response = <SessionQueryHandler<R> as QueryHandlerGat<GetSessionQuery>>::handle(
        &*state.session_query_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}

/// Get session health status
pub(crate) async fn session_health<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<SessionHealthResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id = parse_session_id(session_id)?;

    let query = GetSessionHealthQuery {
        session_id: session_id.into(),
    };

    let response = <SessionQueryHandler<R> as QueryHandlerGat<GetSessionHealthQuery>>::handle(
        &*state.session_query_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(SessionHealthResponse::from(response.health)))
}

/// List active sessions
pub(crate) async fn list_sessions<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<SessionsResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let query = GetActiveSessionsQuery {
        limit: params.limit,
        offset: params.offset,
    };

    let response = <SessionQueryHandler<R> as QueryHandlerGat<GetActiveSessionsQuery>>::handle(
        &*state.session_query_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}

/// Search sessions with filters and sorting.
pub(crate) async fn search_sessions<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    Query(params): Query<SearchSessionsParams>,
) -> Result<Json<SessionsResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let sort_by = params.sort_by.map(parse_sort_field).transpose()?;
    let sort_order = params.sort_order.as_deref().and_then(|s| match s {
        "ascending" | "asc" => Some(SortOrder::Ascending),
        "descending" | "desc" => Some(SortOrder::Descending),
        _ => None,
    });
    let session_state = params.state.map(parse_session_state).transpose()?;
    let query = SearchSessionsQuery {
        filters: SessionFilters {
            state: session_state,
            created_after: None,
            created_before: None,
            client_info: None,
            has_active_streams: None,
        },
        sort_by,
        sort_order,
        limit: params.limit,
        offset: params.offset,
    };
    let response = <SessionQueryHandler<R> as QueryHandlerGat<SearchSessionsQuery>>::handle(
        &*state.session_query_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;
    Ok(Json(response))
}

/// Get statistics for a session
pub(crate) async fn get_session_stats<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<SessionStatsResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id = parse_session_id(session_id)?;

    let query = GetSessionStatsQuery {
        session_id: session_id.into(),
    };

    let response = <SessionQueryHandler<R> as QueryHandlerGat<GetSessionStatsQuery>>::handle(
        &*state.session_query_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}
