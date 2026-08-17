//! System-wide health and statistics endpoints.

use axum::{Json, extract::State};

use crate::{
    application::{
        handlers::{QueryHandlerGat, query_handlers::SystemQueryHandler},
        queries::{GetSystemStatsQuery, SystemStatsResponse},
    },
    domain::ports::{EventPublisherGat, StreamRepositoryGat, StreamStoreGat},
    infrastructure::http::axum_adapter::{PjsAppState, PjsError},
};

/// System health endpoint
pub(crate) async fn system_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "features": ["pjs_streaming", "axum_integration", "gat_handlers"]
    }))
}

/// Real-time system statistics: uptime, session counts, frame throughput.
pub(crate) async fn get_system_stats<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
) -> Result<Json<SystemStatsResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let query = GetSystemStatsQuery {
        include_historical: false,
    };

    let response = <SystemQueryHandler<R> as QueryHandlerGat<GetSystemStatsQuery>>::handle(
        &*state.system_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}
