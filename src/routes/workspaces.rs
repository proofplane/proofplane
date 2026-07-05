use axum::{
    extract::{Path, Request, State},
    middleware::{self, Next},
    response::Response,
    routing::{delete, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims},
        UserContext,
    },
    domain::{
        required_text, CreateWorkspacePayload, UserId, Workspace, WorkspaceId, WorkspaceWithRole,
    },
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    routes::{
        authentication::authenticate_user,
        error::{domain_errors, ApiError},
        me::UserRouteAuthState,
        request_context::RequestId,
    },
    services::workspaces::WorkspaceService,
};

pub struct WorkspacesState<V: TokenVerifier<Claims = VerifiedClaims>> {
    pub service: WorkspaceService,
    pub route_auth: UserRouteAuthState<V>,
}

impl<V: TokenVerifier<Claims = VerifiedClaims>> Clone for WorkspacesState<V> {
    fn clone(&self) -> Self {
        Self {
            service: self.service.clone(),
            route_auth: self.route_auth.clone(),
        }
    }
}

pub fn router<V: TokenVerifier<Claims = VerifiedClaims> + 'static>(
    state: WorkspacesState<V>,
) -> Router {
    let route_auth = state.route_auth.clone();

    Router::new()
        .route(
            "/workspaces",
            post(create_workspace::<V>).get(list_workspaces::<V>),
        )
        .route(
            "/workspaces/{workspace_id}/members/{user_id}",
            delete(remove_member::<V>),
        )
        .route_layer(middleware::from_fn_with_state(
            route_auth,
            authenticate_user_route::<V>,
        ))
        .with_state(state)
}

async fn authenticate_user_route<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<UserRouteAuthState<V>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    authenticate_user(&state.authenticator, &mut request).await?;

    Ok(next.run(request).await)
}

async fn create_workspace<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspacesState<V>>,
    Extension(user): Extension<UserContext>,
    Extension(request_id): Extension<RequestId>,
    Json(body): Json<CreateWorkspaceRequest>,
) -> Result<Json<WorkspaceWithRoleResponse>, ApiError> {
    let name = required_text("name", body.name)
        .into_result()
        .map_err(domain_errors)?;
    let payload = CreateWorkspacePayload {
        id: None,
        slug: body.slug,
        name,
    };

    let created = state.service.create_owned(user.user_id, payload).await?;
    let workspace_id = Uuid::from(created.workspace.id);
    let user_id = Uuid::from(user.user_id);

    AuditEvent::new(
        "workspace.created",
        AuditOutcome::Success,
        AuditActor::User { user_id },
        AuditClientType::Rest,
        "create_workspace",
    )
    .workspace_id(workspace_id)
    .request_id(request_id.0)
    .object(AuditObject::new("workspace", workspace_id))
    .emit();
    AuditEvent::new(
        "workspace.member_added",
        AuditOutcome::Success,
        AuditActor::User { user_id },
        AuditClientType::Rest,
        "create_workspace_owner_membership",
    )
    .workspace_id(workspace_id)
    .request_id(request_id.0)
    .object(AuditObject::new("user", user_id))
    .emit();

    Ok(Json(created.into()))
}

async fn list_workspaces<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspacesState<V>>,
    Extension(user): Extension<UserContext>,
) -> Result<Json<ListWorkspacesResponse>, ApiError> {
    let workspaces = state.service.list_for_user(user.user_id).await?;

    Ok(Json(workspaces.into_iter().map(Into::into).collect()))
}

async fn remove_member<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspacesState<V>>,
    Extension(user): Extension<UserContext>,
    Extension(request_id): Extension<RequestId>,
    Path(path): Path<MemberPath>,
) -> Result<Response, ApiError> {
    let workspace_id = WorkspaceId::from(path.workspace_id);
    let target_user_id = UserId::from(path.user_id);

    state
        .service
        .remove_member(workspace_id, user.user_id, target_user_id)
        .await?;

    AuditEvent::new(
        "workspace.member_removed",
        AuditOutcome::Success,
        AuditActor::User {
            user_id: user.user_id.into(),
        },
        AuditClientType::Rest,
        "remove_workspace_member",
    )
    .workspace_id(path.workspace_id)
    .request_id(request_id.0)
    .object(AuditObject::new("user", target_user_id.into()))
    .emit();

    Ok(Response::new(axum::body::Body::empty()))
}

#[derive(Debug, Deserialize)]
struct CreateWorkspaceRequest {
    name: String,
    #[serde(default)]
    slug: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemberPath {
    workspace_id: Uuid,
    user_id: Uuid,
}

#[derive(Debug, Serialize)]
struct WorkspaceWithRoleResponseDTO {
    id: Uuid,
    slug: Option<String>,
    name: String,
    role: &'static str,
    created_at: DateTime<Utc>,
}

type WorkspaceWithRoleResponse = WorkspaceWithRoleResponseDTO;
type ListWorkspacesResponse = Vec<WorkspaceWithRoleResponseDTO>;

impl From<WorkspaceWithRole> for WorkspaceWithRoleResponseDTO {
    fn from(value: WorkspaceWithRole) -> Self {
        let Workspace {
            id,
            slug,
            name,
            created_at,
        } = value.workspace;

        Self {
            id: Uuid::from(id),
            slug,
            name,
            role: value.role.as_str(),
            created_at,
        }
    }
}
