use std::sync::Arc;

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    authentication::{opaque_token::generate_auditor_invite_secret, AgentConnectionContext},
    domain::{
        AuditReviewPeriod, AuditorAccessGrant, AuditorAccessGrantId, Sha256Digest,
        WorkspacePermission,
    },
    repository::Postgres,
};

const DEFAULT_GRANT_TTL_DAYS: i64 = 30;

#[derive(Debug, Clone)]
pub struct IssueAuditorAccessGrant {
    pub connection: AgentConnectionContext,
    pub auditor_email: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub period: AuditReviewPeriod,
}

#[derive(Clone)]
pub struct IssueAuditorAccessGrantHandler {
    repository: Arc<Postgres>,
}

#[derive(Debug)]
pub struct IssuedAuditorAccessGrant {
    pub grant: AuditorAccessGrant,
    pub raw_secret: SecretString,
}

#[derive(Debug, Error)]
pub enum IssueAuditorAccessGrantError {
    #[error("auditor access grant request is denied")]
    Denied,
    #[error("expires_at must be in the future")]
    ExpiresAtInPast,
    #[error("auditor_email is invalid")]
    InvalidEmail,
    #[error("auditor access grant secret failed")]
    Secret(#[source] crate::authentication::opaque_token::OpaqueTokenError),
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

impl IssueAuditorAccessGrantHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: IssueAuditorAccessGrant,
        _metadata: ExecutionMetadata,
    ) -> Result<IssuedAuditorAccessGrant, IssueAuditorAccessGrantError> {
        authorize(&command.connection)?;
        let auditor_email = normalize_email(&command.auditor_email)?;
        let expires_at = command.expires_at.unwrap_or_else(default_expires_at);
        if expires_at <= Utc::now() {
            return Err(IssueAuditorAccessGrantError::ExpiresAtInPast);
        }

        let issued =
            generate_auditor_invite_secret().map_err(IssueAuditorAccessGrantError::Secret)?;
        let grant_id = AuditorAccessGrantId::from(Uuid::new_v4());
        let connection = command.connection;
        let period = command.period;
        let grant = self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    let grant = AuditorAccessGrant::issue(
                        grant_id,
                        connection.workspace_id,
                        auditor_email,
                        Sha256Digest::from_bytes(*issued.digest.as_bytes()),
                        connection.user_id,
                        connection.connection_id,
                        Utc::now(),
                        expires_at,
                        period,
                    )
                    .map_err(|_| {
                        crate::repository::Error::InvariantViolation(
                            "auditor access grant issuance must be valid",
                        )
                    })?;
                    let repository = context.auditor_access_grants();
                    repository.save(&grant).await?;
                    repository
                        .get(grant.id, connection.workspace_id)
                        .await?
                        .ok_or(crate::repository::Error::InvariantViolation(
                            "saved auditor access grant must be readable",
                        ))
                },
            )
            .await?;

        Ok(IssuedAuditorAccessGrant {
            grant,
            raw_secret: issued.raw_secret,
        })
    }
}

pub fn normalize_email(value: &str) -> Result<String, IssueAuditorAccessGrantError> {
    let email = value.trim().to_ascii_lowercase();
    let Some((local, domain)) = email.split_once('@') else {
        return Err(IssueAuditorAccessGrantError::InvalidEmail);
    };
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return Err(IssueAuditorAccessGrantError::InvalidEmail);
    }
    Ok(email)
}

fn authorize(connection: &AgentConnectionContext) -> Result<(), IssueAuditorAccessGrantError> {
    connection
        .permissions
        .has(WorkspacePermission::ManageAuditorAccess)
        .then_some(())
        .ok_or(IssueAuditorAccessGrantError::Denied)
}

fn default_expires_at() -> DateTime<Utc> {
    Utc::now() + chrono::Duration::days(DEFAULT_GRANT_TTL_DAYS)
}

#[cfg(test)]
mod tests {
    use super::{authorize, normalize_email, IssueAuditorAccessGrantError};
    use crate::{
        authentication::AgentConnectionContext,
        domain::{AgentConnectionId, UserId, WorkspaceId, WorkspacePermissions},
    };
    use uuid::Uuid;

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(
            normalize_email("  Auditor@Example.COM ").unwrap(),
            "auditor@example.com"
        );
    }

    #[test]
    fn normalize_email_rejects_invalid_shape() {
        for value in ["", "auditor", "@example.com", "auditor@", "a@b@c"] {
            assert!(matches!(
                normalize_email(value),
                Err(IssueAuditorAccessGrantError::InvalidEmail)
            ));
        }
    }

    #[test]
    fn issuance_requires_auditor_access_permission() {
        let connection = AgentConnectionContext {
            user_id: UserId::from(Uuid::new_v4()),
            connection_id: AgentConnectionId::from(Uuid::new_v4()),
            workspace_id: WorkspaceId::from(Uuid::new_v4()),
            permissions: WorkspacePermissions::none(),
        };

        assert!(matches!(
            authorize(&connection),
            Err(IssueAuditorAccessGrantError::Denied)
        ));
    }
}
