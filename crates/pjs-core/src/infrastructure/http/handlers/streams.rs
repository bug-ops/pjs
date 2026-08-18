//! Stream lifecycle handlers: create, start, generate frames, fetch, list frames.

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
    http::HeaderMap,
    response::Response,
};
use futures::stream;

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
        http::{
            axum_adapter::{
                FrameQueryParams, GenerateFramesRequest, GenerateFramesResponse, PjsAppState,
                PjsError, StartStreamRequest, parse_session_and_stream_id, parse_session_id,
            },
            streaming::{
                BatchFrameStream, StreamFormat, create_streaming_response,
                create_streaming_response_with_content_type,
            },
        },
    },
};

/// Create a new stream within a session
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

/// Stream frames for a stream as an incremental, frame-delimited HTTP response.
///
/// This is a transport-level alternative to [`get_stream_frames`]: same query
/// parameters, same page (capped at `MAX_PAGINATION_LIMIT` by the query handler),
/// same session/stream existence and priority-validation errors — but the page is
/// serialized and sent one frame at a time instead of buffered into a single JSON
/// envelope.
///
/// # Content negotiation
///
/// Selected from the `Accept` header only (see [`StreamFormat::from_accept_header`]);
/// there is no query-parameter override and an unnegotiable `Accept` never yields
/// `406`.
///
/// | `Accept` prefers | Format used | Response `Content-Type` |
/// |---|---|---|
/// | `text/event-stream` | Server-Sent Events | `text/event-stream` |
/// | `application/x-ndjson` | Newline-delimited JSON | `application/x-ndjson` |
/// | anything else — absent, `*/*`, `application/json`, `application/octet-stream`, unknown | JSON (NDJSON-of-objects) | `application/x-ndjson` |
///
/// `application/octet-stream` does not select a binary wire format: this route has
/// no real binary representation, so it falls back to NDJSON like every other
/// unmatched `Accept`.
///
/// # Not a live tail
///
/// This endpoint streams **one already-materialized page**, not a persistent
/// subscription — a live tail would need a `FrameSourceGat`-backed port that does
/// not exist in production today. Its value over [`get_stream_frames`] is content
/// negotiation, chunked incremental delivery instead of one contiguous buffer, and
/// exercising this crate's streaming serialization path on live traffic.
///
/// Because the stream is finite and emits no `id:`/`retry:` sentinel, a browser
/// `EventSource` will treat clean end-of-stream as a dropped connection and redial
/// on its default ~3s interval — callers using `EventSource` against this route
/// **must** call `.close()` after the final event. Clients that want continuous,
/// live push should use the WebSocket endpoint (`ws://…/pjs/ws/{session_id}`)
/// instead; `retry:`/`Last-Event-ID` handling is out of scope for this route.
///
/// The response carries `X-Total-Count`, mirroring [`FramesResponse::total_count`],
/// since a frame-delimited stream has no envelope to carry it in-body.
pub(crate) async fn stream_stream_frames<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath((session_id, stream_id)): AxumPath<(String, String)>,
    Query(params): Query<FrameQueryParams>,
    headers: HeaderMap,
) -> Result<Response, PjsError>
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

    let total_count = response.total_count;

    // This route offers no binary wire format (no real binary encoding exists —
    // see StreamFormat::Binary's docs); normalize it to the same NDJSON fallback
    // as every other unmatched Accept.
    let format = match StreamFormat::from_accept_header(&headers) {
        StreamFormat::Binary => StreamFormat::Json,
        other => other,
    };

    let batch = BatchFrameStream::new(stream::iter(response.frames), format, 1);
    let content_type = batch.content_type();
    let mut http_response = match format {
        StreamFormat::Json => {
            create_streaming_response_with_content_type(batch.into_stream(), content_type)
        }
        _ => create_streaming_response(batch.into_stream(), format),
    }
    .map_err(|e| {
        tracing::error!(error = %e, "failed to build streaming response");
        PjsError::HttpError("failed to build streaming response".into())
    })?;

    http_response
        .headers_mut()
        .insert("X-Total-Count", axum::http::HeaderValue::from(total_count));

    Ok(http_response)
}
