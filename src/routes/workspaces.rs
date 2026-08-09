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
    application::{
        commands::{
            create_owned_workspace::{CreateOwnedWorkspace, CreateOwnedWorkspaceHandler},
            remove_workspace_member::{RemoveWorkspaceMember, RemoveWorkspaceMemberHandler},
        },
        queries::get_workspace_for_user::{GetWorkspaceForUser, GetWorkspaceForUserHandler},
        ExecutionMetadata,
    },
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims},
        UserContext,
    },
    domain::{required_text, UserId, Workspace, WorkspaceId, WorkspaceWithRole},
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    routes::{
        authentication::authenticate_user,
        error::{domain_errors, ApiError},
        me::UserRouteAuthState,
        request_context::RequestId,
    },
};

pub struct WorkspacesState<V: TokenVerifier<Claims = VerifiedClaims>> {
    pub create_owned: CreateOwnedWorkspaceHandler,
    pub get_for_user: GetWorkspaceForUserHandler,
    pub remove_member: RemoveWorkspaceMemberHandler,
    pub route_auth: UserRouteAuthState<V>,
}

impl<V: TokenVerifier<Claims = VerifiedClaims>> Clone for WorkspacesState<V> {
    fn clone(&self) -> Self {
        Self {
            create_owned: self.create_owned.clone(),
            get_for_user: self.get_for_user.clone(),
            remove_member: self.remove_member.clone(),
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
            "/workspace",
            post(create_workspace::<V>).get(get_workspace::<V>),
        )
        .route("/workspace/members/{user_id}", delete(remove_member::<V>))
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
    let command = CreateOwnedWorkspace {
        workspace_id: WorkspaceId::from(Uuid::new_v4()),
        actor_user_id: user.user_id,
        slug: body.slug,
        name,
        created_at: Utc::now(),
    };

    let created = state
        .create_owned
        .handle(command, ExecutionMetadata::for_request(request_id.0))
        .await?;
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

async fn get_workspace<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspacesState<V>>,
    Extension(user): Extension<UserContext>,
) -> Result<Json<WorkspaceWithRoleResponse>, ApiError> {
    let workspace = state
        .get_for_user
        .handle(GetWorkspaceForUser {
            user_id: user.user_id,
        })
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(workspace.into()))
}

async fn remove_member<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspacesState<V>>,
    Extension(user): Extension<UserContext>,
    Extension(request_id): Extension<RequestId>,
    Path(path): Path<MemberPath>,
) -> Result<Response, ApiError> {
    let target_user_id = UserId::from(path.user_id);

    let workspace_id = state
        .remove_member
        .handle(
            RemoveWorkspaceMember {
                actor_user_id: user.user_id,
                target_user_id,
            },
            ExecutionMetadata::for_request(request_id.0),
        )
        .await?;
    let workspace_id = Uuid::from(workspace_id);

    AuditEvent::new(
        "workspace.member_removed",
        AuditOutcome::Success,
        AuditActor::User {
            user_id: user.user_id.into(),
        },
        AuditClientType::Rest,
        "remove_workspace_member",
    )
    .workspace_id(workspace_id)
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
