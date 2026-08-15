use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    domain::{
        UserId, WorkspaceId, WorkspaceInvitationId, WorkspaceInvitationStatus, WorkspaceRole,
    },
    persistence::{Error as RepositoryError, Postgres},
    read_models::{WorkspaceInvitationMetadata, WorkspacePeople},
    services::workspace_invitation_authority::{
        WorkspaceInvitationAuthority, WorkspaceInvitationAuthorityError,
        WorkspaceInvitationAuthoritySource,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetWorkspacePeople {
    pub actor_user_id: UserId,
    pub now: DateTime<Utc>,
}

#[derive(Clone)]
pub struct GetWorkspacePeopleHandler {
    repository: Arc<Postgres>,
}
impl GetWorkspacePeopleHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: GetWorkspacePeople,
    ) -> Result<Option<WorkspacePeople>, RepositoryError> {
        let reads = self.repository.reads().await?;
        let Some(scope) = reads.workspaces().get_for_user(query.actor_user_id).await? else {
            return Ok(None);
        };
        reads
            .workspace_people()
            .get(
                scope.workspace.id,
                scope.workspace.name,
                scope.role,
                query.now,
            )
            .await
            .map(Some)
    }
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
        query: GetCurrentWorkspaceInvitationLink,
    ) -> Result<CurrentWorkspaceInvitationLink, CurrentWorkspaceInvitationLinkError> {
        let reads = self.repository.reads().await?;
        let Some(scope) = reads.workspaces().get_for_user(query.actor_user_id).await? else {
            return Err(CurrentWorkspaceInvitationLinkError::Unavailable);
        };
        if !matches!(scope.role, WorkspaceRole::Owner | WorkspaceRole::Admin) {
            return Err(CurrentWorkspaceInvitationLinkError::Unavailable);
        }
        let invitation = reads
            .workspace_people()
            .current_for_workspace(query.invitation_id, scope.workspace.id, query.now)
            .await?
            .ok_or(CurrentWorkspaceInvitationLinkError::Unavailable)?;
        let link = self.authority.issue(WorkspaceInvitationAuthoritySource {
            invitation_id: invitation.id,
            generation: invitation.generation,
            expires_at: invitation.expires_at,
        })?;
        Ok(CurrentWorkspaceInvitationLink {
            invitation: WorkspaceInvitationMetadata::from(&invitation),
            url: link.url,
            workspace_id: invitation.workspace_id,
        })
    }
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
pub struct PreviewWorkspaceInvitation {
    pub token: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInvitationPreview {
    pub workspace_name: String,
    pub invited_email: String,
    pub role: crate::domain::WorkspaceRole,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct PreviewWorkspaceInvitationHandler {
    repository: Arc<Postgres>,
    authority: WorkspaceInvitationAuthority,
}
impl PreviewWorkspaceInvitationHandler {
    pub fn new(repository: Arc<Postgres>, authority: WorkspaceInvitationAuthority) -> Self {
        Self {
            repository,
            authority,
        }
    }
    pub async fn handle(
        &self,
        query: PreviewWorkspaceInvitation,
    ) -> Result<WorkspaceInvitationPreview, PreviewWorkspaceInvitationError> {
        let claims = self
            .authority
            .verify(&query.token)
            .map_err(|_| PreviewWorkspaceInvitationError::Unavailable)?;
        let source = self
            .repository
            .reads()
            .await?
            .workspace_people()
            .invitation_preview_source(claims.invitation_id)
            .await?
            .ok_or(PreviewWorkspaceInvitationError::Unavailable)?;
        let status = if source.accepted_at.is_some() {
            WorkspaceInvitationStatus::Accepted
        } else if source.revoked_at.is_some() {
            WorkspaceInvitationStatus::Revoked
        } else if query.now >= source.expires_at {
            WorkspaceInvitationStatus::Expired
        } else {
            WorkspaceInvitationStatus::Pending
        };
        if source.invitation_id != claims.invitation_id
            || source.generation != claims.generation
            || source.expires_at != claims.expires_at
            || status != WorkspaceInvitationStatus::Pending
        {
            return Err(PreviewWorkspaceInvitationError::Unavailable);
        }
        Ok(WorkspaceInvitationPreview {
            workspace_name: source.workspace_name,
            invited_email: source.invited_email,
            role: source.role,
            expires_at: source.expires_at,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PreviewWorkspaceInvitationError {
    #[error("workspace invitation is unavailable")]
    Unavailable,
    #[error("workspace invitation repository error")]
    Repository(#[from] RepositoryError),
}
