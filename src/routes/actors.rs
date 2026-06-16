use api_keys_simplified::ExposeSecret;
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::{delete, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::{auth0::TokenVerifier, UserContext},
    domain::{
        required_text, ActorId, ActorKind, ActorWithPermissions, DomainError, WorkspaceId,
        WorkspacePermission,
    },
    routes::{
        authentication::authenticate_user,
        error::{domain_errors, ApiError},
        me::UserRouteAuthState,
    },
    services::actors::{ActorService, IssuedCredential},
};

pub struct ActorsState<V: TokenVerifier> {
    pub service: ActorService,
    pub route_auth: UserRouteAuthState<V>,
}

impl<V: TokenVerifier> Clone for ActorsState<V> {
    fn clone(&self) -> Self {
        Self {
            service: self.service.clone(),
            route_auth: self.route_auth.clone(),
        }
    }
}

pub fn router<V: TokenVerifier + 'static>(state: ActorsState<V>) -> Router {
    let route_auth = state.route_auth.clone();

    Router::new()
        .route(
            "/workspaces/{workspace_id}/actors",
            post(create_actor::<V>).get(list_actors::<V>),
        )
        .route(
            "/workspaces/{workspace_id}/actors/{actor_id}/credentials",
            post(issue_credential::<V>),
        )
        .route(
            "/workspaces/{workspace_id}/actors/{actor_id}/credentials/{credential_id}",
            delete(revoke_credential::<V>),
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

async fn create_actor<V: TokenVerifier>(
    State(state): State<ActorsState<V>>,
    Extension(user): Extension<UserContext>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<CreateActorRequest>,
) -> Result<Json<ActorResponse>, ApiError> {
    let display_name = required_text("display_name", body.display_name)
        .into_result()
        .map_err(domain_errors)?;
    let kind = body
        .kind
        .parse::<ActorKind>()
        .map_err(|error| domain_errors(vec![error]))?;
    let permissions = parse_permissions(body.permissions)?;

    let created = state
        .service
        .create_actor(
            user.user_id,
            WorkspaceId::from(workspace_id),
            kind,
            display_name,
            permissions,
        )
        .await?;

    Ok(Json(created.into()))
}

async fn list_actors<V: TokenVerifier>(
    State(state): State<ActorsState<V>>,
    Extension(user): Extension<UserContext>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<ActorResponse>>, ApiError> {
    let actors = state
        .service
        .list_actors(user.user_id, WorkspaceId::from(workspace_id))
        .await?;

    Ok(Json(actors.into_iter().map(Into::into).collect()))
}

async fn issue_credential<V: TokenVerifier>(
    State(state): State<ActorsState<V>>,
    Extension(user): Extension<UserContext>,
    Path(path): Path<ActorPath>,
    Json(body): Json<IssueCredentialRequest>,
) -> Result<Json<IssuedCredentialResponse>, ApiError> {
    let name = required_text("name", body.name)
        .into_result()
        .map_err(domain_errors)?;

    let issued = state
        .service
        .issue_credential(
            user.user_id,
            WorkspaceId::from(path.workspace_id),
            ActorId::from(path.actor_id),
            name,
        )
        .await?;

    Ok(Json(issued.into()))
}

async fn revoke_credential<V: TokenVerifier>(
    State(state): State<ActorsState<V>>,
    Extension(user): Extension<UserContext>,
    Path(path): Path<CredentialPath>,
) -> Result<StatusCode, ApiError> {
    state
        .service
        .revoke_credential(
            user.user_id,
            WorkspaceId::from(path.workspace_id),
            ActorId::from(path.actor_id),
            &path.credential_id,
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
struct CreateActorRequest {
    kind: String,
    display_name: String,
    permissions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct IssueCredentialRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ActorPath {
    workspace_id: Uuid,
    actor_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct CredentialPath {
    workspace_id: Uuid,
    actor_id: Uuid,
    credential_id: String,
}

#[derive(Debug, Serialize)]
struct ActorResponse {
    id: Uuid,
    kind: &'static str,
    display_name: String,
    workspace_id: Uuid,
    created_by_user_id: Option<Uuid>,
    permissions: Vec<&'static str>,
    created_at: DateTime<Utc>,
}

impl From<ActorWithPermissions> for ActorResponse {
    fn from(value: ActorWithPermissions) -> Self {
        let ActorWithPermissions { actor, permissions } = value;

        Self {
            id: Uuid::from(actor.id),
            kind: actor.kind.as_str(),
            display_name: actor.display_name,
            workspace_id: Uuid::from(actor.workspace_id),
            created_by_user_id: actor.created_by_user_id.map(Uuid::from),
            permissions: permissions
                .iter()
                .map(WorkspacePermission::as_str)
                .collect(),
            created_at: actor.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct IssuedCredentialResponse {
    id: String,
    name: String,
    /// The raw key, returned exactly once and never persisted or re-shown.
    api_key: String,
    created_at: DateTime<Utc>,
}

impl From<IssuedCredential> for IssuedCredentialResponse {
    fn from(value: IssuedCredential) -> Self {
        Self {
            id: value.id,
            name: value.name,
            api_key: value.raw_key.expose_secret().to_owned(),
            created_at: value.created_at,
        }
    }
}
