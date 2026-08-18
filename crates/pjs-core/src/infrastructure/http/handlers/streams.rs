//! Stream lifecycle handlers: create, start, generate frames, fetch, list frames.

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
};

use crate::{
    application::{
        commands::{CreateStreamCommand, GenerateFramesCommand, StartStreamCommand},
        dto::PriorityDto,
        handlers::{
            CommandHandlerGat, QueryHandlerGat, command_handlers::SessionCommandHandler,
            query_handlers::StreamQueryHandler,
        },
        queries::{FramesResponse, GetStreamFramesQuery, GetStreamQuery, StreamResponse},
    },
    domain::{
        entities::Frame,
        ports::{EventPublisherGat, StreamRepositoryGat, StreamStoreGat},
        value_objects::{Priority, StreamId},
    },
    infrastructure::{
        adapters::InMemoryFrameStore,
        http::axum_adapter::{
            FrameQueryParams, GenerateFramesRequest, GenerateFramesResponse, PjsAppState, PjsError,
            StartStreamRequest, parse_session_and_stream_id, parse_session_id,
        },
    },
};

/// Create a new stream within a session
///
/// TODO(#449): Optimize double JSON processing
/// Current: serde_json::Value -> JsonDataDto -> JsonData
/// Optimization: Direct JsonData deserialization or use sonic-rs
pub(crate) async fn create_stream<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<StartStreamRequest>,
) -> Result<Json<serde_json::Value>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id = parse_session_id(session_id)?;

    let command = CreateStreamCommand {
        session_id: session_id.into(),
        source_data: request.data,
        config: None,
    };

    let stream_id: StreamId = CommandHandlerGat::handle(&*state.command_handler, command)
        .await
        .map_err(PjsError::Application)?;

    Ok(Json(serde_json::json!({
        "stream_id": stream_id.to_string(),
        "status": "created"
    })))
}

/// Start streaming for a specific stream
pub(crate) async fn start_stream<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath((session_id, stream_id)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let (session_id, stream_id) = parse_session_and_stream_id(session_id, stream_id)?;

    let command = StartStreamCommand {
        session_id: session_id.into(),
        stream_id: stream_id.into(),
    };

    <SessionCommandHandler<R, P> as CommandHandlerGat<StartStreamCommand>>::handle(
        &*state.command_handler,
        command,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(serde_json::json!({
        "stream_id": stream_id.to_string(),
        "status": "started"
    })))
}

/// Generate priority-filtered frames for an existing stream.
///
/// Dispatches [`GenerateFramesCommand`] so the produced frames are fed into
/// the per-session dictionary-training corpus (see
/// [`SessionCommandHandler::with_dictionary_store`]). Without this route the
/// `GET /pjs/sessions/{id}/dictionary` endpoint stays at `404 Not Found` for
/// HTTP-only clients regardless of how many sessions and streams they create.
pub(crate) async fn generate_frames<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath((session_id, stream_id)): AxumPath<(String, String)>,
    request: Option<Json<GenerateFramesRequest>>,
) -> Result<Json<GenerateFramesResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let (session_id, stream_id) = parse_session_and_stream_id(session_id, stream_id)?;

    let Json(request) = request.unwrap_or_default();

    let priority_value = request
        .priority_threshold
        .unwrap_or(Priority::BACKGROUND.value());
    let priority_threshold =
        PriorityDto::new(priority_value).map_err(|e| PjsError::InvalidPriority(e.to_string()))?;
    let max_frames = request.max_frames.unwrap_or(16);

    let command = GenerateFramesCommand {
        session_id: session_id.into(),
        stream_id: stream_id.into(),
        priority_threshold,
        max_frames,
    };

    let frames: Vec<Frame> = <SessionCommandHandler<R, P> as CommandHandlerGat<
        GenerateFramesCommand,
    >>::handle(&*state.command_handler, command)
    .await
    .map_err(PjsError::Application)?;

    let frame_count = frames.len();
    Ok(Json(GenerateFramesResponse {
        frames,
        frame_count,
    }))
}

/// Get stream information
pub(crate) async fn get_stream<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath((session_id, stream_id)): AxumPath<(String, String)>,
) -> Result<Json<StreamResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let (session_id, stream_id) = parse_session_and_stream_id(session_id, stream_id)?;

    let query = GetStreamQuery {
        session_id: session_id.into(),
        stream_id: stream_id.into(),
    };

    let response = <StreamQueryHandler<R, S, InMemoryFrameStore> as QueryHandlerGat<
        GetStreamQuery,
    >>::handle(&*state.stream_query_handler, query)
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}

/// Get frames for a stream (currently returns empty; no persistent frame store exists yet)
pub(crate) async fn get_stream_frames<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath((session_id, stream_id)): AxumPath<(String, String)>,
    Query(params): Query<FrameQueryParams>,
) -> Result<Json<FramesResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let (session_id, stream_id) = parse_session_and_stream_id(session_id, stream_id)?;

    let priority_filter = params
        .priority
        .map(|p| Priority::new(p).map(Into::into))
        .transpose()
        .map_err(|e: crate::domain::DomainError| PjsError::InvalidPriority(e.to_string()))?;

    let query = GetStreamFramesQuery {
        session_id: session_id.into(),
        stream_id: stream_id.into(),
        since_sequence: params.since_sequence,
        priority_filter,
        limit: params.limit,
    };

    let response = <StreamQueryHandler<R, S, InMemoryFrameStore> as QueryHandlerGat<
        GetStreamFramesQuery,
    >>::handle(&*state.stream_query_handler, query)
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}
