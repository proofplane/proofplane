use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    domain::{UserId, WorkspaceInvitationStatus},
    persistence::{Error as RepositoryError, Postgres},
    read_models::WorkspacePeople,
    services::workspace_invitation_authority::WorkspaceInvitationAuthority,
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
        self.repository
            .reads()
            .await?
            .workspace_people()
            .get(query.actor_user_id, query.now)
            .await
    }
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
