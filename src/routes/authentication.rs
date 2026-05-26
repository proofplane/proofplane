use std::collections::HashMap;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use tracing::Span;
use uuid::Uuid;

use crate::{
    authentication::ApiKeyAuthenticator,
    domain::{ActorContext, WorkspaceId},
    routes::error::ApiError,
};

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
    let (actor_id, api_key) = credentials_from_request(&request)?;
    let actor = authenticate_request(&state.authenticator, actor_id, api_key).await?;

    attach_actor_context(&mut request, actor);

    Ok(next.run(request).await)
}

pub(in crate::routes) async fn authorize_workspace_route(
    authenticator: &ApiKeyAuthenticator,
    path: &HashMap<String, String>,
    request: &mut Request,
) -> Result<(ActorContext, WorkspaceId), ApiError> {
    let (actor_id, api_key) = credentials_from_request(request)?;
    let actor = authenticate_request(authenticator, actor_id, api_key).await?;
    let workspace_id = workspace_id_from_path(path)?;

    attach_actor_context(request, actor.clone());

    Ok((actor, workspace_id))
}

fn credentials_from_request(request: &Request) -> Result<(String, String), ApiError> {
    let api_key = header_value(request, API_KEY_HEADER).ok_or(ApiError::Unauthorized)?;
    let actor_id = header_value(request, ACTOR_ID_HEADER).ok_or(ApiError::Unauthorized)?;

    Ok((actor_id, api_key))
}

async fn authenticate_request(
    authenticator: &ApiKeyAuthenticator,
    actor_id: String,
    api_key: String,
) -> Result<ActorContext, ApiError> {
    authenticator
        .authenticate(&actor_id, &api_key)
        .await
        .map_err(|error| {
            tracing::error!(%error, "API key authentication failed");
            ApiError::Internal
        })?
        .ok_or(ApiError::Unauthorized)
}

fn attach_actor_context(request: &mut Request, actor: ActorContext) {
    Span::current().record("actor_id", actor.id.as_str());
    request.extensions_mut().insert(actor);
}

fn workspace_id_from_path(path: &HashMap<String, String>) -> Result<WorkspaceId, ApiError> {
    path.get("workspace_id")
        .and_then(|workspace_id| Uuid::parse_str(workspace_id).ok())
        .map(WorkspaceId::from)
        .ok_or(ApiError::NotFound)
}

fn header_value(request: &Request, header: &'static str) -> Option<String> {
    request
        .headers()
        .get(header)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
