use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::{
    application::ExecutionMetadata,
    authentication::normalize_email,
    domain::{
        InvitationAcceptance, UserId, WorkspaceId, WorkspaceInvitation, WorkspaceInvitationId,
        WorkspaceRole,
    },
    messaging::IntegrationMessage,
    persistence::{Error as RepositoryError, NewOutboxMessage, Postgres},
    pubsub::{TopicName, MESSAGE_BUS_TOPIC},
    read_models::{WorkspaceDetails, WorkspaceInvitationMetadata, WorkspaceWithRole},
    services::workspace_invitation_authority::{
        WorkspaceInvitationAuthority, WorkspaceInvitationAuthorityError,
    },
};

const INVITATION_LIFETIME: Duration = Duration::days(7);

impl From<&WorkspaceInvitation> for WorkspaceInvitationMetadata {
    fn from(value: &WorkspaceInvitation) -> Self {
        Self {
            id: value.id(),
            invited_email: value.invited_email().to_owned(),
            role: value.role(),
            generation: value.generation(),
            expires_at: value.expires_at(),
            delivery_state: value.delivery_state(),
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
        metadata: ExecutionMetadata,
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
                if let Some(existing) = unit_of_work
                    .reads()
                    .workspace_people()
                    .lock_pending_for_email(workspace_id, &email, command.created_at)
                    .await?
                {
                    return Ok(CreateOutcome::Duplicate(WorkspaceInvitationMetadata::from(
                        &existing,
                    )));
                }
                let invitations = unit_of_work.aggregates().workspace_invitations();
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
                    metadata,
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
        let link = self.authority.issue((&invitation).into())?;
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
    metadata: ExecutionMetadata,
) -> Result<(), RepositoryError> {
    let message = NewOutboxMessage::new(
        TopicName::new(MESSAGE_BUS_TOPIC),
        IntegrationMessage::send_workspace_invitation(
            invitation_id.into(),
            generation,
            metadata.correlation_id().or(metadata.request_id()),
            metadata.causation_id(),
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
        metadata: ExecutionMetadata,
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
                    metadata,
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
        let link = self.authority.issue((&invitation).into())?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevokeWorkspaceInvitation {
    pub invitation_id: WorkspaceInvitationId,
    pub actor_user_id: UserId,
    pub expected_generation: i64,
    pub revoked_at: DateTime<Utc>,
}

pub struct RevokedWorkspaceInvitation {
    pub invitation_id: WorkspaceInvitationId,
    pub workspace_id: WorkspaceId,
}

#[derive(Clone)]
pub struct RevokeWorkspaceInvitationHandler {
    repository: Arc<Postgres>,
}

impl RevokeWorkspaceInvitationHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: RevokeWorkspaceInvitation,
    ) -> Result<RevokedWorkspaceInvitation, RevokeWorkspaceInvitationError> {
        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let Some(workspace_id) = unit_of_work
                    .reads()
                    .workspaces()
                    .resolve_id_for_member(command.actor_user_id)
                    .await?
                else {
                    return Ok(RevokeOutcome::Unavailable);
                };
                let role = unit_of_work
                    .reads()
                    .workspaces()
                    .role_for(workspace_id, command.actor_user_id)
                    .await?;
                if !matches!(role, Some(WorkspaceRole::Owner | WorkspaceRole::Admin)) {
                    return Ok(RevokeOutcome::Unavailable);
                }
                let invitations = unit_of_work.aggregates().workspace_invitations();
                let Some(mut invitation) = invitations
                    .get_for_workspace(command.invitation_id, workspace_id)
                    .await?
                else {
                    return Ok(RevokeOutcome::Unavailable);
                };
                match invitation.revoke(command.expected_generation, command.revoked_at) {
                    Ok(()) => {}
                    Err(crate::domain::WorkspaceInvitationError::StaleGeneration) => {
                        return Ok(RevokeOutcome::StaleGeneration)
                    }
                    Err(_) => return Ok(RevokeOutcome::Unavailable),
                }
                invitations.save(&invitation).await?;
                Ok(RevokeOutcome::Revoked(workspace_id))
            })
            .await?;
        match outcome {
            RevokeOutcome::Revoked(workspace_id) => Ok(RevokedWorkspaceInvitation {
                invitation_id: command.invitation_id,
                workspace_id,
            }),
            RevokeOutcome::StaleGeneration => Err(RevokeWorkspaceInvitationError::StaleGeneration),
            RevokeOutcome::Unavailable => Err(RevokeWorkspaceInvitationError::Unavailable),
        }
    }
}

enum RevokeOutcome {
    Revoked(WorkspaceId),
    StaleGeneration,
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum RevokeWorkspaceInvitationError {
    #[error("workspace invitation is unavailable")]
    Unavailable,
    #[error("workspace invitation generation is stale")]
    StaleGeneration,
    #[error("workspace invitation repository error")]
    Repository(#[from] RepositoryError),
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
    pub acceptance: InvitationAcceptance,
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
                        (Some(workspace), Some(role)) => AcceptOutcome::Accepted(
                            WorkspaceWithRole { workspace, role },
                            InvitationAcceptance::Replay,
                        ),
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
                let acceptance = match invitation.accept(command.user_id, command.accepted_at) {
                    Ok(acceptance) => acceptance,
                    Err(_) => return Ok(AcceptOutcome::Unavailable),
                };
                invitations.save(&invitation).await?;
                let Some(role) = workspace.role_for(command.user_id) else {
                    return Ok(AcceptOutcome::Unavailable);
                };
                Ok(AcceptOutcome::Accepted(
                    WorkspaceWithRole {
                        workspace: WorkspaceDetails {
                            id: workspace.id(),
                            slug: workspace.slug().map(str::to_owned),
                            name: workspace.name().to_owned(),
                            created_at: workspace.created_at(),
                        },
                        role,
                    },
                    acceptance,
                ))
            })
            .await?;
        match outcome {
            AcceptOutcome::Accepted(workspace, acceptance) => Ok(AcceptedWorkspaceInvitation {
                workspace,
                invitation_id: claims.invitation_id,
                acceptance,
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
    Accepted(WorkspaceWithRole, InvitationAcceptance),
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, Utc};
    use secrecy::SecretString;
    use url::Url;
    use uuid::Uuid;

    use super::*;
    use crate::{
        config::{WorkspaceInvitationPasetoKey, WorkspaceInvitationsConfig},
        persistence::test_support,
    };

    #[tokio::test]
    async fn acceptance_rolls_back_workspace_when_invitation_save_fails() {
        let database = test_support::database().await;
        let fixture = test_support::workspace(&database.postgres, "Acceptance Rollback").await;
        let postgres = Arc::new(database.postgres);
        let invitee_id = UserId::from(Uuid::new_v4());
        let invited_email = "rollback-invitee@example.com";
        let invitation_id = WorkspaceInvitationId::from(Uuid::new_v4());
        let now = Utc::now();
        let client = postgres.get().await.unwrap();
        client
            .execute(
                "INSERT INTO workspace_memberships (user_id, workspace_id, role) VALUES ($1, $2, 'owner')",
                &[&Uuid::from(fixture.user_id), &Uuid::from(fixture.workspace_id)],
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO users (id, auth0_sub, email) VALUES ($1, $2, $3)",
                &[
                    &Uuid::from(invitee_id),
                    &format!("auth0|{}", Uuid::from(invitee_id)),
                    &invited_email,
                ],
            )
            .await
            .unwrap();
        drop(client);

        let invitation = WorkspaceInvitation::create(
            invitation_id,
            fixture.workspace_id,
            fixture.user_id,
            invited_email.to_owned(),
            now,
            now + Duration::days(7),
        )
        .unwrap();
        postgres
            .in_unit_of_work(async |unit_of_work| {
                unit_of_work
                    .aggregates()
                    .workspace_invitations()
                    .save(&invitation)
                    .await
            })
            .await
            .unwrap();

        let authority = WorkspaceInvitationAuthority::from_config(&WorkspaceInvitationsConfig {
            landing_portal_base_url: Url::parse("https://app.proofplane.test").unwrap(),
            active_key_id: "test-key".to_owned(),
            keys: vec![WorkspaceInvitationPasetoKey {
                id: "test-key".to_owned(),
                secret: SecretString::from("k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs"),
            }],
        })
        .unwrap();
        let token = authority.issue((&invitation).into()).unwrap();

        let client = postgres.get().await.unwrap();
        client
            .batch_execute(
                r#"
CREATE FUNCTION reject_invitation_acceptance() RETURNS trigger AS $$
BEGIN
    IF NEW.accepted_at IS NOT NULL THEN
        RAISE EXCEPTION 'forced invitation save failure';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER reject_invitation_acceptance
BEFORE UPDATE ON workspace_invitations
FOR EACH ROW EXECUTE FUNCTION reject_invitation_acceptance();
"#,
            )
            .await
            .unwrap();
        drop(client);

        let result = AcceptWorkspaceInvitationHandler::new(postgres.clone(), authority)
            .handle(AcceptWorkspaceInvitation {
                token: token
                    .url
                    .fragment()
                    .unwrap()
                    .strip_prefix("token=")
                    .unwrap()
                    .to_owned(),
                user_id: invitee_id,
                verified_email: invited_email.to_owned(),
                accepted_at: now + Duration::minutes(1),
            })
            .await;
        assert!(matches!(
            result,
            Err(AcceptWorkspaceInvitationError::Repository(_))
        ));

        let client = postgres.get().await.unwrap();
        let membership_count: i64 = client
            .query_one(
                "SELECT count(*) FROM workspace_memberships WHERE user_id = $1",
                &[&Uuid::from(invitee_id)],
            )
            .await
            .unwrap()
            .get(0);
        let accepted_at: Option<DateTime<Utc>> = client
            .query_one(
                "SELECT accepted_at FROM workspace_invitations WHERE id = $1",
                &[&Uuid::from(invitation_id)],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(membership_count, 0);
        assert_eq!(accepted_at, None);
    }
}
