use std::{sync::Arc, time::Duration};

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use tokio::time::timeout;

use crate::{persistence::Postgres, routes::error::ApiError};

#[derive(Clone)]
pub struct ReadyState {
    pub postgres: Arc<Postgres>,
    pub dependency_timeout_ms: u64,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub fn livez_router() -> Router {
    Router::new().route("/", get(livez))
}

pub fn readyz_router(state: ReadyState) -> Router {
    Router::new().route("/", get(readyz)).with_state(state)
}

async fn livez() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn readyz(State(state): State<ReadyState>) -> Result<Json<HealthResponse>, ApiError> {
    let wait = Duration::from_millis(state.dependency_timeout_ms);

    let client = timeout(wait, state.postgres.get())
        .await
        .map_err(|_| ApiError::ReadinessTimeout)?
        .map_err(ApiError::Pool)?;

    timeout(wait, client.simple_query("SELECT 1"))
        .await
        .map_err(|_| ApiError::ReadinessTimeout)?
        .map_err(ApiError::Postgres)?;

    Ok(Json(HealthResponse { status: "ready" }))
}
