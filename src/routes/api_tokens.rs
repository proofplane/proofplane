use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::{delete, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::{auth0::TokenVerifier, UserContext},
    domain::{
        required_text, ApiTokenId, ApiTokenWithPermissions, DomainError, WorkspaceId,
        WorkspacePermission,
    },
    routes::{
        authentication::authenticate_user,
        error::{domain_errors, ApiError},
        me::UserRouteAuthState,
    },
    services::api_tokens::{ApiTokenService, IssuedUserApiToken},
};

pub struct ApiTokensState<V: TokenVerifier> {
    pub service: ApiTokenService,
    pub route_auth: UserRouteAuthState<V>,
}

impl<V: TokenVerifier> Clone for ApiTokensState<V> {
    fn clone(&self) -> Self {
        Self {
            service: self.service.clone(),
            route_auth: self.route_auth.clone(),
        }
    }
}

pub fn router<V: TokenVerifier + 'static>(state: ApiTokensState<V>) -> Router {
    let route_auth = state.route_auth.clone();

    Router::new()
        .route(
            "/workspaces/{workspace_id}/api-tokens",
            post(create_api_token::<V>).get(list_api_tokens::<V>),
        )
        .route(
            "/workspaces/{workspace_id}/api-tokens/{token_id}",
            delete(revoke_api_token::<V>),
        )
        .route_layer(middleware::from_fn_with_state(
            route_auth,
            authenticate_user_route::<V>,
        ))
        .with_state(state)
}

async fn authenticate_user_route<V: TokenVerifier>(
    State(state): State<UserRouteAuthState<V>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    authenticate_user(&state.authenticator, &mut request).await?;

    Ok(next.run(request).await)
}

async fn create_api_token<V: TokenVerifier>(
    State(state): State<ApiTokensState<V>>,
    Extension(user): Extension<UserContext>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<CreateApiTokenRequest>,
) -> Result<Json<IssuedApiTokenResponse>, ApiError> {
    let name = required_text("name", body.name)
        .into_result()
        .map_err(domain_errors)?;
    let permissions = parse_permissions(body.permissions)?;
    let issued = state
        .service
        .create_token(
            user.user_id,
            WorkspaceId::from(workspace_id),
            name,
            body.expires_at,
            permissions,
        )
        .await?;

    Ok(Json(issued.into()))
}

async fn list_api_tokens<V: TokenVerifier>(
    State(state): State<ApiTokensState<V>>,
    Extension(user): Extension<UserContext>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<ApiTokenResponse>>, ApiError> {
    let tokens = state
        .service
        .list_tokens(user.user_id, WorkspaceId::from(workspace_id))
        .await?;

    Ok(Json(tokens.into_iter().map(Into::into).collect()))
}

async fn revoke_api_token<V: TokenVerifier>(
    State(state): State<ApiTokensState<V>>,
    Extension(user): Extension<UserContext>,
    Path(path): Path<ApiTokenPath>,
) -> Result<StatusCode, ApiError> {
    state
        .service
        .revoke_token(
            user.user_id,
            WorkspaceId::from(path.workspace_id),
            ApiTokenId::from(path.token_id),
        )
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

fn parse_permissions(values: Vec<String>) -> Result<Vec<WorkspacePermission>, ApiError> {
    let mut permissions = Vec::with_capacity(values.len());
    let mut errors: Vec<DomainError> = Vec::new();
    for value in values {
        match value.parse::<WorkspacePermission>() {
            Ok(permission) => permissions.push(permission),
            Err(error) => errors.push(error),
        }
    }

    if errors.is_empty() {
        Ok(permissions)
    } else {
        Err(domain_errors(errors))
    }
}

#[derive(Debug, Deserialize)]
struct CreateApiTokenRequest {
    name: String,
    expires_at: Option<DateTime<Utc>>,
    permissions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApiTokenPath {
    workspace_id: Uuid,
    token_id: Uuid,
}

#[derive(Debug, Serialize)]
struct ApiTokenResponse {
    id: Uuid,
    name: String,
    workspace_id: Uuid,
    permissions: Vec<&'static str>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<ApiTokenWithPermissions> for ApiTokenResponse {
    fn from(value: ApiTokenWithPermissions) -> Self {
        Self {
            id: Uuid::from(value.token.id),
            name: value.token.name,
            workspace_id: Uuid::from(value.token.workspace_id),
            permissions: value
                .permissions
                .iter()
                .map(|permission| permission.as_str())
                .collect(),
            expires_at: value.token.expires_at,
            revoked_at: value.token.revoked_at,
            last_used_at: value.token.last_used_at,
            created_at: value.token.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct IssuedApiTokenResponse {
    id: Uuid,
    name: String,
    workspace_id: Uuid,
    permissions: Vec<&'static str>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    api_key: String,
}

impl From<IssuedUserApiToken> for IssuedApiTokenResponse {
    fn from(value: IssuedUserApiToken) -> Self {
        let mut metadata = ApiTokenResponse::from(value.token);

        Self {
            id: metadata.id,
            name: metadata.name,
            workspace_id: metadata.workspace_id,
            permissions: std::mem::take(&mut metadata.permissions),
            expires_at: metadata.expires_at,
            revoked_at: metadata.revoked_at,
            last_used_at: metadata.last_used_at,
            created_at: metadata.created_at,
            api_key: value.raw_token.expose_secret().to_owned(),
        }
    }
}
