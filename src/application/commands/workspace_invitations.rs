use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::{
    authentication::normalize_email,
    domain::{
        UserId, WorkspaceId, WorkspaceInvitation, WorkspaceInvitationId, WorkspaceInvitationStatus,
        WorkspaceRole,
    },
    messaging::IntegrationMessage,
    persistence::{Error as RepositoryError, NewOutboxMessage, Postgres},
    pubsub::{TopicName, MESSAGE_BUS_TOPIC},
    read_models::{WorkspaceDetails, WorkspaceWithRole},
    services::workspace_invitation_authority::{
        WorkspaceInvitationAuthority, WorkspaceInvitationAuthorityError,
    },
};

const INVITATION_LIFETIME: Duration = Duration::days(7);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInvitationMetadata {
    pub id: WorkspaceInvitationId,
    pub invited_email: String,
    pub role: WorkspaceRole,
    pub generation: i64,
    pub expires_at: DateTime<Utc>,
    pub delivery_state: &'static str,
}

impl From<&WorkspaceInvitation> for WorkspaceInvitationMetadata {
    fn from(value: &WorkspaceInvitation) -> Self {
        Self {
            id: value.id(),
            invited_email: value.invited_email().to_owned(),
            role: value.role(),
            generation: value.generation(),
            expires_at: value.expires_at(),
            delivery_state: if value.last_delivery_failure().is_some() {
                "failed"
            } else if value.delivered_generation() == Some(value.generation()) {
                "delivered"
            } else if value.queued_generation() == Some(value.generation()) {
                "queued"
            } else {
                "not_queued"
            },
        }
    }
}

pub struct CreatedWorkspaceInvitation {
    pub invitation: WorkspaceInvitationMetadata,
    pub url: url::Url,
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWorkspaceInvitation {
    pub invitation_id: WorkspaceInvitationId,
    pub actor_user_id: UserId,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub request_id: Option<Uuid>,
}

#[derive(Clone)]
pub struct CreateWorkspaceInvitationHandler {
    repository: Arc<Postgres>,
    authority: WorkspaceInvitationAuthority,
}
impl CreateWorkspaceInvitationHandler {
    pub fn new(repository: Arc<Postgres>, authority: WorkspaceInvitationAuthority) -> Self {
        Self {
            repository,
            authority,
        }
    }
    pub async fn handle(
        &self,
        command: CreateWorkspaceInvitation,
    ) -> Result<CreatedWorkspaceInvitation, CreateWorkspaceInvitationError> {
        let email =
            normalize_email(&command.email).ok_or(CreateWorkspaceInvitationError::InvalidEmail)?;
        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let Some(workspace_id) = unit_of_work
                    .reads()
                    .workspaces()
                    .resolve_id_for_member(command.actor_user_id)
                    .await?
                else {
                    return Ok(CreateOutcome::Unavailable);
                };
                let repository = unit_of_work.aggregates().workspaces();
                let Some(workspace) = repository.get(workspace_id).await? else {
                    return Ok(CreateOutcome::Unavailable);
                };
                if !matches!(
                    workspace.role_for(command.actor_user_id),
                    Some(WorkspaceRole::Owner | WorkspaceRole::Admin)
                ) {
                    return Ok(CreateOutcome::Unavailable);
                }
                if unit_of_work
                    .reads()
                    .workspace_people()
                    .member_id_by_email(workspace_id, &email)
                    .await?
                    .is_some()
                {
                    return Ok(CreateOutcome::ExistingMember);
                }
                let invitations = unit_of_work.aggregates().workspace_invitations();
                if let Some(existing) = invitations
                    .find_pending_for_email(workspace_id, &email, command.created_at)
                    .await?
                {
                    return Ok(CreateOutcome::Duplicate(WorkspaceInvitationMetadata::from(
                        &existing,
                    )));
                }
                let mut invitation = WorkspaceInvitation::create(
                    command.invitation_id,
                    workspace_id,
                    command.actor_user_id,
                    email,
                    command.created_at,
                    command.created_at + INVITATION_LIFETIME,
                )
                .map_err(|_| {
                    RepositoryError::InvariantViolation("new workspace invitation must be valid")
                })?;
                invitation
                    .queue_delivery(invitation.generation(), command.created_at)
                    .map_err(|_| {
                        RepositoryError::InvariantViolation("new invitation delivery must queue")
                    })?;
                invitations.save(&invitation).await?;
                append_delivery_command(
                    unit_of_work,
                    invitation.id(),
                    invitation.generation(),
                    command.request_id,
                )
                .await?;
                Ok(CreateOutcome::Created(invitation))
            })
            .await?;
        let invitation = match outcome {
            CreateOutcome::Created(invitation) => invitation,
            CreateOutcome::Unavailable => return Err(CreateWorkspaceInvitationError::Unavailable),
            CreateOutcome::ExistingMember => {
                return Err(CreateWorkspaceInvitationError::ExistingMember)
            }
            CreateOutcome::Duplicate(metadata) => {
                return Err(CreateWorkspaceInvitationError::Duplicate(metadata))
            }
        };
        let link = self.authority.issue(&invitation)?;
        Ok(CreatedWorkspaceInvitation {
            invitation: WorkspaceInvitationMetadata::from(&invitation),
            url: link.url,
            workspace_id: invitation.workspace_id(),
        })
    }
}

async fn append_delivery_command(
    unit_of_work: &crate::persistence::UnitOfWork<'_>,
    invitation_id: WorkspaceInvitationId,
    generation: i64,
    correlation_id: Option<Uuid>,
) -> Result<(), RepositoryError> {
    let message = NewOutboxMessage::new(
        TopicName::new(MESSAGE_BUS_TOPIC),
        IntegrationMessage::send_workspace_invitation(
            invitation_id.into(),
            generation,
            correlation_id,
            None,
        ),
    );
    unit_of_work.append_outbox_message(&message).await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResendWorkspaceInvitation {
    pub invitation_id: WorkspaceInvitationId,
    pub actor_user_id: UserId,
    pub expected_generation: i64,
    pub sent_at: DateTime<Utc>,
    pub request_id: Option<Uuid>,
}

pub struct ResentWorkspaceInvitation {
    pub invitation: WorkspaceInvitationMetadata,
    pub url: url::Url,
    pub workspace_id: WorkspaceId,
}

#[derive(Clone)]
pub struct ResendWorkspaceInvitationHandler {
    repository: Arc<Postgres>,
    authority: WorkspaceInvitationAuthority,
}

impl ResendWorkspaceInvitationHandler {
    pub fn new(repository: Arc<Postgres>, authority: WorkspaceInvitationAuthority) -> Self {
        Self {
            repository,
            authority,
        }
    }

    pub async fn handle(
        &self,
        command: ResendWorkspaceInvitation,
    ) -> Result<ResentWorkspaceInvitation, ResendWorkspaceInvitationError> {
        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let Some(workspace_id) = unit_of_work
                    .reads()
                    .workspaces()
                    .resolve_id_for_member(command.actor_user_id)
                    .await?
                else {
                    return Ok(ResendOutcome::Unavailable);
                };
                let role = unit_of_work
                    .reads()
                    .workspaces()
                    .role_for(workspace_id, command.actor_user_id)
                    .await?;
                if !matches!(role, Some(WorkspaceRole::Owner | WorkspaceRole::Admin)) {
                    return Ok(ResendOutcome::Unavailable);
                }
                let invitations = unit_of_work.aggregates().workspace_invitations();
                let Some(mut invitation) = invitations
                    .get_for_workspace(command.invitation_id, workspace_id)
                    .await?
                else {
                    return Ok(ResendOutcome::Unavailable);
                };
                match invitation.resend(
                    command.expected_generation,
                    command.sent_at,
                    command.sent_at + INVITATION_LIFETIME,
                ) {
                    Ok(()) => {}
                    Err(crate::domain::WorkspaceInvitationError::StaleGeneration) => {
                        return Ok(ResendOutcome::Stale)
                    }
                    Err(_) => return Ok(ResendOutcome::Unavailable),
                }
                invitations.save(&invitation).await?;
                append_delivery_command(
                    unit_of_work,
                    invitation.id(),
                    invitation.generation(),
                    command.request_id,
                )
                .await?;
                Ok(ResendOutcome::Resent(Box::new(invitation)))
            })
            .await?;
        let invitation = match outcome {
            ResendOutcome::Resent(invitation) => *invitation,
            ResendOutcome::Stale => return Err(ResendWorkspaceInvitationError::StaleGeneration),
            ResendOutcome::Unavailable => return Err(ResendWorkspaceInvitationError::Unavailable),
        };
        let link = self.authority.issue(&invitation)?;
        Ok(ResentWorkspaceInvitation {
            invitation: WorkspaceInvitationMetadata::from(&invitation),
            url: link.url,
            workspace_id: invitation.workspace_id(),
        })
    }
}

enum ResendOutcome {
    Resent(Box<WorkspaceInvitation>),
    Stale,
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum ResendWorkspaceInvitationError {
    #[error("workspace invitation is unavailable")]
    Unavailable,
    #[error("workspace invitation generation is stale")]
    StaleGeneration,
    #[error("workspace invitation repository error")]
    Repository(#[from] RepositoryError),
    #[error("workspace invitation authority error")]
    Authority(#[from] WorkspaceInvitationAuthorityError),
}

enum CreateOutcome {
    Created(WorkspaceInvitation),
    Unavailable,
    ExistingMember,
    Duplicate(WorkspaceInvitationMetadata),
}

#[derive(Debug, thiserror::Error)]
pub enum CreateWorkspaceInvitationError {
    #[error("invitation email is invalid")]
    InvalidEmail,
    #[error("workspace is unavailable")]
    Unavailable,
    #[error("the invited email already belongs to a workspace member")]
    ExistingMember,
    #[error("a pending invitation already exists")]
    Duplicate(WorkspaceInvitationMetadata),
    #[error("workspace invitation repository error")]
    Repository(#[from] RepositoryError),
    #[error("workspace invitation authority error")]
    Authority(#[from] WorkspaceInvitationAuthorityError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetCurrentWorkspaceInvitationLink {
    pub actor_user_id: UserId,
    pub invitation_id: WorkspaceInvitationId,
    pub now: DateTime<Utc>,
}
pub struct CurrentWorkspaceInvitationLink {
    pub invitation: WorkspaceInvitationMetadata,
    pub url: url::Url,
    pub workspace_id: WorkspaceId,
}

#[derive(Clone)]
pub struct GetCurrentWorkspaceInvitationLinkHandler {
    repository: Arc<Postgres>,
    authority: WorkspaceInvitationAuthority,
}
impl GetCurrentWorkspaceInvitationLinkHandler {
    pub fn new(repository: Arc<Postgres>, authority: WorkspaceInvitationAuthority) -> Self {
        Self {
            repository,
            authority,
        }
    }
    pub async fn handle(
        &self,
        command: GetCurrentWorkspaceInvitationLink,
    ) -> Result<CurrentWorkspaceInvitationLink, CurrentWorkspaceInvitationLinkError> {
        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let Some(workspace_id) = unit_of_work
                    .reads()
                    .workspaces()
                    .resolve_id_for_member(command.actor_user_id)
                    .await?
                else {
                    return Ok(CurrentLinkOutcome::Unavailable);
                };
                let role = unit_of_work
                    .reads()
                    .workspaces()
                    .role_for(workspace_id, command.actor_user_id)
                    .await?;
                if !matches!(role, Some(WorkspaceRole::Owner | WorkspaceRole::Admin)) {
                    return Ok(CurrentLinkOutcome::Unavailable);
                }
                let Some(invitation) = unit_of_work
                    .aggregates()
                    .workspace_invitations()
                    .get_for_workspace(command.invitation_id, workspace_id)
                    .await?
                else {
                    return Ok(CurrentLinkOutcome::Unavailable);
                };
                if invitation.status_at(command.now) != WorkspaceInvitationStatus::Pending {
                    return Ok(CurrentLinkOutcome::Unavailable);
                }
                Ok(CurrentLinkOutcome::Found(Box::new(invitation)))
            })
            .await?;
        let CurrentLinkOutcome::Found(invitation) = outcome else {
            return Err(CurrentWorkspaceInvitationLinkError::Unavailable);
        };
        let link = self.authority.issue(&invitation)?;
        Ok(CurrentWorkspaceInvitationLink {
            invitation: WorkspaceInvitationMetadata::from(invitation.as_ref()),
            url: link.url,
            workspace_id: invitation.workspace_id(),
        })
    }
}

enum CurrentLinkOutcome {
    Found(Box<WorkspaceInvitation>),
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum CurrentWorkspaceInvitationLinkError {
    #[error("workspace invitation is unavailable")]
    Unavailable,
    #[error("workspace invitation repository error")]
    Repository(#[from] RepositoryError),
    #[error("workspace invitation authority error")]
    Authority(#[from] WorkspaceInvitationAuthorityError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptWorkspaceInvitation {
    pub token: String,
    pub user_id: UserId,
    pub verified_email: String,
    pub accepted_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AcceptWorkspaceInvitationHandler {
    repository: Arc<Postgres>,
    authority: WorkspaceInvitationAuthority,
}
pub struct AcceptedWorkspaceInvitation {
    pub workspace: WorkspaceWithRole,
    pub invitation_id: WorkspaceInvitationId,
}
impl AcceptWorkspaceInvitationHandler {
    pub fn new(repository: Arc<Postgres>, authority: WorkspaceInvitationAuthority) -> Self {
        Self {
            repository,
            authority,
        }
    }
    pub async fn handle(
        &self,
        command: AcceptWorkspaceInvitation,
    ) -> Result<AcceptedWorkspaceInvitation, AcceptWorkspaceInvitationError> {
        let claims = self
            .authority
            .verify(&command.token)
            .map_err(|_| AcceptWorkspaceInvitationError::Unavailable)?;
        let email = normalize_email(&command.verified_email)
            .ok_or(AcceptWorkspaceInvitationError::EmailMismatch)?;
        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let invitations = unit_of_work.aggregates().workspace_invitations();
                let Some(mut invitation) = invitations.get(claims.invitation_id).await? else {
                    return Ok(AcceptOutcome::Unavailable);
                };
                if invitation.generation() != claims.generation
                    || invitation.expires_at() != claims.expires_at
                {
                    return Ok(AcceptOutcome::Unavailable);
                }
                if invitation.accepting_user_id() == Some(command.user_id)
                    && invitation.accepted_at().is_some()
                {
                    let workspace_id = unit_of_work
                        .reads()
                        .workspaces()
                        .resolve_id_for_member(command.user_id)
                        .await?;
                    if workspace_id != Some(invitation.workspace_id()) {
                        return Ok(AcceptOutcome::Unavailable);
                    }
                    let details = unit_of_work
                        .reads()
                        .workspaces()
                        .get(invitation.workspace_id())
                        .await?;
                    let role = unit_of_work
                        .reads()
                        .workspaces()
                        .role_for(invitation.workspace_id(), command.user_id)
                        .await?;
                    return Ok(match (details, role) {
                        (Some(workspace), Some(role)) => {
                            AcceptOutcome::Accepted(WorkspaceWithRole { workspace, role })
                        }
                        _ => AcceptOutcome::Unavailable,
                    });
                }
                if invitation
                    .ensure_current(claims.generation, claims.expires_at, command.accepted_at)
                    .is_err()
                {
                    return Ok(AcceptOutcome::Unavailable);
                }
                if invitation.invited_email() != email {
                    return Ok(AcceptOutcome::EmailMismatch);
                }
                let existing_workspace_id = unit_of_work
                    .reads()
                    .workspaces()
                    .resolve_id_for_member(command.user_id)
                    .await?;
                if existing_workspace_id.is_some_and(|id| id != invitation.workspace_id()) {
                    return Ok(AcceptOutcome::ExistingWorkspace);
                }
                let workspaces = unit_of_work.aggregates().workspaces();
                let Some(mut workspace) = workspaces.get(invitation.workspace_id()).await? else {
                    return Ok(AcceptOutcome::Unavailable);
                };
                if workspace.role_for(command.user_id).is_none() {
                    if workspace
                        .add_member(command.user_id, WorkspaceRole::Admin, command.accepted_at)
                        .is_err()
                    {
                        return Ok(AcceptOutcome::Unavailable);
                    }
                    workspaces.save(&workspace).await?;
                }
                if invitation
                    .accept(command.user_id, command.accepted_at)
                    .is_err()
                {
                    return Ok(AcceptOutcome::Unavailable);
                }
                invitations.save(&invitation).await?;
                let Some(role) = workspace.role_for(command.user_id) else {
                    return Ok(AcceptOutcome::Unavailable);
                };
                Ok(AcceptOutcome::Accepted(WorkspaceWithRole {
                    workspace: WorkspaceDetails {
                        id: workspace.id(),
                        slug: workspace.slug().map(str::to_owned),
                        name: workspace.name().to_owned(),
                        created_at: workspace.created_at(),
                    },
                    role,
                }))
            })
            .await?;
        match outcome {
            AcceptOutcome::Accepted(workspace) => Ok(AcceptedWorkspaceInvitation {
                workspace,
                invitation_id: claims.invitation_id,
            }),
            AcceptOutcome::Unavailable => Err(AcceptWorkspaceInvitationError::Unavailable),
            AcceptOutcome::EmailMismatch => Err(AcceptWorkspaceInvitationError::EmailMismatch),
            AcceptOutcome::ExistingWorkspace => {
                Err(AcceptWorkspaceInvitationError::ExistingWorkspace)
            }
        }
    }
}

enum AcceptOutcome {
    Accepted(WorkspaceWithRole),
    Unavailable,
    EmailMismatch,
    ExistingWorkspace,
}

#[derive(Debug, thiserror::Error)]
pub enum AcceptWorkspaceInvitationError {
    #[error("workspace invitation is unavailable")]
    Unavailable,
    #[error("workspace invitation email does not match the verified identity")]
    EmailMismatch,
    #[error("the user already belongs to another workspace")]
    ExistingWorkspace,
    #[error("workspace invitation repository error")]
    Repository(#[from] RepositoryError),
}
