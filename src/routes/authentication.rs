use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use tracing::Span;

use crate::{authentication::ApiKeyAuthenticator, routes::error::ApiError};

pub const ACTOR_ID_HEADER: &str = "x-proofplane-actor-id";
pub const API_KEY_HEADER: &str = "x-proofplane-api-key";

#[derive(Clone)]
pub struct ApiKeyState {
    pub authenticator: ApiKeyAuthenticator,
}

pub async fn require_api_key(
    State(state): State<ApiKeyState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let api_key = request
        .headers()
        .get(API_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let actor_id = request
        .headers()
        .get(ACTOR_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;

    let actor = state
        .authenticator
        .authenticate(actor_id, api_key)
        .await
        .map_err(|error| {
            tracing::error!(%error, "API key authentication failed");
            ApiError::Internal
        })?
        .ok_or(ApiError::Unauthorized)?;

    Span::current().record("actor_id", actor.id.as_str());
    request.extensions_mut().insert(actor);

    Ok(next.run(request).await)
}
