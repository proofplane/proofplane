//! Temporary compatibility boundary for auditor access grants.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::{
    application::{
        commands::{
            issue_auditor_access_grant::{
                normalize_email as normalize_command_email, IssueAuditorAccessGrant,
                IssueAuditorAccessGrantError, IssueAuditorAccessGrantHandler,
            },
            revoke_auditor_access_grant::{
                RevokeAuditorAccessGrant, RevokeAuditorAccessGrantError,
                RevokeAuditorAccessGrantHandler,
            },
        },
        ExecutionMetadata,
    },
    authentication::opaque_token::{parse_auditor_invite_secret, OpaqueTokenError},
    domain::{
        AuditReviewPeriod, AuditorAccessGrant, AuditorAccessGrantId, WorkspaceId,
        WorkspacePermission,
    },
    repository::Postgres,
};

use super::agent_connections::AgentConnectionContext;

#[derive(Clone)]
pub struct AuditorAccessGrantService {
    repository: Arc<Postgres>,
    issue_handler: IssueAuditorAccessGrantHandler,
    revoke_handler: RevokeAuditorAccessGrantHandler,
}

#[derive(Debug, Error)]
pub enum AuditorAccessGrantError {
    #[error("auditor access grant is unavailable")]
    Unavailable,
    #[error("auditor access grant request is denied")]
    Denied,
    #[error("expires_at must be in the future")]
    ExpiresAtInPast,
    #[error("auditor_email is invalid")]
    InvalidEmail,
    #[error("auditor access grant secret failed")]
    Secret(#[source] OpaqueTokenError),
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAuditorAccessGrantRequest {
    pub auditor_email: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub period: AuditReviewPeriod,
}

pub use crate::application::commands::issue_auditor_access_grant::IssuedAuditorAccessGrant;

impl AuditorAccessGrantService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self {
            issue_handler: IssueAuditorAccessGrantHandler::new(repository.clone()),
            revoke_handler: RevokeAuditorAccessGrantHandler::new(repository.clone()),
            repository,
        }
    }

    pub async fn create(
        &self,
        connection: &AgentConnectionContext,
        request: CreateAuditorAccessGrantRequest,
    ) -> Result<IssuedAuditorAccessGrant, AuditorAccessGrantError> {
        self.issue_handler
            .handle(
                IssueAuditorAccessGrant {
                    connection: *connection,
                    auditor_email: request.auditor_email,
                    expires_at: request.expires_at,
                    period: request.period,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(Into::into)
    }

    pub async fn list(
        &self,
        connection: &AgentConnectionContext,
    ) -> Result<Vec<AuditorAccessGrant>, AuditorAccessGrantError> {
        authorize(connection)?;
        Ok(self
            .repository
            .list_auditor_access_grants(connection.workspace_id)
            .await?)
    }

    pub async fn revoke(
        &self,
        connection: &AgentConnectionContext,
        grant_id: AuditorAccessGrantId,
    ) -> Result<AuditorAccessGrant, AuditorAccessGrantError> {
        self.revoke_handler
            .handle(
                RevokeAuditorAccessGrant {
                    connection: *connection,
                    grant_id,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(Into::into)
    }

    pub async fn load_for_use(
        &self,
        workspace_id: WorkspaceId,
        raw_secret: &str,
    ) -> Result<AuditorAccessGrant, AuditorAccessGrantError> {
        let digest = parse_auditor_invite_secret(raw_secret)
            .map_err(|_| AuditorAccessGrantError::Unavailable)?;
        self.repository
            .get_active_auditor_access_grant_by_digest(workspace_id, digest)
            .await?
            .ok_or(AuditorAccessGrantError::Unavailable)
    }
}

impl From<IssueAuditorAccessGrantError> for AuditorAccessGrantError {
    fn from(error: IssueAuditorAccessGrantError) -> Self {
        match error {
            IssueAuditorAccessGrantError::Denied => Self::Denied,
            IssueAuditorAccessGrantError::ExpiresAtInPast => Self::ExpiresAtInPast,
            IssueAuditorAccessGrantError::InvalidEmail => Self::InvalidEmail,
            IssueAuditorAccessGrantError::Secret(error) => Self::Secret(error),
            IssueAuditorAccessGrantError::Repository(error) => Self::Repository(error),
        }
    }
}

impl From<RevokeAuditorAccessGrantError> for AuditorAccessGrantError {
    fn from(error: RevokeAuditorAccessGrantError) -> Self {
        match error {
            RevokeAuditorAccessGrantError::Unavailable => Self::Unavailable,
            RevokeAuditorAccessGrantError::Denied => Self::Denied,
            RevokeAuditorAccessGrantError::Repository(error) => Self::Repository(error),
        }
    }
}

fn authorize(connection: &AgentConnectionContext) -> Result<(), AuditorAccessGrantError> {
    connection
        .permissions
        .has(WorkspacePermission::ManageAuditorAccess)
        .then_some(())
        .ok_or(AuditorAccessGrantError::Denied)
}

pub(crate) fn normalize_email(value: &str) -> Result<String, AuditorAccessGrantError> {
    normalize_command_email(value).map_err(Into::into)
}
