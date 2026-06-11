use std::collections::HashMap;

use axum::extract::Request;
use tracing::Span;
use uuid::Uuid;

use crate::{
    authentication::{
        auth0::TokenVerifier, ActorContext, ApiKeyAuthenticator, AuthError, UserAuthenticator,
        UserContext,
    },
    domain::{ActorId, WorkspaceId},
    routes::error::ApiError,
};

pub const ACTOR_ID_HEADER: &str = "x-proofplane-actor-id";
pub const API_KEY_HEADER: &str = "x-proofplane-api-key";
pub const AUTHORIZATION_HEADER: &str = "authorization";

pub(in crate::routes) async fn authorize_workspace_route(
    authenticator: &ApiKeyAuthenticator,
    path: &HashMap<String, String>,
    request: &mut Request,
) -> Result<ActorContext, ApiError> {
    let (actor_id, api_key) = credentials_from_request(request)?;
    let workspace_id = path
        .get("workspace_id")
        .and_then(|workspace_id| Uuid::parse_str(workspace_id).ok())
        .map(WorkspaceId::from)
        .ok_or(ApiError::NotFound)?;
    let actor = authenticator
        .authenticate(workspace_id, actor_id, &api_key)
        .await
        .map_err(|error| {
            tracing::error!(%error, "API key authentication failed");
            ApiError::Internal
        })?
        .ok_or(ApiError::Unauthorized)?;

    attach_actor_context(request, actor);

    Ok(actor)
}

pub(in crate::routes) async fn authenticate_user<V: TokenVerifier>(
    authenticator: &UserAuthenticator<V>,
    request: &mut Request,
) -> Result<(), ApiError> {
    let token = bearer_token_from_request(request).ok_or(ApiError::Unauthorized)?;
    let user = authenticator.authenticate(&token).await.map_err(|error| {
        if let AuthError::Unauthorized(_) = error {
            return ApiError::Unauthorized;
        }

        tracing::error!(%error, "user authentication failed");
        ApiError::Internal
    })?;

    attach_user_context(request, user);

    Ok(())
}

fn bearer_token_from_request(request: &Request) -> Option<String> {
    header_value(request, AUTHORIZATION_HEADER)?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
}

fn attach_user_context(request: &mut Request, user: UserContext) {
    Span::current().record("user_id", user.user_id.to_string());
    request.extensions_mut().insert(user);
}

fn credentials_from_request(request: &Request) -> Result<(ActorId, String), ApiError> {
    let api_key = header_value(request, API_KEY_HEADER).ok_or(ApiError::Unauthorized)?;
    let actor_id = header_value(request, ACTOR_ID_HEADER)
        .and_then(|value| Uuid::parse_str(&value).ok())
        .map(ActorId::from)
        .ok_or(ApiError::Unauthorized)?;

    Ok((actor_id, api_key))
}

fn attach_actor_context(request: &mut Request, actor: ActorContext) {
    Span::current().record("actor_id", actor.id.to_string());
    request.extensions_mut().insert(actor);
}

fn header_value(request: &Request, header: &'static str) -> Option<String> {
    request
        .headers()
        .get(header)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
