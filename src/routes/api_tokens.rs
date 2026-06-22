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
        canonical_permissions, required_text, ApiTokenId, ApiTokenWithPermissions, DomainError,
        WorkspaceId, WorkspacePermission,
    },
    routes::{
        authentication::authenticate_user,
        error::{domain_errors, ApiError},
        me::UserRouteAuthState,
    },
    services::api_tokens::{ApiTokenService, CreateUserApiTokenPayload, IssuedUserApiToken},
    validate,
    validation::Validation,
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
    let request = body.into_new().into_result().map_err(domain_errors)?;
    let issued = state
        .service
        .create_token(user.user_id, WorkspaceId::from(workspace_id), request)
        .await?;

    Ok(Json(issued.into()))
}

async fn list_api_tokens<V: TokenVerifier>(
    State(state): State<ApiTokensState<V>>,
    Extension(user): Extension<UserContext>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<ListApiTokensResponse>, ApiError> {
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

fn validate_permissions(values: Vec<String>) -> Validation<Vec<WorkspacePermission>, DomainError> {
    let mut permissions = Vec::with_capacity(values.len());
    let mut errors: Vec<DomainError> = Vec::new();
    for value in values {
        match value.parse::<WorkspacePermission>() {
            Ok(permission) => permissions.push(permission),
            Err(error) => errors.push(error),
        }
    }

    if !errors.is_empty() {
        return Validation::invalid_many(errors);
    }

    canonical_permissions(permissions)
        .map(Validation::valid)
        .unwrap_or_else(Validation::invalid)
}

fn validate_api_token_expiration(
    value: Option<DateTime<Utc>>,
) -> Validation<DateTime<Utc>, DomainError> {
    match value {
        Some(expires_at) if expires_at > Utc::now() => Validation::valid(expires_at),
        Some(_) => Validation::invalid(DomainError::ApiTokenExpirationNotFuture),
        None => Validation::invalid(DomainError::MissingApiTokenExpiration),
    }
}

#[derive(Debug, Deserialize)]
struct CreateApiTokenRequest {
    name: String,
    expires_at: Option<DateTime<Utc>>,
    permissions: Vec<String>,
}

impl CreateApiTokenRequest {
    fn into_new(self) -> Validation<CreateUserApiTokenPayload, DomainError> {
        validate! {
            name <- required_text("name", self.name),
            expires_at <- validate_api_token_expiration(self.expires_at),
            permissions <- validate_permissions(self.permissions),
            => CreateUserApiTokenPayload {
                name,
                expires_at,
                permissions,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiTokenPath {
    workspace_id: Uuid,
    token_id: Uuid,
}

#[derive(Debug, Serialize)]
struct ApiTokenResponseDTO {
    id: Uuid,
    name: String,
    workspace_id: Uuid,
    permissions: Vec<String>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

type ListApiTokensResponse = Vec<ApiTokenResponseDTO>;

impl From<ApiTokenWithPermissions> for ApiTokenResponseDTO {
    fn from(value: ApiTokenWithPermissions) -> Self {
        Self {
            id: Uuid::from(value.token.id),
            name: value.token.name,
            workspace_id: Uuid::from(value.token.workspace_id),
            permissions: value
                .permissions
                .iter()
                .map(|permission| permission.as_str().to_owned())
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
    permissions: Vec<String>,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    api_token: String,
}

impl From<IssuedUserApiToken> for IssuedApiTokenResponse {
    fn from(value: IssuedUserApiToken) -> Self {
        let mut metadata = ApiTokenResponseDTO::from(value.token);

        Self {
            id: metadata.id,
            name: metadata.name,
            workspace_id: metadata.workspace_id,
            permissions: std::mem::take(&mut metadata.permissions),
            expires_at: metadata.expires_at,
            revoked_at: metadata.revoked_at,
            last_used_at: metadata.last_used_at,
            created_at: metadata.created_at,
            api_token: value.raw_token.expose_secret().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration as ChronoDuration, Utc};

    use super::CreateApiTokenRequest;
    use crate::domain::{DomainError, WorkspacePermission};

    #[test]
    fn token_request_maps_to_create_payload() {
        let expires_at = future_expiration();
        let payload = CreateApiTokenRequest {
            name: "CI token".to_owned(),
            expires_at: Some(expires_at),
            permissions: vec!["write_controls".to_owned(), "read_controls".to_owned()],
        }
        .into_new()
        .into_result()
        .unwrap();

        assert_eq!(payload.name, "CI token");
        assert_eq!(payload.expires_at, expires_at);
        assert_eq!(
            payload.permissions,
            vec![
                WorkspacePermission::ReadControls,
                WorkspacePermission::WriteControls,
            ]
        );
    }

    #[test]
    fn token_request_accumulates_blank_name_missing_expiration_and_duplicate_permission() {
        let errors = CreateApiTokenRequest {
            name: " ".to_owned(),
            expires_at: None,
            permissions: vec!["read_controls".to_owned(), "read_controls".to_owned()],
        }
        .into_new()
        .into_result()
        .unwrap_err();

        assert_eq!(
            errors,
            vec![
                DomainError::EmptyRequiredText { field: "name" },
                DomainError::MissingApiTokenExpiration,
                DomainError::DuplicatePermission {
                    permission: "read_controls".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn token_request_rejects_past_expiration() {
        let errors = CreateApiTokenRequest {
            name: "CI token".to_owned(),
            expires_at: Some(Utc::now() - ChronoDuration::minutes(1)),
            permissions: vec![],
        }
        .into_new()
        .into_result()
        .unwrap_err();

        assert_eq!(errors, vec![DomainError::ApiTokenExpirationNotFuture]);
    }

    #[test]
    fn token_request_accumulates_invalid_permissions() {
        let errors = CreateApiTokenRequest {
            name: "CI token".to_owned(),
            expires_at: Some(future_expiration()),
            permissions: vec!["delete_everything".to_owned(), "unknown".to_owned()],
        }
        .into_new()
        .into_result()
        .unwrap_err();

        assert_eq!(
            errors,
            vec![
                DomainError::InvalidEnumValue {
                    field: "permission",
                    value: "delete_everything".to_owned(),
                },
                DomainError::InvalidEnumValue {
                    field: "permission",
                    value: "unknown".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn token_request_canonicalizes_permission_order() {
        let payload = CreateApiTokenRequest {
            name: "CI token".to_owned(),
            expires_at: Some(future_expiration()),
            permissions: vec![
                "write_controls".to_owned(),
                "read_evidence_requests".to_owned(),
                "read_controls".to_owned(),
            ],
        }
        .into_new()
        .into_result()
        .unwrap();

        assert_eq!(
            payload.permissions,
            vec![
                WorkspacePermission::ReadEvidenceRequests,
                WorkspacePermission::ReadControls,
                WorkspacePermission::WriteControls,
            ]
        );
    }

    fn future_expiration() -> chrono::DateTime<Utc> {
        Utc::now() + ChronoDuration::days(1)
    }
}
