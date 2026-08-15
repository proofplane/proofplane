use axum::{
    extract::{Path, Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    application::{
        commands::workspace_invitations::{
            AcceptWorkspaceInvitation, AcceptWorkspaceInvitationError,
            AcceptWorkspaceInvitationHandler, CreateWorkspaceInvitation,
            CreateWorkspaceInvitationError, CreateWorkspaceInvitationHandler,
            ResendWorkspaceInvitation, ResendWorkspaceInvitationError,
            ResendWorkspaceInvitationHandler, RevokeWorkspaceInvitation,
            RevokeWorkspaceInvitationError, RevokeWorkspaceInvitationHandler,
        },
        queries::workspace_invitations::{
            CurrentWorkspaceInvitationLinkError, GetCurrentWorkspaceInvitationLink,
            GetCurrentWorkspaceInvitationLinkHandler, GetWorkspacePeople,
            GetWorkspacePeopleHandler, PreviewWorkspaceInvitation, PreviewWorkspaceInvitationError,
            PreviewWorkspaceInvitationHandler,
        },
        ExecutionMetadata,
    },
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims},
        UserContext,
    },
    domain::InvitationAcceptance,
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    read_models::{
        PendingWorkspaceInvitation, WorkspaceInvitationMetadata, WorkspacePeople, WorkspacePerson,
    },
    routes::{
        authentication::authenticate_user, error::ApiError, me::UserRouteAuthState,
        request_context::RequestId, workspaces::WorkspaceWithRoleResponse,
    },
};

pub struct WorkspaceInvitationsState<V: TokenVerifier<Claims = VerifiedClaims>> {
    pub create: CreateWorkspaceInvitationHandler,
    pub current_link: GetCurrentWorkspaceInvitationLinkHandler,
    pub resend: ResendWorkspaceInvitationHandler,
    pub revoke: RevokeWorkspaceInvitationHandler,
    pub preview: PreviewWorkspaceInvitationHandler,
    pub accept: AcceptWorkspaceInvitationHandler,
    pub people: GetWorkspacePeopleHandler,
    pub route_auth: UserRouteAuthState<V>,
}
impl<V: TokenVerifier<Claims = VerifiedClaims>> Clone for WorkspaceInvitationsState<V> {
    fn clone(&self) -> Self {
        Self {
            create: self.create.clone(),
            current_link: self.current_link.clone(),
            resend: self.resend.clone(),
            revoke: self.revoke.clone(),
            preview: self.preview.clone(),
            accept: self.accept.clone(),
            people: self.people.clone(),
            route_auth: self.route_auth.clone(),
        }
    }
}

pub fn router<V: TokenVerifier<Claims = VerifiedClaims> + 'static>(
    state: WorkspaceInvitationsState<V>,
) -> Router {
    let protected = Router::new()
        .route("/workspace/people", get(get_people::<V>))
        .route("/workspace/invitations", post(create_invitation::<V>))
        .route("/workspace/invitations/{id}/link", post(current_link::<V>))
        .route(
            "/workspace/invitations/{id}/resend",
            post(resend_invitation::<V>),
        )
        .route(
            "/workspace/invitations/{id}",
            delete(revoke_invitation::<V>),
        )
        .route(
            "/workspace-invitations/accept",
            post(accept_invitation::<V>),
        )
        .route_layer(middleware::from_fn_with_state(
            state.route_auth.clone(),
            authenticate_user_route::<V>,
        ));
    Router::new()
        .route(
            "/workspace-invitations/preview",
            post(preview_invitation::<V>),
        )
        .merge(protected)
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

async fn get_people<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspaceInvitationsState<V>>,
    Extension(user): Extension<UserContext>,
) -> Result<Json<WorkspacePeopleResponse>, InvitationApiError> {
    let people = state
        .people
        .handle(GetWorkspacePeople {
            actor_user_id: user.user_id,
            now: Utc::now(),
        })
        .await?
        .ok_or(InvitationApiError::Api(ApiError::NotFound))?;
    Ok(Json(people.into()))
}

async fn create_invitation<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspaceInvitationsState<V>>,
    Extension(user): Extension<UserContext>,
    Extension(request_id): Extension<RequestId>,
    Json(body): Json<CreateInvitationRequest>,
) -> Result<Json<InvitationWithLinkResponse>, InvitationApiError> {
    let created = state
        .create
        .handle(
            CreateWorkspaceInvitation {
                invitation_id: Uuid::new_v4().into(),
                actor_user_id: user.user_id,
                email: body.email,
                created_at: Utc::now(),
            },
            ExecutionMetadata::for_request(request_id.0),
        )
        .await
        .map_err(InvitationApiError::from)?;
    AuditEvent::new(
        "workspace.invitation_created",
        AuditOutcome::Success,
        AuditActor::User {
            user_id: user.user_id.into(),
        },
        AuditClientType::Rest,
        "create_workspace_invitation",
    )
    .workspace_id(created.workspace_id.into())
    .request_id(request_id.0)
    .object(AuditObject::new(
        "workspace_invitation",
        created.invitation.id.into(),
    ))
    .emit();
    Ok(Json(InvitationWithLinkResponse::new(
        created.invitation,
        created.url,
    )))
}

async fn resend_invitation<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspaceInvitationsState<V>>,
    Extension(user): Extension<UserContext>,
    Extension(request_id): Extension<RequestId>,
    Path(path): Path<InvitationPath>,
    Json(body): Json<ResendInvitationRequest>,
) -> Result<Json<InvitationWithLinkResponse>, InvitationApiError> {
    let resent = state
        .resend
        .handle(
            ResendWorkspaceInvitation {
                invitation_id: path.id.into(),
                actor_user_id: user.user_id,
                expected_generation: body.expected_generation,
                sent_at: Utc::now(),
            },
            ExecutionMetadata::for_request(request_id.0),
        )
        .await?;
    AuditEvent::new(
        "workspace.invitation_resent",
        AuditOutcome::Success,
        AuditActor::User {
            user_id: user.user_id.into(),
        },
        AuditClientType::Rest,
        "resend_workspace_invitation",
    )
    .workspace_id(resent.workspace_id.into())
    .request_id(request_id.0)
    .object(AuditObject::new(
        "workspace_invitation",
        resent.invitation.id.into(),
    ))
    .emit();
    Ok(Json(InvitationWithLinkResponse::new(
        resent.invitation,
        resent.url,
    )))
}

async fn current_link<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspaceInvitationsState<V>>,
    Extension(user): Extension<UserContext>,
    Path(path): Path<InvitationPath>,
) -> Result<Json<InvitationWithLinkResponse>, InvitationApiError> {
    let current = state
        .current_link
        .handle(GetCurrentWorkspaceInvitationLink {
            actor_user_id: user.user_id,
            invitation_id: path.id.into(),
            now: Utc::now(),
        })
        .await
        .map_err(InvitationApiError::from)?;
    Ok(Json(InvitationWithLinkResponse::new(
        current.invitation,
        current.url,
    )))
}

async fn revoke_invitation<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspaceInvitationsState<V>>,
    Extension(user): Extension<UserContext>,
    Extension(request_id): Extension<RequestId>,
    Path(path): Path<InvitationPath>,
    Json(body): Json<RevokeInvitationRequest>,
) -> Result<Json<RevokedInvitationResponse>, InvitationApiError> {
    let revoked = state
        .revoke
        .handle(RevokeWorkspaceInvitation {
            invitation_id: path.id.into(),
            actor_user_id: user.user_id,
            expected_generation: body.expected_generation,
            revoked_at: Utc::now(),
        })
        .await?;
    AuditEvent::new(
        "workspace.invitation_revoked",
        AuditOutcome::Success,
        AuditActor::User {
            user_id: user.user_id.into(),
        },
        AuditClientType::Rest,
        "revoke_workspace_invitation",
    )
    .workspace_id(revoked.workspace_id.into())
    .request_id(request_id.0)
    .object(AuditObject::new(
        "workspace_invitation",
        revoked.invitation_id.into(),
    ))
    .emit();
    Ok(Json(RevokedInvitationResponse {
        id: revoked.invitation_id.into(),
        status: "revoked",
    }))
}

async fn preview_invitation<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspaceInvitationsState<V>>,
    Json(body): Json<TokenRequest>,
) -> Result<Json<PreviewResponse>, InvitationApiError> {
    let preview = state
        .preview
        .handle(PreviewWorkspaceInvitation {
            token: body.token,
            now: Utc::now(),
        })
        .await
        .map_err(InvitationApiError::from)?;
    Ok(Json(PreviewResponse {
        workspace_name: preview.workspace_name,
        invited_email: preview.invited_email,
        role: preview.role.as_str(),
        expires_at: preview.expires_at,
    }))
}

async fn accept_invitation<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<WorkspaceInvitationsState<V>>,
    Extension(user): Extension<UserContext>,
    Extension(request_id): Extension<RequestId>,
    Json(body): Json<TokenRequest>,
) -> Result<Json<WorkspaceWithRoleResponse>, InvitationApiError> {
    let identity = user.require_verified_management_identity().map_err(|_| {
        InvitationApiError::Api(ApiError::Forbidden {
            code: "verified_email_required",
            message: "a verified email is required to accept an invitation".to_owned(),
        })
    })?;
    let accepted = state
        .accept
        .handle(AcceptWorkspaceInvitation {
            token: body.token,
            user_id: user.user_id,
            verified_email: identity.email.clone(),
            accepted_at: Utc::now(),
        })
        .await
        .map_err(InvitationApiError::from)?;
    let workspace_id = accepted.workspace.workspace.id;
    if accepted.acceptance == InvitationAcceptance::Applied {
        AuditEvent::new(
            "workspace.invitation_accepted",
            AuditOutcome::Success,
            AuditActor::User {
                user_id: user.user_id.into(),
            },
            AuditClientType::Rest,
            "accept_workspace_invitation",
        )
        .workspace_id(workspace_id.into())
        .request_id(request_id.0)
        .object(AuditObject::new(
            "workspace_invitation",
            accepted.invitation_id.into(),
        ))
        .emit();
    }
    Ok(Json(accepted.workspace.into()))
}

#[derive(Deserialize)]
struct CreateInvitationRequest {
    email: String,
}
#[derive(Deserialize)]
struct ResendInvitationRequest {
    expected_generation: i64,
}
#[derive(Deserialize)]
struct RevokeInvitationRequest {
    expected_generation: i64,
}
#[derive(Deserialize)]
struct TokenRequest {
    token: String,
}
#[derive(Deserialize)]
struct InvitationPath {
    id: Uuid,
}

#[derive(Serialize)]
struct RevokedInvitationResponse {
    id: Uuid,
    status: &'static str,
}

#[derive(Serialize)]
struct PreviewResponse {
    workspace_name: String,
    invited_email: String,
    role: &'static str,
    expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct InvitationMetadataResponse {
    id: Uuid,
    invited_email: String,
    role: &'static str,
    generation: i64,
    expires_at: DateTime<Utc>,
}
impl From<WorkspaceInvitationMetadata> for InvitationMetadataResponse {
    fn from(value: WorkspaceInvitationMetadata) -> Self {
        Self {
            id: value.id.into(),
            invited_email: value.invited_email,
            role: value.role.as_str(),
            generation: value.generation,
            expires_at: value.expires_at,
        }
    }
}

#[derive(Serialize)]
struct InvitationWithLinkResponse {
    id: Uuid,
    invited_email: String,
    role: &'static str,
    generation: i64,
    expires_at: DateTime<Utc>,
    delivery_state: &'static str,
    url: String,
}
impl InvitationWithLinkResponse {
    fn new(value: WorkspaceInvitationMetadata, url: url::Url) -> Self {
        Self {
            id: value.id.into(),
            invited_email: value.invited_email,
            role: value.role.as_str(),
            generation: value.generation,
            expires_at: value.expires_at,
            delivery_state: value.delivery_state.as_str(),
            url: url.to_string(),
        }
    }
}

#[derive(Serialize)]
struct WorkspacePeopleResponse {
    workspace: PeopleWorkspaceResponse,
    actor_role: &'static str,
    members: Vec<PersonResponse>,
    pending_invitations: Vec<PendingInvitationResponse>,
}
#[derive(Serialize)]
struct PeopleWorkspaceResponse {
    id: Uuid,
    name: String,
}
#[derive(Serialize)]
struct PersonResponse {
    user_id: Uuid,
    display_name: Option<String>,
    email: Option<String>,
    role: &'static str,
    joined_at: DateTime<Utc>,
}
#[derive(Serialize)]
struct PendingInvitationResponse {
    id: Uuid,
    invited_email: String,
    role: &'static str,
    generation: i64,
    expires_at: DateTime<Utc>,
    delivery_state: &'static str,
    queued_at: Option<DateTime<Utc>>,
    delivered_at: Option<DateTime<Utc>>,
    delivery_failed_at: Option<DateTime<Utc>>,
}
impl From<WorkspacePeople> for WorkspacePeopleResponse {
    fn from(value: WorkspacePeople) -> Self {
        Self {
            workspace: PeopleWorkspaceResponse {
                id: value.workspace_id.into(),
                name: value.workspace_name,
            },
            actor_role: value.actor_role.as_str(),
            members: value
                .members
                .into_iter()
                .map(PersonResponse::from)
                .collect(),
            pending_invitations: value
                .pending_invitations
                .into_iter()
                .map(PendingInvitationResponse::from)
                .collect(),
        }
    }
}
impl From<WorkspacePerson> for PersonResponse {
    fn from(value: WorkspacePerson) -> Self {
        Self {
            user_id: value.user_id.into(),
            display_name: value.display_name,
            email: value.email,
            role: value.role.as_str(),
            joined_at: value.joined_at,
        }
    }
}
impl From<PendingWorkspaceInvitation> for PendingInvitationResponse {
    fn from(value: PendingWorkspaceInvitation) -> Self {
        Self {
            id: value.id.into(),
            invited_email: value.invited_email,
            role: value.role.as_str(),
            generation: value.generation,
            expires_at: value.expires_at,
            delivery_state: value.delivery_state.as_str(),
            queued_at: value.queued_at,
            delivered_at: value.delivered_at,
            delivery_failed_at: value.delivery_failed_at,
        }
    }
}

enum InvitationApiError {
    Api(ApiError),
    Duplicate(WorkspaceInvitationMetadata),
}
impl From<crate::persistence::Error> for InvitationApiError {
    fn from(value: crate::persistence::Error) -> Self {
        Self::Api(ApiError::from(value))
    }
}
impl From<CreateWorkspaceInvitationError> for InvitationApiError {
    fn from(value: CreateWorkspaceInvitationError) -> Self {
        match value {
            CreateWorkspaceInvitationError::InvalidEmail => Self::Api(ApiError::BadRequest(vec![
                "email must be a valid address".to_owned(),
            ])),
            CreateWorkspaceInvitationError::Unavailable => Self::Api(ApiError::NotFound),
            CreateWorkspaceInvitationError::ExistingMember => Self::Api(ApiError::Conflict {
                code: "workspace_member_exists",
                message: "the invited email already belongs to a workspace member".to_owned(),
            }),
            CreateWorkspaceInvitationError::Duplicate(invitation) => Self::Duplicate(invitation),
            CreateWorkspaceInvitationError::Repository(error) => Self::Api(ApiError::from(error)),
            CreateWorkspaceInvitationError::Authority(_) => Self::Api(ApiError::Internal),
        }
    }
}
impl From<CurrentWorkspaceInvitationLinkError> for InvitationApiError {
    fn from(value: CurrentWorkspaceInvitationLinkError) -> Self {
        match value {
            CurrentWorkspaceInvitationLinkError::Unavailable => Self::Api(ApiError::NotFound),
            CurrentWorkspaceInvitationLinkError::Repository(error) => {
                Self::Api(ApiError::from(error))
            }
            CurrentWorkspaceInvitationLinkError::Authority(_) => Self::Api(ApiError::Internal),
        }
    }
}
impl From<ResendWorkspaceInvitationError> for InvitationApiError {
    fn from(value: ResendWorkspaceInvitationError) -> Self {
        match value {
            ResendWorkspaceInvitationError::Unavailable => Self::Api(ApiError::NotFound),
            ResendWorkspaceInvitationError::StaleGeneration => Self::Api(ApiError::Conflict {
                code: "stale_invitation_generation",
                message: "the invitation generation has changed".to_owned(),
            }),
            ResendWorkspaceInvitationError::Repository(error) => Self::Api(ApiError::from(error)),
            ResendWorkspaceInvitationError::Authority(_) => Self::Api(ApiError::Internal),
        }
    }
}
impl From<RevokeWorkspaceInvitationError> for InvitationApiError {
    fn from(value: RevokeWorkspaceInvitationError) -> Self {
        match value {
            RevokeWorkspaceInvitationError::Unavailable => Self::Api(ApiError::NotFound),
            RevokeWorkspaceInvitationError::StaleGeneration => Self::Api(ApiError::Conflict {
                code: "stale_invitation_generation",
                message: "the invitation generation has changed".to_owned(),
            }),
            RevokeWorkspaceInvitationError::Repository(error) => Self::Api(ApiError::from(error)),
        }
    }
}
impl From<PreviewWorkspaceInvitationError> for InvitationApiError {
    fn from(value: PreviewWorkspaceInvitationError) -> Self {
        match value {
            PreviewWorkspaceInvitationError::Unavailable => unavailable(),
            PreviewWorkspaceInvitationError::Repository(error) => Self::Api(ApiError::from(error)),
        }
    }
}
impl From<AcceptWorkspaceInvitationError> for InvitationApiError {
    fn from(value: AcceptWorkspaceInvitationError) -> Self {
        match value {
            AcceptWorkspaceInvitationError::Unavailable => unavailable(),
            AcceptWorkspaceInvitationError::EmailMismatch => Self::Api(ApiError::Forbidden {
                code: "invitation_email_mismatch",
                message: "the verified email does not match this invitation".to_owned(),
            }),
            AcceptWorkspaceInvitationError::ExistingWorkspace => Self::Api(ApiError::Conflict {
                code: "user_already_has_workspace",
                message: "this account already belongs to another workspace".to_owned(),
            }),
            AcceptWorkspaceInvitationError::Repository(error) => Self::Api(ApiError::from(error)),
        }
    }
}
fn unavailable() -> InvitationApiError {
    InvitationApiError::Api(ApiError::Gone {
        code: "invitation_unavailable",
        message: "this invitation is no longer available".to_owned(),
    })
}

impl IntoResponse for InvitationApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Api(error) => error.into_response(),
            Self::Duplicate(invitation) => (
                axum::http::StatusCode::CONFLICT,
                Json(DuplicateInvitationResponse {
                    error: DuplicateErrorResponse {
                        code: "invitation_already_pending",
                        message: "a pending invitation already exists",
                        details: Vec::new(),
                    },
                    invitation: invitation.into(),
                }),
            )
                .into_response(),
        }
    }
}
#[derive(Serialize)]
struct DuplicateInvitationResponse {
    error: DuplicateErrorResponse,
    invitation: InvitationMetadataResponse,
}
#[derive(Serialize)]
struct DuplicateErrorResponse {
    code: &'static str,
    message: &'static str,
    details: Vec<String>,
}
