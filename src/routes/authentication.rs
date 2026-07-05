use std::collections::HashMap;

use axum::extract::Request;
use tracing::Span;
use uuid::Uuid;

use crate::{
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims},
        ApiTokenAuthenticator, ApiTokenContext, AuthError, UserAuthenticator, UserContext,
    },
    domain::WorkspaceId,
    routes::error::ApiError,
};

pub const AUTHORIZATION_HEADER: &str = "authorization";

pub(in crate::routes) async fn authorize_workspace_route(
    authenticator: &ApiTokenAuthenticator,
    path: &HashMap<String, String>,
    request: &mut Request,
) -> Result<ApiTokenContext, ApiError> {
    let token = bearer_token_from_request(request).ok_or(ApiError::Unauthorized)?;
    let workspace_id = path
        .get("workspace_id")
        .and_then(|workspace_id| Uuid::parse_str(workspace_id).ok())
        .map(WorkspaceId::from)
        .ok_or(ApiError::NotFound)?;
    let token_context = authenticator
        .authenticate(&token)
        .await
        .map_err(|error| {
            tracing::error!(%error, "API token authentication failed");
            ApiError::Internal
        })?
        .ok_or(ApiError::Unauthorized)?;

    if token_context.workspace_id != workspace_id {
        return Err(ApiError::NotFound);
    }

    attach_api_token_context(request, token_context);

    Ok(token_context)
}

pub(in crate::routes) async fn authenticate_user<V: TokenVerifier<Claims = VerifiedClaims>>(
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

fn attach_api_token_context(request: &mut Request, context: ApiTokenContext) {
    Span::current().record("user_id", context.user_id.to_string());
    Span::current().record("api_token_id", context.api_token_id.to_string());
    request.extensions_mut().insert(context);
}

fn header_value(request: &Request, header: &'static str) -> Option<String> {
    request
        .headers()
        .get(header)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
